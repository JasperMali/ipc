use std::sync::atomic::Ordering;
use crate::layout::*;
use crate::shm::*;

pub fn run_producer() {
    let mut mmap = create_or_open();
    let shm = unsafe { &mut *(mmap.as_mut_ptr() as *mut ShmLayout) };

    for i in 0..100 {
        let msg = format!("hello-{}", i).into_bytes();
        let pos = shm.write_pos.fetch_add(1, Ordering::SeqCst) % RING_SIZE;
        let msg_id = shm.next_msg_id.fetch_add(1, Ordering::SeqCst);

        let slot = &mut shm.ring[pos];
        slot.msg_id = msg_id;
        slot.len = msg.len();
        slot.data[..msg.len()].copy_from_slice(&msg);

        println!("[P] send {} => slot {}", msg_id, pos);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}
