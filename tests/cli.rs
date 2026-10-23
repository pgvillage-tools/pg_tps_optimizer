//! Tests that run the pg_tps_optimizer binary without PostgreSQL.

use std::process::{Command, Stdio};

// When the reader of stdout goes away (e.g. `pg_tps_optimizer | head`), the binary must stop
// quietly, like other command line tools, instead of panicking on the failed write
#[test]
fn stops_quietly_when_stdout_is_closed() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pg_tps_optimizer"))
        // Nothing to connect to: the output before connecting is enough
        .env("PGHOST", "/nonexistent")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("could not start pg_tps_optimizer");
    // Close the read end of stdout before the binary writes to it
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked") && output.status.code() != Some(101),
        "pg_tps_optimizer panicked on a closed stdout ({})\nstderr:\n{}",
        output.status,
        stderr
    );
}
