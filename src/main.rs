use std::thread;
use std::time::{Instant, Duration};
use clap::Parser;

mod ipc;
use ipc::Ipc;

/// MPMC Shared Memory IPC CLI
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Shared memory file path
    #[arg(short = 'f', long, default_value = "/tmp/shm_ipc_perf.dat")]
    ipc: String,

    /// Number of producers
    #[arg(short = 'p', long, default_value_t = 2)]
    producers: usize,

    /// Number of consumers
    #[arg(short = 'c', long, default_value_t = 2)]
    consumers: usize,

    /// Messages per producer
    #[arg(short = 'n', long, default_value_t = 5000)]
    msg_count: usize,
}

const END_MSG: &[u8] = b"__END__";

fn main() {
    let args = Args::parse();

    let start = Instant::now();

    // 消费者线程
    let mut cons_handles = vec![];
    for cid in 0..args.consumers {
        let mut ipc_clone = Ipc::from_existing(&args.ipc);
        cons_handles.push(thread::spawn(move || {
            let mut received = 0usize;
            loop {
                let msg = ipc_clone.read();
                if &msg == END_MSG {
                    println!("[Consumer {}] received {} msgs, exiting", cid, received);
                    break;
                }
                received += 1;
            }
            received
        }));
    }

    // 生产者线程
    let mut prod_handles = vec![];
    for pid in 0..args.producers {
        let mut ipc_clone = Ipc::new(&args.ipc);
        let msg_count = args.msg_count;
        let consumers = args.consumers;
        prod_handles.push(thread::spawn(move || {
            for i in 0..msg_count {
                let msg = format!("P{}-{}", pid, i);
                ipc_clone.write(msg.as_bytes());
            }
            // 每个生产者发送 END_MSG 给每个消费者
            for _ in 0..consumers {
                ipc_clone.write(END_MSG);
            }
        }));
    }

    // 等待生产者完成
    for h in prod_handles {
        h.join().unwrap();
    }

    // 等待消费者完成，并统计消息数量
    let mut total_consumed = 0usize;
    for h in cons_handles {
        total_consumed += h.join().unwrap();
    }

    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "All done. Total messages consumed: {}, elapsed: {:.3}s, throughput: {:.0} msg/sec",
        total_consumed,
        elapsed,
        total_consumed as f64 / elapsed
    );
}
