/// Mode of operation for the server node.
/// In `Cluster` mode, an embedded list of peers is included.
#[derive(Debug)]
pub enum NodeMode {
    Singleton,
    Cluster(Vec<String>),
}

/// Static configurations for the server node.
#[derive(Debug)]
pub struct NodeConfig {
    base_pause_time_ms: usize,
}

/// The base class of the server node.
#[derive(Debug)]
pub struct Node {
    mode: NodeMode,
    config: NodeConfig,
}

impl Node {
    /// Starts the server's routine.
    pub fn orchestrate(&mut self) {
        todo!()
    }
}
