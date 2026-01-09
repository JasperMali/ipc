use std::sync::atomic::{AtomicU64, AtomicUsize};

pub const MAX_MSG_SIZE: usize = 1024 * 1024;
pub const RING_SIZE: usize = 8;
pub const MAX_CONSUMERS: usize = 4;

#[repr(C)]
pub struct MsgSlot {
    pub msg_id: u64,
    pub len: usize,
    pub data: [u8; MAX_MSG_SIZE],
}

#[repr(C)]
pub struct ShmLayout {
    pub write_pos: AtomicUsize,
    pub next_msg_id: AtomicU64,
    pub consumer_pos: [AtomicUsize; MAX_CONSUMERS],
    pub ring: [MsgSlot; RING_SIZE],
}
