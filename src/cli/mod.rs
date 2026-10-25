use crate::dsn::Dsn;
use crate::output::graph::GraphFormat;
use crate::output::Format;
use crate::threader::workload::Workload;
use clap::{CommandFactory, FromArgMatches, Parser};
use duration_string::DurationString;
use std::path::PathBuf;

mod config;

// Every option can also be set with an environment variable. An option on the command line wins
// over the environment variable, which wins over the default.
#[derive(Parser)]
#[command(
    version,
    about = "I detect maximum TPS with minimal latency. Pass `-h` for more info."
)]
pub struct Params {
    /// the DSN to connect to (or use env vars PG...)
    #[arg(short, long, env = "PGTPSSOURCE", default_value = "")]
    pub dsn: String,

    /// the query every client runs; $1 is the number of the client. Without a query, every
    /// client updates its own row in table pg_tps_optimizer (removed after the run)
    #[arg(short, long, env = "PGTPSQUERY", default_value = "")]
    pub query: String,

    /// prepare the query once per client, and then only execute it
    #[arg(short, long, env = "PGTPSPREPARED")]
    pub prepared: bool,

    /// run every query in its own transaction (begin / commit)
    #[arg(short, long, env = "PGTPSTRANSACTIONAL")]
    pub transactional: bool,

    /// the number of clients to run with: min:max, or only max (both included). The default
    /// ends at 89 clients, below the default max_connections of 100
    #[arg(short, long, env = "PGTPSRANGE", default_value = "1:100")]
    pub range: String,

    /// the maximum spread (in %) of TPS and latency for a stable step
    #[arg(short, long, env = "PGTPSSPREAD", default_value_t = 10.0)]
    pub spread: f64,

    /// the number of samples (of 200ms) to check if a step is stable
    #[arg(short = 'm', long, env = "PGTPSMINSAMPLES", default_value_t = 10)]
    pub min_samples: u32,

    /// how long to wait for a step to become stable, like 10s, 1m30s or 20 (seconds)
    #[arg(short = 'M', long, env = "PGTPSMAXWAIT", default_value = "10s")]
    pub max_wait: String,

    /// output format: a table while running, or a JSON document at the end
    #[arg(short, long, env = "PGTPSOUTPUT", value_enum, default_value_t = Format::Table)]
    pub output: Format,

    /// write a graph of the TPS per number of clients to this file (PNG, or SVG for .svg)
    #[arg(short, long, env = "PGTPSGRAPH")]
    pub graph: Option<PathBuf>,

    /// the format of the graph; auto: SVG when the file name ends with .svg, otherwise PNG
    #[arg(long, env = "PGTPSGRAPHFORMAT", value_enum, default_value_t = GraphFormat::Auto)]
    pub graph_format: GraphFormat,

    /// instead of running, draw the graph of these JSON reports (from --output json), one line per
    /// report; repeat (or separate with commas) for more reports
    #[arg(
        long,
        env = "PGTPSGRAPHINPUT",
        value_delimiter = ',',
        requires = "graph"
    )]
    pub graph_input: Vec<PathBuf>,

    /// a YAML config file with a value per option, like `range: 1:200`; options on the command
    /// line and environment variables win over the config file
    #[arg(short, long, env = "PGTPSCONFIG")]
    pub config: Option<PathBuf>,
}

// The config file, from -c/--config or PGTPSCONFIG. Found before parsing the options, as its
// values are the defaults of the other options (also in --help).
fn config_path(args: impl Iterator<Item = String>) -> Option<PathBuf> {
    let mut args = args.peekable();
    let mut path = None;
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        } else if arg == "-c" || arg == "--config" {
            path = args.next();
        } else if let Some(value) = arg.strip_prefix("--config=") {
            path = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("-c").filter(|v| !v.is_empty()) {
            path = Some(value.trim_start_matches('=').to_string());
        }
    }
    path.or_else(|| std::env::var("PGTPSCONFIG").ok())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

// Like clap, for errors before running
fn exit_with_error(message: &str) -> ! {
    eprintln!("error: {}", message);
    std::process::exit(crate::exit::USAGE);
}

impl Params {
    pub fn get_args() -> Params {
        let mut command = Params::command();
        if let Some(path) = config_path(std::env::args().skip(1)) {
            let settings = config::read(&path).unwrap_or_else(|err| exit_with_error(&err));
            for (key, values) in settings {
                let known = command
                    .get_arguments()
                    .any(|arg| arg.get_id() == key.as_str() && key != "config");
                if !known {
                    exit_with_error(&format!(
                        "unknown option \"{}\" in config file \"{}\"",
                        key,
                        path.display()
                    ));
                }
                command = command.mut_arg(key, |arg| arg.default_values(values));
            }
        }
        let matches = command.get_matches();
        Params::from_arg_matches(&matches).unwrap_or_else(|err| err.exit())
    }
    pub fn as_dsn(&self) -> Dsn {
        Dsn::from_string(self.dsn.as_str())
    }
    pub fn as_workload(&self) -> Workload {
        Workload::new(
            self.as_dsn(),
            self.query.to_string(),
            self.transactional,
            self.prepared,
        )
    }
    // A duration like 10s, 1m30s or 500ms; a plain number means seconds
    pub fn as_max_wait(&self) -> Result<chrono::Duration, String> {
        let value = self.max_wait.trim();
        let invalid = || {
            format!(
                "invalid value for max-wait: \"{}\" is not a duration (e.g. 10, 10s, 1m30s or 500ms)",
                self.max_wait
            )
        };
        let duration: std::time::Duration = match value.parse::<u64>() {
            Ok(seconds) => std::time::Duration::from_secs(seconds),
            Err(_) => DurationString::from_string(value.to_string())
                .map_err(|_| invalid())?
                .into(),
        };
        chrono::Duration::from_std(duration).map_err(|_| invalid())
    }
    pub fn range_min_max(&self) -> (u32, u32) {
        let re = regex::Regex::new(r"\d+").unwrap();
        let values: Vec<_> = re
            .find_iter(self.range.as_str())
            .filter_map(|digits| digits.as_str().parse().ok())
            .collect();
        match values.len() {
            0 => (1, 100),
            1 => (1, values[0]),
            _ => (values[0], values[values.len() - 1]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params_with_max_wait(max_wait: &str) -> Params {
        Params {
            dsn: String::new(),
            query: String::new(),
            prepared: false,
            transactional: false,
            range: String::new(),
            spread: 0.0,
            min_samples: 0,
            max_wait: max_wait.to_string(),
            output: Format::Table,

            config: None,
            graph: None,
            graph_format: GraphFormat::Auto,
            graph_input: Vec::new(),
        }
    }

    #[test]
    fn test_config_path() {
        let path = |args: &[&str]| config_path(args.iter().map(|a| a.to_string()));
        assert_eq!(path(&["-c", "a.yaml"]), Some(PathBuf::from("a.yaml")));
        assert_eq!(path(&["--config", "a.yaml"]), Some(PathBuf::from("a.yaml")));
        assert_eq!(path(&["--config=a.yaml"]), Some(PathBuf::from("a.yaml")));
        assert_eq!(path(&["-ca.yaml"]), Some(PathBuf::from("a.yaml")));
        assert_eq!(
            path(&["-r", "1:5", "-c", "a.yaml", "-s", "5"]),
            Some(PathBuf::from("a.yaml"))
        );
        assert_eq!(
            path(&["--", "-c", "a.yaml"]),
            std::env::var("PGTPSCONFIG").ok().map(PathBuf::from)
        );
    }

    #[test]
    fn test_as_max_wait() {
        for (value, seconds) in [("3", 3), ("10s", 10), ("1m30s", 90), (" 5 ", 5)] {
            assert_eq!(
                params_with_max_wait(value).as_max_wait(),
                Ok(chrono::Duration::seconds(seconds)),
                "{}",
                value
            );
        }
        assert_eq!(
            params_with_max_wait("500ms").as_max_wait(),
            Ok(chrono::Duration::milliseconds(500))
        );
        for value in ["abc", "3x", "-1"] {
            let err = params_with_max_wait(value).as_max_wait().unwrap_err();
            assert!(err.contains("is not a duration"), "{}: {}", value, err);
        }
    }
}
