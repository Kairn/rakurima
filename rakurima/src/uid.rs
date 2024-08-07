use crate::util::{get_cur_time_ms, node_id_to_raft_id};

const TIMESTAMP_MASK: u128 = 0x7FFFFFFFFFFF; // 47 bits.
const ID_MASK: u128 = 0x7F; // 7 bits.
const SEQUENCE_MASK: u128 = 0x3FF; // 10 bits.

#[derive(Debug)]
pub struct UidCore {
    box_id: u128,
    sequence_number: u128,
}

impl UidCore {
    pub fn new(node_id: &str) -> Self {
        Self {
            box_id: node_id_to_raft_id(node_id) as u128, // Same parsing as Raft ID.
            sequence_number: 0,
        }
    }

    /// Generates a 64-bit Hex-encoded string using the "Snowflake" algorithm.
    /// This is NOT thread-safe.
    pub fn generate(&mut self) -> String {
        let cur_time_ms = get_cur_time_ms();
        // Take the bits from timestamp and move it to front.
        let mut id_raw = (cur_time_ms & TIMESTAMP_MASK) << 17;
        // Append the bits from the box ID.
        id_raw += (self.box_id & ID_MASK) << 10;
        // Append the bits from sequence number.
        id_raw += self.sequence_number;
        // Increment or reset sequence number.
        if self.sequence_number >= SEQUENCE_MASK {
            self.sequence_number = 0;
        } else {
            self.sequence_number += 1;
        }

        format!("{id_raw:x}")
    }
}
