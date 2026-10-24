// Exit codes for connection errors (see docs/users-guide/usage.md#exit-codes)
use crate::common::*;

fn exit_code(home: &Path, env: &[(&str, String)]) -> Option<i32> {
    let run = run_pg_tps_optimizer(home, env);
    run.assert_clean_failure();
    run.output.status.code()
}

#[test]
#[ignore = "requires a container runtime"]
fn exits_with_distinct_codes_for_connection_errors() {
    // A server with password authentication (scram-sha-256), password "postgres"
    let pg = Postgres::default()
        .with_tag(pg_version())
        .start()
        .expect("could not start postgres container");
    let home = scratch_dir("exits_with_distinct_codes_for_connection_errors").join("home");
    fs::create_dir_all(&home).unwrap();
    let env = |extra: &[(&'static str, String)]| {
        let mut env = vec![
            ("PGHOST", "localhost".to_string()),
            ("PGPORT", pg.get_host_port_ipv4(5432).unwrap().to_string()),
            ("PGUSER", "postgres".to_string()),
            ("PGDATABASE", "postgres".to_string()),
            ("PGPASSWORD", "postgres".to_string()),
            ("PGSSLMODE", "disable".to_string()),
        ];
        env.extend_from_slice(extra);
        env
    };

    // 3: TLS is required, but the server doesn't support it
    assert_eq!(
        exit_code(&home, &env(&[("PGSSLMODE", "require".to_string())])),
        Some(3)
    );
    // 4: the server refuses the connection
    assert_eq!(
        exit_code(&home, &env(&[("PGPASSWORD", "wrong".to_string())])),
        Some(4)
    );
    assert_eq!(
        exit_code(&home, &env(&[("PGDATABASE", "nope".to_string())])),
        Some(4)
    );
    // 0: the run finishes
    let run = run_pg_tps_optimizer(&home, &env(&[]));
    run.assert_success();
    assert_eq!(run.output.status.code(), Some(0));
}
