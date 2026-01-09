use std::{fs::OpenOptions, mem, ptr};
use memmap2::MmapMut;
use crate::layout::*;

pub fn create_or_open() -> MmapMut {
    let path = "/tmp/ipc_demo.shm";
    let size = mem::size_of::<ShmLayout>();

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(path)
        .unwrap();

    file.set_len(size as u64).unwrap();

    let mut mmap = unsafe { MmapMut::map_mut(&file).unwrap() };

    let shm = unsafe { &mut *(mmap.as_mut_ptr() as *mut ShmLayout) };

    // 初始化一次
    shm.write_pos.store(0, std::sync::atomic::Ordering::Relaxed);
    shm.next_msg_id.store(0, std::sync::atomic::Ordering::Relaxed);
    for c in &shm.consumer_pos {
        c.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    mmap
}
