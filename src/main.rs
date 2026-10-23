extern crate chrono;
extern crate postgres;

mod cli;
mod dsn;
mod fibonacci;
mod generic;
mod pg_sampler;
mod threader;

use crate::fibonacci::Fibonacci;
use crate::threader::workload::Workload;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {}", err);
        ::std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = cli::Params::get_args();

    println!("Initializing");
    // Validate before connecting
    let max_wait: chrono::Duration = args.as_max_wait()?;
    let (min_threads, max_threads) = args.range_min_max();
    let w: Workload = args.as_workload();
    println!("{}", w.as_string());
    let mut threader = threader::Threader::new(max_threads as usize, w);
    let mut sampler = pg_sampler::PgSampler::new(args.as_dsn())?;
    sampler.next()?;
    let mut instable: bool = false;

    println!("min threads: {} max threads: {}", min_threads, max_threads);
    println!(
        "max_wait: {}s, min_samples: {}, spread: {}",
        max_wait.num_seconds(),
        args.min_samples,
        args.spread
    );

    println!("|---------------------|---------|-----------------------------------------|-----------------------|");
    println!("| Date       time     | Clients |                 Performance             |       Postgres        |");
    println!("|                     |         |---------------|-----------|-------------|-----------|-----------|");
    println!("|                     |         |      TPS      |  Latency  | TPS/Latency |   TPS     |    wal    |");
    println!("|                     |         |               |   (usec)  |             |           |    kB/s   |");
    println!("|---------------------|---------|---------------|-----------|-------------|-----------|-----------|");

    for num_threads in Fibonacci::new(1_u32, 1_u32).take_while(|v| *v < max_threads) {
        if num_threads < min_threads {
            continue;
        }
        threader.scaleup(num_threads);
        match threader.wait_stable(args.spread, args.min_samples as usize, max_wait) {
            Some(result) => {
                sampler.next()?;
                let latency = result.latency.num_microseconds().unwrap() as f64;
                let pg_tps: f64 = sampler.tps() as f64;
                if !result.stable {
                    instable = true;
                }
                println!(
                    "| {0} | {1:7.5} | {2} {3:>11.3} | {4:>9.1} | {5:>11.3} | {6:>9.3} | {7:>9.3} |",
                    chrono::offset::Local::now().format("%Y-%m-%d %H:%M:%S"),
                    num_threads,
                    match result.stable {
                        true => " ",
                        _ => "*",
                    },
                    result.tps,
                    latency,
                    result.tps / latency,
                    pg_tps,
                    sampler.wal_per_sec() as i32,
                    );
            }
            None => {
                println!(
                    "| {0} | {1:7.5} |   {2:>11.3} | {3:>9.1} | {4:>11.3} | {5:>9.3} | {6:>9.3} |",
                    chrono::offset::Local::now().format("%Y-%m-%d %H:%M:%S"),
                    num_threads,
                    "?",
                    "?",
                    "?",
                    "?",
                    "?"
                );
                break;
            }
        }
    }
    println!("|---------------------|---------|---------------|-----------|-------------|-----------|-----------|");

    if instable {
        println!("* Samples marked with '*' did not stabilize before max-wait.")
    }
    println!("Stopping, but lets give the threads some time to stop");
    threader.finish();

    println!("Finished");
    ::std::process::exit(0);
}
