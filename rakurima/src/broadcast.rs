use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct BroadcastTask {
    msg_id: usize,
    content: usize,
    recipients: HashSet<String>,
    next_retry_time: u128,
}

#[derive(Debug)]
pub struct Broadcast {
    messages: HashSet<i32>,
    neighbors: Vec<String>,
    tasks: HashMap<usize, BroadcastTask>,
}
