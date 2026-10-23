use crate::dsn::Dsn;
use crate::generic;
use crate::threader::workload::Workload;
use duration_string::DurationString;
use structopt::StructOpt;

/// Search for a pattern in a file and display the lines that contain it.

#[derive(StructOpt)]
#[structopt(about = "I detect maximum TPS with minimal latency. Pass `-h` for more info.")]
pub struct Params {
    /// Connection string
    #[structopt(
        default_value,
        short,
        long,
        help = "the DSN to connect to (or use env vars PG...)"
    )]
    pub dsn: String,

    /// Query
    #[structopt(default_value, short, long, help = "the query to run")]
    pub query: String,

    /// Prepared queries
    #[structopt(
        short,
        long,
        help = "you can run prepared statements, or run direct statements"
    )]
    #[structopt(long)]
    pub prepared: bool,

    /// Transactional workload
    #[structopt(short, long, help = "you can run inside of a transaction or direct")]
    pub transactional: bool,

    /// Testrange
    #[structopt(
        default_value,
        short,
        long,
        help = "you can set min and max of numclients if you know (default 1:1000)"
    )]
    pub range: String,

    /// spread
    #[structopt(
        default_value,
        short,
        long,
        help = "you can set the spread that defines if the clients run stable."
    )]
    pub spread: f64,

    /// min_samples
    #[structopt(
        default_value,
        short = "m",
        long,
        help = "number of samples before we check the spread."
    )]
    pub min_samples: u32,

    /// max_wait
    #[structopt(
        default_value = "",
        short = "M",
        long,
        help = "Give it this ammount of seconds before we decide it wil never stabilize."
    )]
    pub max_wait: String,
}

impl Params {
    fn from_args() -> Params {
        <Params as StructOpt>::from_args()
    }
    pub fn get_args() -> Params {
        let mut args = Params::from_args();
        args.dsn = generic::get_env_str(&args.dsn, &String::from("PGTPSSOURCE"), "");
        args.query = generic::get_env_str(
            &args.query,
            &String::from("PGTPSQUERY"),
            &String::from("select * from pg_tables"),
        );
        args.prepared = generic::get_env_bool(args.prepared, &String::from("PGTPSPREPARED"));
        args.transactional =
            generic::get_env_bool(args.transactional, &String::from("PGTPSTRANSACTIONAL"));
        args.range = generic::get_env_str(
            &args.range,
            &String::from("PGTPSRANGE"),
            &String::from("1:1000"),
        );
        args.max_wait = generic::get_env_str(&args.max_wait, "PGTPSMAXWAIT", "10s");
        args.spread = generic::get_env_f64(args.spread, "PGTPSSPREAD", 10.0);
        args.min_samples = generic::get_env_u32(args.min_samples, "PGTPSMINSAMPLES", 10);
        args
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
