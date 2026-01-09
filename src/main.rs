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

    // 打开共享内存
    let (shm, first_init) = open_shm("/tmp/ipc_cond_demo.dat");

    // 构造 Ipc
    let ipc = Ipc::new(shm, first_init);

    match args.mode {
        Mode::Producer => {
            for i in 0..10 {
                let msg = format!("hello-{}", i);
                ipc.write(msg.as_bytes());
                ipc.write_done();
                println!("[P] {}", msg);
            }
            println!("[P] Producer finished");
        }
        Mode::Consumer => {
            loop {
                let read = ipc.read();
                println!("[C] Received: {}", std::str::from_utf8(read.data).unwrap());
                ipc.read_done();
            }
        }
    }
}
