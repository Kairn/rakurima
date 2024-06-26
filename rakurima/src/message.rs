use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::raft::RaftLog;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub src: String,
    #[serde(rename = "dest")]
    pub dst: String,
    pub body: Body,
}

impl Message {
    /// Based on the incoming message, craft a response with the appropriate meta information.
    pub fn into_response(message: Self, payload: Payload, msg_id: Option<usize>) -> Self {
        Self {
            src: message.dst,
            dst: message.src,
            body: Body {
                msg_id,
                in_reply_to: message.body.msg_id,
                payload,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Body {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<usize>,
    #[serde(flatten)]
    pub payload: Payload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum Payload {
    Error {
        code: usize,
        text: String,
    },
    Init {
        node_id: String,
        node_ids: Vec<String>,
    },
    InitOk {},
    Echo {
        echo: String,
    },
    EchoOk {
        echo: String,
    },
    Topology {
        topology: HashMap<String, Vec<String>>,
    },
    TopologyOk {},
    Broadcast {
        message: i32,
    },
    BroadcastOk {},
    Read {},
    ReadOk {
        #[serde(skip_serializing_if = "Option::is_none")]
        messages: Option<Vec<i32>>, // For broadcast workload.
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<i32>, // For PN counter workload.
    },
    Add {
        delta: i32,
    },
    AddOk {},
    // Raft specific messages follows.
    AppendEntries {
        term: usize,
        leader_id: usize,
        prev_log_index: usize,
        prev_log_term: usize,
        entries: Option<Vec<RaftLog>>,
        leader_commit: usize,
    },
    AppendEntriesResult {
        term: usize,
        success: bool,
    },
    RequestVote {
        term: usize,
        candidate_id: usize,
        last_log_index: usize,
        last_log_term: usize,
    },
    RequestVoteResult {
        term: usize,
        vote_granted: bool,
    },
}
