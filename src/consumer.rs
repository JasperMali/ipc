use std::sync::atomic::Ordering;
use crate::layout::*;
use crate::shm::*;

pub fn run_consumer(id: usize) {
    let mut mmap = create_or_open();
    let shm = unsafe { &mut *(mmap.as_mut_ptr() as *mut ShmLayout) };

    loop {
        let read_pos  = shm.consumer_pos[id].load(Ordering::Acquire);
        let write_pos = shm.write_pos.load(Ordering::Acquire);

        if read_pos >= write_pos {
            // 如果你有 shutdown flag，这里可以退出
            std::thread::sleep(std::time::Duration::from_millis(10));
            continue;
        }

        let slot = &shm.ring[read_pos % RING_SIZE];

        println!(
            "[C{}] recv msg_id={} {}",
            id,
            slot.msg_id,
            String::from_utf8_lossy(&slot.data[..slot.len])
        );

        shm.consumer_pos[id].store(read_pos + 1, Ordering::Release);
    }
}
