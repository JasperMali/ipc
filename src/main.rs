mod layout;
mod shm;
mod producer;
mod consumer;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("producer") => producer::run_producer(),
        Some("consumer") => {
            let id = args.get(2).unwrap().parse::<usize>().unwrap();
            consumer::run_consumer(id)
        }
        _ => {
            println!("usage:");
            println!("  producer");
            println!("  consumer <id>");
        }
    }
}
