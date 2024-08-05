use std::collections::{HashMap, HashSet};
use std::sync::mpsc::TryRecvError::{Disconnected, Empty};
use std::{
    sync::mpsc::{Receiver, Sender},
    thread,
    time::Duration,
};

use crate::broadcast;
use crate::message::{Body, Payload};
use crate::raft::RaftCore;
use crate::raft::{RaftCommand, RaftRequest};
use crate::util::{get_cur_time_ms, node_id_to_raft_id, raft_id_to_node_id};
use crate::{
    broadcast::BroadcastCore,
    logger::ServerLogger,
    message::{self, Message, Payload::*},
    util::jitter,
};
use anyhow::bail;

/// Mode of operation for the server node.
/// In `Cluster` mode, an embedded list of peers is included.
#[derive(Debug)]
pub enum NodeMode {
    Singleton,
    Cluster(Vec<String>),
}

impl NodeMode {
    /// Constructs the NodeMode based on the list of neighbors.
    pub fn from_node_ids(node_ids: Vec<String>) -> Self {
        if (node_ids.len() > 1) {
            NodeMode::Cluster(node_ids)
        } else {
            NodeMode::Singleton
        }
    }
}

/// Static configurations for the server node.
#[derive(Debug)]
pub struct NodeConfig {
    base_pause_time_ms: usize,
    base_broadcast_retry_ms: usize,
    raft_request_ttl_ms: usize,
}

impl NodeConfig {
    pub fn new(
        base_pause_time_ms: usize,
        base_broadcast_retry_ms: usize,
        raft_request_ttl_ms: usize,
    ) -> Self {
        Self {
            base_pause_time_ms,
            base_broadcast_retry_ms,
            raft_request_ttl_ms,
        }
    }
}

/// The base class of the server node.
#[derive(Debug)]
pub struct Node {
    // Static configs.
    node_id: String,
    mode: NodeMode,
    config: NodeConfig,
    logger: &'static ServerLogger,

    // Communication channels.
    in_receiver: Receiver<Message>,
    out_sender: Sender<Message>,

    // Internal states.
    next_msg_id: usize,

    // Internal cores.
    broadcast_core: Option<BroadcastCore>,
    raft_core: RaftCore,

    // Pending requests to be processed by the `RaftCore`.
    // Processing requests requires mutability of the `RaftCore`, hence `Node` holds them to avoid borrowing conflict.
    pending_requests: HashMap<usize, RaftRequest>,
}

impl Node {
    pub fn new(
        node_id: String,
        mode: NodeMode,
        config: NodeConfig,
        logger: &'static ServerLogger,
        in_receiver: Receiver<Message>,
        out_sender: Sender<Message>,
        raft_core: RaftCore,
    ) -> Self {
        Self {
            node_id,
            mode,
            config,
            logger,
            in_receiver,
            out_sender,
            next_msg_id: 0,
            broadcast_core: None,
            raft_core,
            pending_requests: HashMap::new(),
        }
    }

    // Returns the ID (e.g. `n1`) of this node as a string slice.
    pub fn get_node_id(&self) -> &str {
        &self.node_id
    }

    // Checks if the node is in `Singleton` mode.
    pub fn is_singleton(&self) -> bool {
        matches!(self.mode, NodeMode::Singleton)
    }

    // Increments and returns the next message ID to be used for communication.
    pub fn vend_msg_id(&mut self) -> usize {
        self.next_msg_id += 1;
        self.next_msg_id
    }

    /// Starts the server's routine.
    pub fn orchestrate(&mut self) -> anyhow::Result<()> {
        let node_id = self.node_id.clone();
        let mut next_health_log_time = get_cur_time_ms();
        let mut next_dump_log_time = get_cur_time_ms();

        loop {
            if get_cur_time_ms() >= next_health_log_time {
                // Intent to emit a health log every 3 seconds to indicate the server is still responsive.
                self.logger
                    .log_debug(&format!("Server node: {node_id} is still operational."));
                next_health_log_time += 3000;
            }
            if get_cur_time_ms() >= next_dump_log_time {
                // Dump the internal states every 6 seconds.
                self.raft_core.dump_internal_state();
                next_dump_log_time += 6000;
            }

            // Process all pending messages currently available.
            loop {
                match self.in_receiver.try_recv() {
                    Ok(message) => {
                        self.process_message(message);
                    }
                    Err(e) => {
                        match e {
                            Empty => {
                                // No more messages at the moment, proceed to internal housekeeping.
                                break;
                            }
                            Disconnected => {
                                self.logger.log_debug(
                                    "Input channel closed. Shutting down server node...",
                                );
                                return Ok(());
                            }
                        }
                    }
                }
            }

            // Process pending broadcast tasks if necessary.
            if let Some(ref mut broadcast_core) = self.broadcast_core {
                broadcast_core.clean_tasks();
                for task in broadcast_core.tasks() {
                    if let Some(broadcast_messages) = task.generate_retry_messages(
                        &self.node_id,
                        jitter(self.config.base_broadcast_retry_ms, None),
                    ) {
                        let value = task.get_content();
                        self.logger.log_debug(&format!(
                            "Preparing to broadcast value: {value} to neighbors..."
                        ));
                        for m in broadcast_messages.into_iter() {
                            self.out_sender.send(m)?;
                        }
                    }
                }
            }

            // Processing pending Raft requests.
            let consumed_msg_ids: Vec<usize> = self
                .pending_requests
                .iter_mut()
                .filter_map(|(msg_id, request)| self.raft_core.handle_request(*msg_id, request))
                .collect();

            for id_to_dequeue in consumed_msg_ids {
                self.pending_requests.remove(&id_to_dequeue);
            }

            self.pending_requests.retain(|_, req| req.can_retry());

            // Do chores on the Raft node.
            let next_msg_id = self.vend_msg_id();
            self.raft_core.run_cycle(next_msg_id)?;

            thread::sleep(Duration::from_millis(
                jitter(self.config.base_pause_time_ms, None) as u64,
            ));
        }

        Ok(())
    }

    /// Helper function to process the message.
    fn process_message(&mut self, mut o_msg: Message) -> anyhow::Result<()> {
        match o_msg.body.payload {
            Topology { ref mut topology } => {
                let neighbors = topology
                    .remove(&self.node_id)
                    .expect("Maelstrom topology should always be valid");

                self.logger.log_debug(&format!(
                    "Acknowledged broadcast neighbors: {neighbors:?} from Topology message."
                ));
                self.broadcast_core
                    .insert(BroadcastCore::new(neighbors, self.is_singleton()));

                // Send out the ack message on Topology.
                self.out_sender
                    .send(Message::into_response(o_msg, TopologyOk {}, None))?;
            }
            Broadcast { message } => {
                let next_msg_id = self.vend_msg_id();
                if let Some(ref mut broadcast_core) = self.broadcast_core {
                    broadcast_core.store_message(message, next_msg_id, &o_msg.src);

                    // Send out the ack message on Broadcast.
                    self.out_sender
                        .send(Message::into_response(o_msg, BroadcastOk {}, None))?;
                } else {
                    bail!("Got broadcast without initialization");
                }
            }
            BroadcastOk {} => {
                if let Some(ref mut broadcast_core) = self.broadcast_core {
                    let msg_id = o_msg
                        .body
                        .in_reply_to
                        .expect("in_reply_to should be present on broadcast_ok");
                    let recipient = &o_msg.src;
                    self.logger.log_debug(&format!(
                        "Received broadcast_ok from {recipient} for msg_id: {msg_id}."
                    ));
                    broadcast_core.receive_ok(msg_id, recipient);
                } else {
                    bail!("Got broadcast_ok without initialization");
                }
            }
            Add { .. } | Send { .. } | CommitOffsets { .. } | Txn { .. } => {
                // Enqueue the request.
                let msg_id = self.vend_msg_id();
                let (command, response_payload) = match o_msg.body.payload {
                    Payload::Add { delta } => (RaftCommand::UpdateCounter { delta }, None),
                    Payload::Send { key, msg } => {
                        (RaftCommand::AppendKafkaRecord { key, msg }, None)
                    }
                    Payload::CommitOffsets { offsets } => {
                        (RaftCommand::CommitOffsets { offsets }, None)
                    }
                    Payload::Txn { txn } => {
                        // Return data immediately to the client to maintain availability.
                        // Writes are only eventually consistent.
                        let txn_ro_data = self.raft_core.get_txn_ro_data(&txn);
                        (RaftCommand::Txn { txn }, Some(TxnOk { txn: txn_ro_data }))
                    }
                    _ => {
                        // Only Raft based update requests are allowed.
                        unreachable!()
                    }
                };

                let msg_id = o_msg
                    .body
                    .msg_id
                    .expect("Requests should always have msg_id");
                let cur_time_ms = get_cur_time_ms();
                self.pending_requests.insert(
                    msg_id,
                    RaftRequest::new(
                        o_msg.src.clone(),
                        msg_id,
                        command,
                        cur_time_ms,
                        cur_time_ms + self.config.raft_request_ttl_ms as u128,
                    ),
                );

                // Respond immediately for certain requests if update doesn't have to be committed right away.
                if let Some(response_payload) = response_payload {
                    self.out_sender.send(Message::new(
                        o_msg.dst,
                        o_msg.src,
                        None,
                        Some(msg_id),
                        response_payload,
                    ))?;
                }
            }
            Read {} => {
                if let Some(ref mut broadcast_core) = self.broadcast_core {
                    // Return broadcast messages.
                    self.out_sender.send(Message::into_response(
                        o_msg,
                        broadcast_core.generate_read_payload(),
                        None,
                    ))?;
                } else {
                    // Return PN counter value.
                    self.out_sender.send(Message::into_response(
                        o_msg,
                        Payload::ReadOk {
                            messages: None,
                            value: Some(self.raft_core.get_pn_counter_value()),
                        },
                        None,
                    ))?;
                }
            }
            Poll { ref offsets } => {
                if self.raft_core.get_leader_id() == node_id_to_raft_id(&o_msg.dst) {
                    if (self.raft_core.is_leader()) {
                        let msgs = self.raft_core.get_kafka_records(offsets);
                        self.out_sender.send(Message::into_response(
                            o_msg,
                            Payload::PollOk { msgs },
                            None,
                        ))?;
                    } else {
                        // Force client to retry.
                        self.logger
                            .log_debug("Dropping Kafka poll due to unresolved election.");
                    }
                } else {
                    // Kafka polls are always forwarded to prevent clients from committing without getting the latest data.
                    self.forward_to_leader(o_msg)?;
                }
            }
            ListCommittedOffsets { ref keys } => {
                let offsets = self.raft_core.get_committed_offsets(keys);
                self.out_sender.send(Message::into_response(
                    o_msg,
                    Payload::ListCommittedOffsetsOk { offsets },
                    None,
                ))?;
            }
            // Raft specific internal handling follows.
            AppendEntries {
                term,
                leader_id,
                prev_log_index,
                prev_log_term,
                entries,
                leader_commit,
            } => {
                let result = self.raft_core.accept_entries(
                    term,
                    leader_id,
                    prev_log_index,
                    prev_log_term,
                    entries,
                    leader_commit,
                );
                self.reply_to_raft_node(result, o_msg.dst, o_msg.src)?
            }
            AppendEntriesResult {
                term,
                leader_id,
                success,
                last_log_index,
            } => {
                self.raft_core.process_append_result(
                    term,
                    leader_id,
                    success,
                    node_id_to_raft_id(&o_msg.src),
                    last_log_index,
                );
            }
            RequestVote {
                term,
                candidate_id,
                last_log_index,
                last_log_term,
            } => {
                let result = self.raft_core.accept_vote_request(
                    term,
                    candidate_id,
                    last_log_index,
                    last_log_term,
                );
                self.reply_to_raft_node(result, o_msg.dst, o_msg.src)?
            }
            RequestVoteResult {
                term,
                leader_id,
                vote_granted,
            } => {
                self.raft_core.process_vote_result(
                    term,
                    leader_id,
                    vote_granted,
                    node_id_to_raft_id(&o_msg.src),
                );
            }
            RaftRequestForward {
                client_id,
                client_msg_id,
                command,
            } => {
                if self
                    .raft_core
                    .accept_new_log(command, client_id, Some(client_msg_id))
                {
                    // Send an ack message.
                    self.out_sender.send(Message::new(
                        o_msg.dst,
                        o_msg.src,
                        None,
                        o_msg.body.msg_id,
                        RaftRequestAck {},
                    ))?;
                }
            }
            RaftRequestAck {} => {
                self.pending_requests.remove(
                    &o_msg
                        .body
                        .in_reply_to
                        .expect("RaftRequestAck must have in_reply_to"),
                );
            }
            _ => {
                // Unexpected types should've never reached here from the input handler.
                self.logger.log_debug(&format!("Server node encountered unexpected message passed from input handler: {o_msg:?}. Ignored."));
            }
        }

        Ok(())
    }

    /// Forwards a message (client request) to the Raft leader.
    fn forward_to_leader(&self, mut message: Message) -> anyhow::Result<()> {
        let leader_node_id = self.raft_core.get_leader_node_id();
        self.logger.log_debug(&format!(
            "Forwarding message with ID: {} to leader: {}.",
            message
                .body
                .msg_id
                .expect("Client request should have msg_id"),
            &leader_node_id
        ));
        message.dst = leader_node_id;
        self.out_sender.send(message)?;

        Ok(())
    }

    /// Replies to the Raft node with the given result.
    /// The result can be a response to either `AppendEntries` or `RequestVote`.
    fn reply_to_raft_node(
        &self,
        payload: Payload,
        node_id: String,
        dst: String,
    ) -> anyhow::Result<()> {
        self.logger
            .log_debug(&format!("Replying to Raft node: {dst} with: {payload:?}"));
        self.out_sender
            .send(Message::new(node_id, dst, None, None, payload))?;

        Ok(())
    }

    /// Handles a potential client request that needs to be replicated via the Raft state machine.
    /// Leader will consume the supplied command, whereas non-leader will forward the message.
    /// During election, the current "incumbent" (no longer the official leader) will response with temporarily-available.
    fn handle_raft_command(&mut self, msg: Message) -> anyhow::Result<()> {
        if self.raft_core.get_leader_id() == node_id_to_raft_id(&msg.dst) {
            let command = match msg.body.payload {
                Add { delta } => RaftCommand::UpdateCounter { delta },
                Send { key, msg } => RaftCommand::AppendKafkaRecord { key, msg },
                CommitOffsets { offsets } => RaftCommand::CommitOffsets { offsets },
                Txn { txn } => RaftCommand::Txn { txn },
                _ => {
                    // Only Raft based update requests are allowed.
                    unreachable!()
                }
            };

            if !self
                .raft_core
                .accept_new_log(command, msg.src.clone(), msg.body.msg_id)
            {
                let error_payload = Payload::Error {
                    code: 11, // `temporarily-unavailable` in Maelstrom.
                    text: "No leader available to serve at the moment due to pending election"
                        .to_string(),
                };
                self.out_sender.send(Message::new(
                    msg.dst,
                    msg.src,
                    None,
                    msg.body.msg_id,
                    error_payload,
                ))?;
            }
        } else {
            self.forward_to_leader(msg)?;
        }

        Ok(())
    }
}
