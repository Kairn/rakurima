use std::sync::mpsc::TryRecvError::{Disconnected, Empty};
use std::{
    sync::mpsc::{Receiver, Sender},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    broadcast::Broadcast,
    logger::Logger,
    message::{self, Message},
    util::jitter,
};

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
    logger: &'static Logger,

    // Communication channels.
    in_receiver: Receiver<Message>,
    out_sender: Sender<Message>,

    // Internal cores.
    broadcast: Option<Broadcast>,
}

impl Node {
    pub fn new(
        node_id: String,
        mode: NodeMode,
        config: NodeConfig,
        logger: &'static Logger,
        in_receiver: Receiver<Message>,
        out_sender: Sender<Message>,
    ) -> Self {
        Self {
            node_id,
            mode,
            config,
            logger,
            in_receiver,
            out_sender,
            broadcast: None,
        }
    }

    pub fn get_node_id(&self) -> &str {
        &self.node_id
    }

    /// Starts the server's routine.
    pub fn orchestrate(&mut self) -> anyhow::Result<()> {
        let node_id = self.node_id.as_str().to_string();
        let mut clock = SystemTime::now();
        let mut next_debug_log_time = clock.duration_since(UNIX_EPOCH)?.as_millis();

        loop {
            clock = SystemTime::now();
            let cur_time_ms = clock.duration_since(UNIX_EPOCH)?.as_millis();
            if cur_time_ms >= next_debug_log_time {
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

            thread::sleep(Duration::from_millis(
                jitter(self.config.base_pause_time_ms) as u64,
            ));
        }

        Ok(())
    }

    fn process_message(&mut self, message: Message) -> anyhow::Result<()> {
        Ok(())
    }
}
