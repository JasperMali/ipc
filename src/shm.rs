use memmap2::MmapMut;
use std::fs::OpenOptions;
use std::io::Result;
use std::mem::size_of;

use crate::ipc::ShmIpc;

pub fn open_shm(path: &str) -> Result<MmapMut> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(path)?;

    file.set_len(size_of::<ShmIpc>() as u64)?;

    unsafe { MmapMut::map_mut(&file) }
}
