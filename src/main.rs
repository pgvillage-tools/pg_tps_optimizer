extern crate chrono;
extern crate postgres;

mod cli;
mod dsn;
mod exit;
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
        ::std::process::exit(exit::code_of(err.as_ref()));
    }
}

// A line in the graph per JSON report (from --output json), named after the file
fn load_series(
    path: &std::path::Path,
) -> Result<output::graph::Series, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("could not read \"{}\": {}", path.display(), e))?;
    let report: output::JsonSteps = serde_json::from_str(&content)
        .map_err(|e| format!("\"{}\" is not a JSON report: {}", path.display(), e))?;
    Ok(output::graph::Series {
        name: path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_default(),
        steps: report.steps,
    })
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = cli::Params::get_args();

    // Only draw the graph of earlier runs, without running
    if let (Some(graph), false) = (&args.graph, args.graph_input.is_empty()) {
        let series = args
            .graph_input
            .iter()
            .map(|path| load_series(path))
            .collect::<Result<Vec<_>, _>>()?;
        output::graph::draw(graph, args.graph_format, &series)?;
        eprintln!("Wrote graph to {}", graph.display());
        return Ok(());
    }

    // Validate before connecting
    let max_wait: chrono::Duration = args
        .as_max_wait()
        .map_err(|e| exit::Failure::new(exit::USAGE, e))?;
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
    let default_workload = w.is_default();
    let mut sampler = pg_sampler::PgSampler::new(args.as_dsn())?;
    sampler.check_query(&w)?;
    let mut threader = threader::Threader::new(max_threads as usize, w);
    sampler.next()?;
    report.start();

    // The number of clients of a step without any finished transactions, which ends the run
    let mut failed_step: Option<u32> = None;
    for num_threads in Fibonacci::new(1_u32, 1_u32).take_while(|v| *v <= max_threads) {
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
                failed_step = Some(num_threads);
                break;
            }
        }
    }
    report.finish()?;
    if let Some(graph) = &args.graph {
        let series = output::graph::Series {
            name: "TPS".to_string(),
            steps: report.steps().to_vec(),
        };
        output::graph::draw(graph, args.graph_format, &[series])?;
        report.info(&format!("Wrote graph to {}", graph.display()));
    }

    report.info("Stopping, but lets give the threads some time to stop");
    threader.finish();
    if default_workload {
        if let Err(err) = sampler.drop_table() {
            eprintln!("warning: could not drop table pg_tps_optimizer: {}", err);
        }
    }

    if let Some(clients) = failed_step {
        return Err(Box::new(exit::Failure::new(
            exit::ERROR,
            format!(
                "no transaction finished with {} {} within max-wait: the queries fail \
                 (see the errors above) or hang",
                clients,
                if clients == 1 { "client" } else { "clients" }
            ),
        )));
    }

    report.info("Finished");
    ::std::process::exit(0);
}
