use std::collections::{HashMap, HashSet};
use std::sync::mpsc::TryRecvError::{Disconnected, Empty};
use std::{
    sync::mpsc::{Receiver, Sender},
    thread,
    time::Duration,
};

use crate::broadcast;
use crate::message::{Body, Payload};
use crate::raft::RaftCommand;
use crate::raft::RaftCore;
use crate::util::{get_cur_time_ms, node_id_to_raft_id};
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
}

impl NodeConfig {
    pub fn new(base_pause_time_ms: usize, base_broadcast_retry_ms: usize) -> Self {
        Self {
            base_pause_time_ms,
            base_broadcast_retry_ms,
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
        let mut next_debug_log_time = get_cur_time_ms();

        loop {
            if get_cur_time_ms() >= next_debug_log_time {
                // Intent to emit a debug log every 3 seconds to indicate the server is still responsive.
                self.logger
                    .log_debug(&format!("Server node: {node_id} is still operational."));
                next_debug_log_time += 3000;
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
                                // No more messages at the moment, ignore and go back to sleep.
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
                        jitter(self.config.base_broadcast_retry_ms),
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

            // Do chores on the Raft node.
            self.raft_core.run_cycle()?;

            thread::sleep(Duration::from_millis(
                jitter(self.config.base_pause_time_ms) as u64,
            ));
        }

        Ok(())
    }

    /// Helper function to process the message.
    fn process_message(&mut self, mut msg: Message) -> anyhow::Result<()> {
        match msg.body.payload {
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
                    .send(Message::into_response(msg, TopologyOk {}, None))?;
            }
            Broadcast { message } => {
                let next_msg_id = self.vend_msg_id();
                if let Some(ref mut broadcast_core) = self.broadcast_core {
                    broadcast_core.store_message(message, next_msg_id, &msg.src);

                    // Send out the ack message on Broadcast.
                    self.out_sender
                        .send(Message::into_response(msg, BroadcastOk {}, None))?;
                } else {
                    bail!("Got broadcast without initialization");
                }
            }
            BroadcastOk {} => {
                if let Some(ref mut broadcast_core) = self.broadcast_core {
                    let msg_id = msg
                        .body
                        .in_reply_to
                        .expect("in_reply_to should be present on broadcast_ok");
                    let recipient = &msg.src;
                    self.logger.log_debug(&format!(
                        "Received broadcast_ok from {recipient} for msg_id: {msg_id}."
                    ));
                    broadcast_core.receive_ok(msg_id, recipient);
                } else {
                    bail!("Got broadcast_ok without initialization");
                }
            }
            Add { delta } => {
                if self.raft_core.is_leader() {
                    self.raft_core.accept_new_log(
                        RaftCommand::UpdateCounter { delta },
                        msg.src,
                        msg.body.msg_id,
                    );
                } else {
                    self.forward_to_leader(msg)?;
                }
            }
            Read {} => {
                if let Some(ref mut broadcast_core) = self.broadcast_core {
                    // Return broadcast messages.
                    self.out_sender.send(Message::into_response(
                        msg,
                        broadcast_core.generate_read_payload(),
                        None,
                    ))?;
                } else {
                    // TODO: Return PN counter value.
                    self.logger.log_debug("Unexpected read at this time.");
                }
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
                self.reply_to_raft_node(result, msg.dst, msg.src)?
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
                    node_id_to_raft_id(&msg.src),
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
                self.reply_to_raft_node(result, msg.dst, msg.src)?
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
                    node_id_to_raft_id(&msg.src),
                );
            }
            _ => {
                // Unexpected types should've never reached here from the input handler.
                self.logger.log_debug(&format!("Server node encountered unexpected message passed from input handler: {msg:?}. Ignored."));
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
        self.out_sender.send(Message {
            src: node_id,
            dst,
            body: Body {
                msg_id: None,
                in_reply_to: None,
                payload,
            },
        })?;
        Ok(())
    }
}
