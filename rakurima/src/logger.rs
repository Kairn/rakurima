#[derive(Debug)]
pub struct ServerLogger;

impl ServerLogger {
    // Logs a message to STDERR.
    pub fn log_debug(&self, message: &str) {
        eprintln!("[RAKURIMA DEBUG] - {message}");
    }
}

#[derive(Debug)]
pub struct RaftLogger;

impl RaftLogger {
    // Logs a message to STDERR in the Raft context.
    pub fn log_debug(&self, message: &str) {
        eprintln!("[RAFT DEBUG] - {message}");
    }
}
