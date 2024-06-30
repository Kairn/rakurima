use std::{collections::HashSet, sync::mpsc::Sender};

use serde::{Deserialize, Serialize};

use crate::{
    logger::RaftLogger,
    message::{Message, Payload},
    util::{get_cur_time_ms, jitter, node_id_to_raft_id, raft_id_to_node_id},
};

// Node `n0` will be the default leader on startup without an explicit election.
const DEF_LEADER_ID: &str = "n0";

#[derive(Debug, Clone)]
pub struct RequestVoteTask {
    msg_id: String,
    granted_by: HashSet<String>,
}

#[derive(Debug)]
pub struct RaftConfig {
    base_election_timeout_ms: usize,
    base_heartbeat_interval_ms: usize,
    base_replicate_interval_ms: usize,
}

impl RaftConfig {
    pub fn new(
        base_election_timeout_ms: usize,
        base_heartbeat_interval_ms: usize,
        base_replicate_interval_ms: usize,
    ) -> Self {
        Self {
            base_election_timeout_ms,
            base_heartbeat_interval_ms,
            base_replicate_interval_ms,
        }
    }
}

#[derive(Debug)]
pub enum RaftRole {
    Leader,
    Candidate,
    Follower,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd_type")]
#[serde(rename_all = "snake_case")]
pub enum RaftCommand {
    UpdateCounter { delta: i32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftLog {
    term: usize,
    index: usize, // First index is 1.
    command: RaftCommand,
    // For client response.
    src: String,
    msg_id: usize,
}

#[derive(Debug)]
pub struct RaftCore {
    // Config and metadata.
    raft_id: usize,
    config: RaftConfig,
    logger: RaftLogger,
    out_sender: Sender<Message>,
    is_singleton: bool,

    // Persistent states.
    cur_term: usize,
    cur_leader: usize,
    role: RaftRole,
    voted_for: Option<usize>,
    request_vote_task: Option<RequestVoteTask>,
    logs: Vec<RaftLog>, // Note: this is 1-based indexing.

    // Volatile states.
    next_election_time: u128,
    commit_index: usize,
    last_applied: usize,

    // Leader states.
    next_indices: Vec<usize>,
    match_indices: Vec<usize>,
    next_heartbeat_time: u128,
    next_replicate_time: u128,

    // Data that logs are applied to.
    pn_counter_value: i32,
}

impl RaftCore {
    pub fn new(
        config: RaftConfig,
        node_id: &str,
        cluster_size: usize,
        out_sender: Sender<Message>,
    ) -> Self {
        let role = if node_id == DEF_LEADER_ID {
            RaftRole::Leader
        } else {
            RaftRole::Follower
        };
        let next_election_time =
            get_cur_time_ms() + jitter(config.base_election_timeout_ms) as u128;

        Self {
            raft_id: node_id_to_raft_id(node_id),
            config,
            logger: RaftLogger {},
            out_sender,
            is_singleton: cluster_size <= 1,
            cur_term: 0,
            cur_leader: 0,
            role,
            voted_for: None,
            request_vote_task: None,
            logs: Vec::new(),
            next_election_time,
            commit_index: 0,
            last_applied: 0,
            next_indices: Vec::with_capacity(cluster_size),
            match_indices: Vec::with_capacity(cluster_size),
            next_heartbeat_time: 0,
            next_replicate_time: 0,
            pn_counter_value: 0,
        }
    }

    pub fn is_leader(&self) -> bool {
        matches!(self.role, RaftRole::Leader)
    }

    pub fn get_leader_node_id(&self) -> String {
        raft_id_to_node_id(self.cur_leader)
    }

    /// Accepts a client update command as the leader.
    pub fn accept_new_log(&mut self, command: RaftCommand, src: String, msg_id: Option<usize>) {
        todo!()
    }

    pub fn accept_entries(
        &mut self,
        term: usize,
        leader_id: usize,
        prev_log_index: usize,
        prev_log_term: usize,
        entries: Option<Vec<RaftLog>>,
        leader_commit: usize,
    ) -> Payload {
        // TODO.
        let mut success = false;
        Payload::AppendEntriesResult {
            term: self.cur_leader,
            success,
        }
    }
}
