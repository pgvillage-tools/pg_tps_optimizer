// Reports the results of a run, as a table (printed while running) or as JSON (printed at the end)
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

pub mod graph;

#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
pub enum Format {
    Table,
    Json,
}

#[derive(Serialize)]
pub struct Settings {
    pub min_clients: u32,
    pub max_clients: u32,
    pub max_wait_seconds: i64,
    pub min_samples: u32,
    pub spread: f64,
    pub transactional: bool,
    pub prepared: bool,
}

// The results of one step (number of clients). The measurements are missing (null in JSON) when
// the step got no samples at all.
#[derive(Clone, Deserialize, Serialize)]
pub struct Step {
    pub timestamp: String,
    pub clients: u32,
    pub stable: bool,
    pub tps: Option<f64>,
    pub latency_us: Option<f64>,
    pub tps_per_latency: Option<f64>,
    pub postgres_tps: Option<f64>,
    pub wal_kb_per_sec: Option<f64>,
}

#[derive(Serialize)]
struct JsonReport<'a> {
    version: &'a str,
    settings: &'a Settings,
    steps: &'a [Step],
}

// The steps of a JSON report (from --output json), to draw a graph of
#[derive(Deserialize)]
pub struct JsonSteps {
    pub steps: Vec<Step>,
}

pub struct Report {
    format: Format,
    settings: Settings,
    steps: Vec<Step>,
}

impl Report {
    pub fn new(format: Format, settings: Settings) -> Report {
        Report {
            format,
            settings,
            steps: Vec::new(),
        }
    }

    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    // Progress messages go to stdout with the table, and to stderr with JSON, so stdout only has
    // the JSON document
    pub fn info(&self, message: &str) {
        match self.format {
            Format::Table => println!("{}", message),
            Format::Json => eprintln!("{}", message),
        }
    }

    pub fn start(&self) {
        self.info(&format!(
            "min threads: {} max threads: {}",
            self.settings.min_clients, self.settings.max_clients
        ));
        self.info(&format!(
            "max_wait: {}s, min_samples: {}, spread: {}",
            self.settings.max_wait_seconds, self.settings.min_samples, self.settings.spread
        ));
        if self.format == Format::Table {
            println!("|---------------------|---------|-----------------------------------------|-----------------------|");
            println!("| Date       time     | Clients |                 Performance             |       Postgres        |");
            println!("|                     |         |---------------|-----------|-------------|-----------|-----------|");
            println!("|                     |         |      TPS      |  Latency  | TPS/Latency |   TPS     |    wal    |");
            println!("|                     |         |               |   (usec)  |             |           |    kB/s   |");
            println!("|---------------------|---------|---------------|-----------|-------------|-----------|-----------|");
        }
    }

    pub fn step(&mut self, step: Step) {
        if self.format == Format::Table {
            println!("{}", table_row(&step));
        }
        self.steps.push(step);
    }

    pub fn finish(&self) -> Result<(), serde_json::Error> {
        match self.format {
            Format::Table => {
                println!("|---------------------|---------|---------------|-----------|-------------|-----------|-----------|");
                if self.steps.iter().any(|step| !step.stable) {
                    println!("* Samples marked with '*' did not stabilize before max-wait.")
                }
            }
            Format::Json => {
                let report = JsonReport {
                    version: env!("CARGO_PKG_VERSION"),
                    settings: &self.settings,
                    steps: &self.steps,
                };
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
        }
        Ok(())
    }
}

fn table_row(step: &Step) -> String {
    let timestamp = chrono::DateTime::parse_from_rfc3339(&step.timestamp)
        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|_| step.timestamp.clone());
    match (
        step.tps,
        step.latency_us,
        step.tps_per_latency,
        step.postgres_tps,
        step.wal_kb_per_sec,
    ) {
        (Some(tps), Some(latency), Some(tps_per_latency), Some(pg_tps), Some(wal)) => format!(
            "| {0} | {1:7.5} | {2} {3:>11.3} | {4:>9.1} | {5:>11.3} | {6:>9.3} | {7:>9.1} |",
            timestamp,
            step.clients,
            match step.stable {
                true => " ",
                false => "*",
            },
            tps,
            latency,
            tps_per_latency,
            pg_tps,
            wal,
        ),
        _ => format!(
            "| {0} | {1:7.5} |   {2:>11.3} | {3:>9.1} | {4:>11.3} | {5:>9.3} | {6:>9.3} |",
            timestamp, step.clients, "?", "?", "?", "?", "?"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(tps: Option<f64>) -> Step {
        Step {
            timestamp: "2026-10-24T10:00:00+02:00".to_string(),
            clients: 5,
            stable: false,
            tps,
            latency_us: tps.map(|_| 500.0),
            tps_per_latency: tps.map(|_| 20.0),
            postgres_tps: tps.map(|_| 9900.0),
            wal_kb_per_sec: tps.map(|_| 614.7),
        }
    }

    #[test]
    fn test_table_row() {
        assert_eq!(
            table_row(&step(Some(10000.0))),
            "| 2026-10-24 10:00:00 |       5 | *   10000.000 |     500.0 |      20.000 |  9900.000 |     614.7 |"
        );
        assert_eq!(
            table_row(&step(None)),
            "| 2026-10-24 10:00:00 |       5 |             ? |         ? |           ? |         ? |         ? |"
        );
    }

    #[test]
    fn test_json() {
        let json = serde_json::to_value(step(None)).unwrap();
        assert_eq!(json["clients"], 5);
        assert_eq!(json["stable"], false);
        assert!(json["tps"].is_null());
        let json = serde_json::to_value(step(Some(10000.0))).unwrap();
        assert_eq!(json["tps"], 10000.0);
        assert_eq!(json["wal_kb_per_sec"], 614.7);
    }
}
