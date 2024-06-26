use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct BroadcastTask {
    msg_id: String,
    content: usize,
    recipients: HashSet<String>,
    next_retry_time: u128,
}

#[derive(Debug)]
pub struct Broadcast {
    messages: HashSet<i32>,
    neighbors: Vec<String>,
}
