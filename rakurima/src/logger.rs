#[derive(Debug)]
pub struct ServerLogger;

impl ServerLogger {
    pub fn log_debug(&self, message: &str) {
        eprintln!("[RAKURIMA DEBUG] - {message}");
    }
}

#[derive(Debug)]
pub struct RaftLogger;

impl RaftLogger {
    pub fn log_debug(&self, message: &str) {
        eprintln!("[RAFT DEBUG] - {message}");
    }
}
