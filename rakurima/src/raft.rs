use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct RequestVoteTask {
    msg_id: String,
    granted_by: HashSet<String>,
}

#[derive(Debug)]
pub struct Raft {
    // TODO.
}
