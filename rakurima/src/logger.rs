#[derive(Debug)]
pub struct Logger;

impl Logger {
    pub fn log_debug(&self, message: &str) {
        eprintln!("[RAKURIMA DEBUG] - {message}");
    }
}
