use memmap2::{MmapMut, MmapOptions};
use std::fs::OpenOptions;
use std::io::Result;
use std::mem::size_of;

use crate::ipc::ShmIpc;

// pub fn open_shm(path: &str) -> std::io::Result<MmapMut> {
//     let file = OpenOptions::new()
//         .read(true)
//         .write(true)
//         .create(true)
//         .open(path)?;
//
//     file.set_len(std::mem::size_of::<ShmIpc>() as u64)?;
//
//     unsafe { MmapOptions::new().map_mut(&file) }
// }