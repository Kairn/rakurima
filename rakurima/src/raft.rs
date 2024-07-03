use std::{cmp::max, collections::HashSet, sync::mpsc::Sender};

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
    base_replicate_interval_ms: usize,
}

impl RaftConfig {
    pub fn new(base_election_timeout_ms: usize, base_replicate_interval_ms: usize) -> Self {
        Self {
            base_election_timeout_ms,
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
    cur_leader_id: usize,
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
            cur_leader_id: 0,
            role,
            voted_for: None,
            request_vote_task: None,
            logs: Vec::new(),
            next_election_time,
            commit_index: 0,
            last_applied: 0,
            next_indices: Vec::with_capacity(cluster_size),
            match_indices: Vec::with_capacity(cluster_size),
            next_replicate_time: 0,
            pn_counter_value: 0,
        }
    }

    pub fn is_leader(&self) -> bool {
        matches!(self.role, RaftRole::Leader)
    }

    pub fn get_leader_node_id(&self) -> String {
        raft_id_to_node_id(self.cur_leader_id)
    }

    /// Accepts a client update command as the leader.
    pub fn accept_new_log(&mut self, command: RaftCommand, src: String, msg_id: Option<usize>) {
        todo!()
    }

    /// Processes the `AppendEntries` RPC from a claimed leader.
    pub fn accept_entries(
        &mut self,
        term: usize,
        leader_id: usize,
        prev_log_index: usize,
        prev_log_term: usize,
        mut entries: Option<Vec<RaftLog>>,
        leader_commit: usize,
    ) -> Payload {
        if term < self.cur_term {
            // Reject the leader.
            return Payload::AppendEntriesResult {
                term: self.cur_term,
                leader_id: self.cur_leader_id,
                success: false,
            };
        }

        self.maybe_convert_to_follower(term, leader_id);

        // Reset election timeout.
        self.next_election_time =
            get_cur_time_ms() + jitter(self.config.base_election_timeout_ms) as u128;

        if self.log_matches(prev_log_index, prev_log_term) {
            if let Some(ref mut entries) = entries {
                // Append entries starting from the current index.
                self.logs.truncate(prev_log_index);
                self.logs.append(entries);
            }
            // Update commit.
            self.commit_index = max(self.commit_index, leader_commit);

            self.logger.log_debug(&format!("Replication validation succeeded at previous index: {prev_log_index} and previous log term: {prev_log_term}."));

            Payload::AppendEntriesResult {
                term: self.cur_term,
                leader_id: self.cur_leader_id,
                success: true,
            }
        } else {
            self.logger.log_debug(&format!("Replication validation failed at previous index: {prev_log_index} and previous log term: {prev_log_term}."));

            // Delete invalid entries at and after the previous index.
            self.logs.truncate(max(prev_log_index - 1, 0));

            Payload::AppendEntriesResult {
                term: self.cur_term,
                leader_id: self.cur_leader_id,
                success: false,
            }
        }
    }

    /// Checks and converts the current node into a follower if the current term is less than received.
    fn maybe_convert_to_follower(&mut self, term: usize, leader_id: usize) {
        if term > self.cur_term {
            self.logger.log_debug(&format!(
                "Converting to a follower in favor of leader: {leader_id} and term: {term}.",
            ));
            self.cur_term = term;
            self.role = RaftRole::Follower;
            self.cur_leader_id = leader_id;
            self.voted_for = None;
            self.request_vote_task = None;
        }
    }

    /// Checks if the log entries match the previous index and term given by the leader for replication.
    fn log_matches(&self, prev_log_index: usize, prev_log_term: usize) -> bool {
        if prev_log_index == 0 {
            // 0 is the starting point, always match.
            return true;
        }

        if let Some(log) = self.logs.get(prev_log_index - 1) {
            log.term == prev_log_term
        } else {
            false
        }
    }

    /// Processes the result from an `AppendEntries` request sent to another node.
    pub fn process_append_result(&mut self, term: usize, leader_id: usize, success: bool) {
        todo!()
    }

    /// Processes the `RequestVote` RPC from a potential candidate.
    pub fn accept_vote_request(
        &mut self,
        term: usize,
        candidate_id: usize,
        last_log_index: usize,
        last_log_term: usize,
    ) -> Payload {
        todo!()
    }

    /// Processes the result from an `RequestVote` request sent to another node.
    pub fn process_vote_result(&mut self, term: usize, leader_id: usize, vote_granted: bool) {
        todo!()
    }

    /// Analyzes the state of the Raft core and performs a series of actions if necessary.
    /// Leader will check if it needs to send log replication.
    /// Leader will check if it needs to commit any entries which have been replicated in the majority of nodes.
    /// All nodes will apply committed log entries.
    /// Followers and candidates will check if election timeout has occurred.
    /// Candidate will check if it has the votes to become the new leader.
    pub fn run_cycle(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
