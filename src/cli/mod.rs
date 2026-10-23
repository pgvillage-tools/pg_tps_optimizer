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
    pub fn as_max_wait(&self) -> chrono::Duration {
        match DurationString::from_string(self.max_wait.clone()) {
            Ok(ds) => match chrono::Duration::from_std(ds.into()) {
                Ok(duration) => duration,
                Err(_) => panic!(
                    "invalid value for max_wait: {} is not a Duration",
                    self.max_wait
                ),
            },
            Err(_) => panic!(
                "invalid value for max_wait: {} is not a Duration",
                self.max_wait
            ),
        }
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
