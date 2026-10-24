// The exit codes of pg_tps_optimizer, documented in docs/users-guide/usage.md#exit-codes
use std::error::Error;
use std::fmt;

// An error while running, like a failing query
pub const ERROR: i32 = 1;
// Invalid options, environment variables, config file or connection settings (like clap uses 2)
pub const USAGE: i32 = 2;
// The server can't be reached, or TLS fails (like the certificate check)
pub const UNREACHABLE: i32 = 3;
// The server refuses the connection (authentication, pg_hba.conf, database doesn't exist, ...)
pub const REJECTED: i32 = 4;

// An error with the exit code it should end pg_tps_optimizer with
#[derive(Debug)]
pub struct Failure {
    pub code: i32,
    message: String,
}

impl Failure {
    pub fn new(code: i32, message: impl Into<String>) -> Failure {
        Failure {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for Failure {}

// The exit code for an error: its own for a Failure, otherwise ERROR
pub fn code_of(err: &(dyn Error + 'static)) -> i32 {
    err.downcast_ref::<Failure>()
        .map(|failure| failure.code)
        .unwrap_or(ERROR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_of() {
        let failure: Box<dyn Error> = Box::new(Failure::new(REJECTED, "refused"));
        assert_eq!(code_of(failure.as_ref()), REJECTED);
        assert_eq!(failure.to_string(), "refused");
        let other: Box<dyn Error> = "something else".into();
        assert_eq!(code_of(other.as_ref()), ERROR);
    }
}
