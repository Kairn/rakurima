use std::{cmp::max, collections::HashSet, sync::mpsc::Sender};

use serde::{Deserialize, Serialize};

use crate::{
    logger::RaftLogger,
    message::{Body, Message, Payload},
    util::{get_cur_time_ms, jitter, node_id_to_raft_id, raft_id_to_node_id},
};

// Node `n0` will be the default leader on startup without an explicit election.
const DEF_LEADER_ID: &str = "n0";

#[derive(Debug, Clone)]
pub struct RequestVoteTask {
    granted_by: HashSet<usize>,
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
    cluster_size: usize,

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
            cluster_size,
            cur_term: 0,
            cur_leader_id: 0,
            role,
            voted_for: None,
            request_vote_task: None,
            logs: Vec::new(),
            next_election_time,
            commit_index: 0,
            last_applied: 0,
            next_indices: vec![1; cluster_size],
            match_indices: vec![0; cluster_size],
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
    /// Response won't be sent until this log is committed and applied.
    pub fn accept_new_log(&mut self, command: RaftCommand, src: String, msg_id: Option<usize>) {
        self.logs.push(RaftLog {
            term: self.cur_term,
            index: self.logs.len() + 1,
            command,
            src,
            msg_id: msg_id.expect("Requests should always have msg_id"),
        });
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
                last_log_index: 0, // Irrelevant if not success.
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
                last_log_index: if let Some(last_log) = self.logs.last() {
                    last_log.index
                } else {
                    0
                },
            }
        } else {
            self.logger.log_debug(&format!("Replication validation failed at previous index: {prev_log_index} and previous log term: {prev_log_term}."));

            // Delete invalid entries at and after the previous index.
            self.logs.truncate(max(prev_log_index - 1, 0));

            Payload::AppendEntriesResult {
                term: self.cur_term,
                leader_id: self.cur_leader_id,
                success: false,
                last_log_index: 0,
            }
        }
    }

    /// Checks and converts the current node into a follower if the current term is less than received.
    /// Also convert if current node is candidate when another node is already leader with the same term.
    /// Returns whether a conversion happened.
    fn maybe_convert_to_follower(&mut self, term: usize, leader_id: usize) -> bool {
        if term > self.cur_term
            || (term == self.cur_term && matches!(self.role, RaftRole::Candidate))
        {
            self.logger.log_debug(&format!(
                "Converting to a follower in favor of leader: {leader_id} and term: {term}.",
            ));
            self.cur_term = term;
            self.role = RaftRole::Follower;
            self.cur_leader_id = leader_id;
            self.voted_for = None;
            self.request_vote_task = None;
            self.next_election_time =
                get_cur_time_ms() + jitter(self.config.base_election_timeout_ms) as u128;
            return true;
        }
        false
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
    pub fn process_append_result(
        &mut self,
        term: usize,
        leader_id: usize,
        success: bool,
        follower_id: usize,
        last_log_index: usize,
    ) {
        if !self.maybe_convert_to_follower(term, leader_id) {
            if success {
                // Update the follower's next index and match index to the latest.
                *self
                    .next_indices
                    .get_mut(follower_id)
                    .expect("Follower ID should always exist") = last_log_index + 1;
                *self
                    .match_indices
                    .get_mut(follower_id)
                    .expect("Follower ID should always exist") = last_log_index;
            } else {
                // Decrement the follower's next index to retry later.
                // Index is guaranteed to be greater or equal to 0 at any point.
                *self
                    .next_indices
                    .get_mut(follower_id)
                    .expect("Follower ID should always exist") -= 1;
            }
        }
        // Do nothing if no longer the leader.
    }

    /// Processes the `RequestVote` RPC from a potential candidate.
    pub fn accept_vote_request(
        &mut self,
        term: usize,
        candidate_id: usize,
        last_log_index: usize,
        last_log_term: usize,
    ) -> Payload {
        if term < self.cur_term {
            return Payload::RequestVoteResult {
                term: self.cur_term,
                leader_id: self.cur_leader_id,
                vote_granted: false,
            };
        }

        self.maybe_convert_to_follower(term, self.cur_leader_id); // Keep leader ID the same during election.

        if let Some(voted_for_id) = self.voted_for {
            if voted_for_id == candidate_id {
                // Already granted vote, grant again since this may be a retry.
                return Payload::RequestVoteResult {
                    term,
                    leader_id: self.cur_leader_id,
                    vote_granted: true,
                };
            } else {
                // Voted for another candidate, decline request.
                return Payload::RequestVoteResult {
                    term,
                    leader_id: self.cur_leader_id,
                    vote_granted: false,
                };
            }
        }

        let (my_last_log_index, my_last_log_term) = self.get_log_index_and_term(None);

        // Check if the candidate's log is up to date.
        if last_log_term > my_last_log_term
            || (last_log_term == my_last_log_term && last_log_index >= my_last_log_index)
        {
            // Grant vote.
            self.logger.log_debug(&format!(
                "Granting vote to candidate: {candidate_id} for term: {term}."
            ));
            self.voted_for.insert(candidate_id);
            Payload::RequestVoteResult {
                term,
                leader_id: self.cur_leader_id,
                vote_granted: true,
            }
        } else {
            Payload::RequestVoteResult {
                term,
                leader_id: self.cur_leader_id,
                vote_granted: false,
            }
        }
    }

    /// Retrieves the index and term of a log entry at a particular index.
    /// The last log is checked if index is `None`.
    fn get_log_index_and_term(&self, index: Option<usize>) -> (usize, usize) {
        let log = match index {
            Some(i) => self.logs.get(i - 1), // 1-based indexing.
            None => self.logs.last(),
        };

        match log {
            Some(last_log) => (last_log.index, last_log.term),
            None => (0, self.cur_term),
        }
    }

    /// Processes the result from an `RequestVote` request sent to another node.
    pub fn process_vote_result(
        &mut self,
        term: usize,
        leader_id: usize,
        vote_granted: bool,
        voter_id: usize,
    ) {
        if !vote_granted || term != self.cur_term {
            return;
        }

        if let Some(ref mut task) = self.request_vote_task {
            task.granted_by.insert(voter_id);
            if task.granted_by.len() > self.cluster_size / 2 {
                // Won the election.
                self.logger
                    .log_debug(&format!("Election won for the term: {term}."));
                self.voted_for = None;
                self.request_vote_task = None;
                self.role = RaftRole::Leader;
                self.cur_leader_id = self.raft_id;
                self.next_replicate_time = get_cur_time_ms(); // Immediately schedule a round of replication to signal election result.
                self.next_indices = vec![self.logs.len() + 1; self.cluster_size];
                self.match_indices = vec![0; self.cluster_size];
            }
        }
    }

    /// Analyzes the state of the Raft core and performs a series of actions if necessary.
    /// Leader will check if it needs to send log replication.
    /// Leader will check if it needs to commit any entries which have been replicated in the majority of nodes.
    /// All nodes will apply committed log entries.
    /// Followers and candidates will check if election timeout has occurred.
    pub fn run_cycle(&mut self, next_msg_id: usize) -> anyhow::Result<()> {
        if self.cluster_size <= 1 {
            // Singleton mode, commit everything.
            self.commit_index = self.logs.len();
        } else if matches!(self.role, RaftRole::Leader) {
            todo!()
        } else {
            // Follower or candidate.
            // Check for election timeout.
            let cur_time_ms = get_cur_time_ms();
            if cur_time_ms >= self.next_election_time {
                // Start a new election.
                self.next_election_time =
                    cur_time_ms + jitter(self.config.base_election_timeout_ms) as u128;
                self.cur_term += 1;
                // Vote for self.
                self.voted_for = Some(self.raft_id);
                let mut granted_by = HashSet::with_capacity(self.cluster_size);
                granted_by.insert(self.raft_id);
                self.request_vote_task = Some(RequestVoteTask { granted_by });

                // Send vote requests to all peers in the cluster.
                let (my_last_log_index, my_last_log_term) = self.get_log_index_and_term(None);
                let message_body = Body {
                    msg_id: Some(next_msg_id),
                    in_reply_to: None,
                    payload: Payload::RequestVote {
                        term: self.cur_term,
                        candidate_id: self.raft_id,
                        last_log_index: my_last_log_index,
                        last_log_term: my_last_log_term,
                    },
                };
                for peer_id in 0..self.cluster_size {
                    if peer_id == self.raft_id {
                        continue;
                    }
                    let peer_id = raft_id_to_node_id(peer_id);
                    self.out_sender.send(Message {
                        src: raft_id_to_node_id(self.raft_id),
                        dst: peer_id,
                        body: message_body.clone(),
                    })?;
                }
            }
        }

        // Apply committed log entries.
        for log_index in (self.last_applied + 1)..(self.commit_index + 1) {
            self.apply_log(log_index);
        }
        self.last_applied = max(self.last_applied, self.commit_index);

        Ok(())
    }

    /// Applies a log's command to the state.
    /// Leader will also send an acknowledgment message back to the client.
    fn apply_log(&mut self, log_index: usize) -> anyhow::Result<()> {
        self.logger
            .log_debug(&format!("Applying log entry at index: {log_index}."));
        let log_to_apply: &RaftLog = self
            .logs
            .get(log_index - 1)
            .expect("Log should exist if committed.");

        todo!()
    }
}
