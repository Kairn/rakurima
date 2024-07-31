use crate::message::Payload::{self, *};
use std::collections::{hash_map::ValuesMut, HashMap, HashSet};

use crate::{
    message::{Body, Message},
    util::get_cur_time_ms,
};

#[derive(Debug, Clone)]
pub struct BroadcastTask {
    msg_id: usize,
    content: i32,
    recipients: HashSet<String>,
    next_retry_time: u128,
}

impl BroadcastTask {
    pub fn new(msg_id: usize, content: i32, recipients: HashSet<String>) -> Self {
        Self {
            msg_id,
            content,
            recipients,
            next_retry_time: get_cur_time_ms(),
        }
    }

    pub fn get_content(&self) -> i32 {
        self.content
    }

    pub fn remove_recipient(&mut self, recipient: &str) {
        self.recipients.remove(recipient);
    }

    /// Returns the broadcast messages to retry if time is up.
    /// Otherwise None is returned.
    pub fn generate_retry_messages(
        &mut self,
        node_id: &str,
        next_timeout: usize,
    ) -> Option<Vec<Message>> {
        if get_cur_time_ms() >= self.next_retry_time {
            self.next_retry_time += next_timeout as u128;
            Some(
                self.recipients
                    .iter()
                    .map(|dst| {
                        Message::new(
                            node_id.to_string(),
                            dst.clone(),
                            Some(self.msg_id),
                            None,
                            Broadcast {
                                message: self.content,
                            },
                        )
                    })
                    .collect(),
            )
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct BroadcastCore {
    messages: HashSet<i32>,
    neighbors: Vec<String>,
    tasks: HashMap<usize, BroadcastTask>,
    is_singleton: bool,
}

impl BroadcastCore {
    pub fn new(neighbors: Vec<String>, is_singleton: bool) -> Self {
        Self {
            messages: HashSet::new(),
            neighbors,
            tasks: HashMap::new(),
            is_singleton,
        }
    }

    pub fn tasks(&mut self) -> ValuesMut<'_, usize, BroadcastTask> {
        self.tasks.values_mut()
    }

    /// Stores the broadcast message into the internal set.
    /// Creates a new task to send this message to its neighbors if the message is not already present.
    /// Returns whether this message is not seen before
    pub fn store_message(&mut self, message: i32, msg_id: usize, src: &str) -> bool {
        if self.messages.insert(message) {
            if !self.is_singleton {
                let mut recipients = HashSet::from_iter(self.neighbors.iter().cloned());
                recipients.remove(src);
                self.tasks
                    .insert(msg_id, BroadcastTask::new(msg_id, message, recipients));
            }
            true
        } else {
            false
        }
    }

    /// Processes the Ok message from other nodes for a broadcast.
    pub fn receive_ok(&mut self, msg_id: usize, recipient: &str) {
        if let Some(broadcast_task) = self.tasks.get_mut(&msg_id) {
            broadcast_task.remove_recipient(recipient);
        }
    }

    /// Deletes all tasks that have been acknowledged by their intended recipients.
    pub fn clean_tasks(&mut self) {
        self.tasks.retain(|_, v| !v.recipients.is_empty())
    }

    pub fn generate_read_payload(&self) -> Payload {
        Payload::ReadOk {
            messages: Some(Vec::from_iter(self.messages.iter().cloned())),
            value: None,
        }
    }
}
