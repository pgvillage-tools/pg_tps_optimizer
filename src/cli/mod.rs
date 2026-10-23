use crate::dsn::Dsn;
use crate::threader::workload::Workload;
use clap::Parser;
use duration_string::DurationString;

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

    /// the query to run
    #[arg(
        short,
        long,
        env = "PGTPSQUERY",
        default_value = "select * from pg_tables"
    )]
    pub query: String,

    /// you can run prepared statements, or run direct statements
    #[arg(short, long, env = "PGTPSPREPARED")]
    pub prepared: bool,

    /// you can run inside of a transaction or direct
    #[arg(short, long, env = "PGTPSTRANSACTIONAL")]
    pub transactional: bool,

    /// you can set min and max of numclients if you know
    #[arg(short, long, env = "PGTPSRANGE", default_value = "1:1000")]
    pub range: String,

    /// you can set the spread that defines if the clients run stable.
    #[arg(short, long, env = "PGTPSSPREAD", default_value_t = 10.0)]
    pub spread: f64,

    /// number of samples before we check the spread.
    #[arg(short = 'm', long, env = "PGTPSMINSAMPLES", default_value_t = 10)]
    pub min_samples: u32,

    /// Give it this ammount of seconds before we decide it wil never stabilize.
    #[arg(short = 'M', long, env = "PGTPSMAXWAIT", default_value = "10s")]
    pub max_wait: String,
}

impl Params {
    pub fn get_args() -> Params {
        Params::parse()
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
            0 => (1, 1000),
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
        }
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
