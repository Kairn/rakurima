use std::{
    env,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::logger::Logger;

/// Reads and parses out a numeric value from an environment variable key.
/// The given default will be returned if the key is missing or the value is invalid.
pub fn get_numeric_environment_variable(
    logger: &'static Logger,
    key: &str,
    default_val: usize,
) -> usize {
    match env::var(key) {
        Ok(val) => match val.parse() {
            Ok(v) => {
                logger.log_debug(&format!("{key}: set to value: {v}."));
                v
            }
            Err(_) => {
                logger.log_debug(&format!(
                    "{key}: parsing failed for value: {val}. Using default: {default_val}."
                ));
                default_val
            }
        },
        Err(_) => {
            logger.log_debug(&format!("{key}: not found. Using default: {default_val}."));
            default_val
        }
    }
}

/// Applies a random (between 0 and 10) fraction (1/10) of the base value on top of the base value.
/// The final value will be between the base value and 2 times the base value (both inclusive).
/// Used to randomize sleep/pause/retry timeouts.
pub fn jitter(base: usize) -> usize {
    let fraction = rand::random::<usize>() % 11;
    base + base * fraction / 10
}

/// Returns the current system time in milliseconds.
pub fn get_cur_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System clock should be available")
        .as_millis()
}
