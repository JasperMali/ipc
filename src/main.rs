use clap::{Parser, Subcommand};

mod ipc;
mod shm;

use ipc::*;
use shm::*;

#[derive(Subcommand)]
enum Mode {
    Producer,
    Consumer,
}

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    mode: Mode,
}

fn main() {
    let args = Args::parse();

    let mmap = open_shm("/tmp/ipc_kqueue_demo.dat").expect("mmap failed");
    let shm = unsafe { &*(mmap.as_ptr() as *const ShmIpc) };
    let ipc = Ipc::new(shm);

    match args.mode {
        Mode::Producer => {
            for i in 0..10 {
                let msg = format!("hello-{}", i);
                ipc.write(msg.as_bytes());
                println!("[P] {}", msg);
            }
        }
        Mode::Consumer => loop {
            let data = ipc.read_blocking();
            println!("recv: {}", std::str::from_utf8(data.data).unwrap());        },
    }
}
