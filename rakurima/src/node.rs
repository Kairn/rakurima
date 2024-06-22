/// Mode of operation for the server node.
/// In `Cluster` mode, an embedded list of peers is included.
#[derive(Debug)]
pub enum NodeMode {
    Singleton,
    Cluster(Vec<String>),
}

impl NodeMode {
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
    pub base_pause_time_ms: usize,
}

/// The base class of the server node.
#[derive(Debug)]
pub struct Node {
    pub node_id: String,
    pub mode: NodeMode,
    pub config: NodeConfig,
}

impl Node {
    /// Starts the server's routine.
    pub fn orchestrate(&mut self) {
        todo!()
    }
}
