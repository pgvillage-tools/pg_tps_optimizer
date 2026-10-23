extern crate chrono;
extern crate postgres;

mod cli;
mod dsn;
mod fibonacci;
mod generic;
mod output;
mod pg_sampler;
mod threader;

use crate::fibonacci::Fibonacci;
use crate::threader::workload::Workload;

fn main() {
    // Rust ignores SIGPIPE, so writing to a closed stdout (e.g. `pg_tps_optimizer | head`)
    // panics. Restore the default, so the process stops quietly like other command line tools.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    if let Err(err) = run() {
        eprintln!("error: {}", err);
        ::std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = cli::Params::get_args();

    // Validate before connecting
    let max_wait: chrono::Duration = args.as_max_wait()?;
    let (min_threads, max_threads) = args.range_min_max();
    let mut report = output::Report::new(
        args.output,
        output::Settings {
            min_clients: min_threads,
            max_clients: max_threads,
            max_wait_seconds: max_wait.num_seconds(),
            min_samples: args.min_samples,
            spread: args.spread,
            transactional: args.transactional,
            prepared: args.prepared,
        },
    );
    report.info("Initializing");
    let w: Workload = args.as_workload();
    report.info(&w.as_string());
    let mut threader = threader::Threader::new(max_threads as usize, w);
    let mut sampler = pg_sampler::PgSampler::new(args.as_dsn())?;
    sampler.next()?;
    report.start();

    for num_threads in Fibonacci::new(1_u32, 1_u32).take_while(|v| *v < max_threads) {
        if num_threads < min_threads {
            continue;
        }
        threader.scaleup(num_threads);
        let timestamp = || chrono::offset::Local::now().to_rfc3339();
        match threader.wait_stable(args.spread, args.min_samples as usize, max_wait) {
            Some(result) => {
                sampler.next()?;
                let latency = result.latency.num_microseconds().unwrap() as f64;
                report.step(output::Step {
                    timestamp: timestamp(),
                    clients: num_threads,
                    stable: result.stable,
                    tps: Some(result.tps),
                    latency_us: Some(latency),
                    tps_per_latency: Some(result.tps / latency),
                    postgres_tps: Some(sampler.tps()),
                    wal_kb_per_sec: Some(sampler.wal_per_sec()),
                });
            }
            None => {
                report.step(output::Step {
                    timestamp: timestamp(),
                    clients: num_threads,
                    stable: false,
                    tps: None,
                    latency_us: None,
                    tps_per_latency: None,
                    postgres_tps: None,
                    wal_kb_per_sec: None,
                });
                break;
            }
        }
    }
    report.finish()?;

    report.info("Stopping, but lets give the threads some time to stop");
    threader.finish();

    report.info("Finished");
    ::std::process::exit(0);
}
