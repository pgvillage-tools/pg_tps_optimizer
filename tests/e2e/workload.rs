// The workload: transactional and prepared modes
use crate::common::*;

// With --prepared, every client prepares the update once, and executes it for every query. The
// postgres crate gives every prepared statement its own name (s0, s1, ...), so the server log
// shows one statement name per client.
fn check_prepares_once(name: &str, transactional: &str) {
    let pg = start_postgres();
    // Log all statements (set at runtime: as a startup option, the container doesn't become ready
    // for testcontainers)
    let mut admin = postgres::Client::connect(
        &format!(
            "host=localhost port={} user=postgres dbname=postgres",
            pg.get_host_port_ipv4(5432).unwrap()
        ),
        postgres::NoTls,
    )
    .unwrap();
    admin
        .batch_execute("ALTER SYSTEM SET log_statement = 'all'")
        .unwrap();
    admin.batch_execute("SELECT pg_reload_conf()").unwrap();

    let home = scratch_dir(name).join("home");
    fs::create_dir_all(&home).unwrap();
    let run = run_pg_tps_optimizer(
        &home,
        &[
            ("PGHOST", "localhost".to_string()),
            ("PGPORT", pg.get_host_port_ipv4(5432).unwrap().to_string()),
            ("PGUSER", "postgres".to_string()),
            ("PGDATABASE", "postgres".to_string()),
            ("PGSSLMODE", "disable".to_string()),
            ("PGTPSPREPARED", "true".to_string()),
            ("PGTPSTRANSACTIONAL", transactional.to_string()),
        ],
    );
    run.assert_success();

    let log = String::from_utf8_lossy(&pg.stderr_to_vec().unwrap()).to_string();
    let names: std::collections::HashSet<&str> = log
        .lines()
        .filter(|line| line.contains("update pg_tps_optimizer set id=$1"))
        .filter_map(|line| line.split("execute ").nth(1))
        .filter_map(|rest| rest.split(':').next())
        .collect();
    // One client (PGTPSRANGE=1:2), so one prepared statement
    assert_eq!(
        names.len(),
        1,
        "expected 1 prepared statement, got {:?}",
        names
    );
}

#[test]
#[ignore = "requires a container runtime"]
fn prepares_statements_once_per_client() {
    check_prepares_once("prepares_statements_once_per_client", "false");
}

#[test]
#[ignore = "requires a container runtime"]
fn prepares_statements_once_per_client_in_transactions() {
    check_prepares_once(
        "prepares_statements_once_per_client_in_transactions",
        "true",
    );
}
