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

fn table_exists(pg: &Container<Postgres>, table: &str) -> bool {
    let mut admin = postgres::Client::connect(
        &format!(
            "host=localhost port={} user=postgres dbname=postgres",
            pg.get_host_port_ipv4(5432).unwrap()
        ),
        postgres::NoTls,
    )
    .unwrap();
    let row = admin
        .query_one(
            "select count(*) from pg_tables where tablename = $1",
            &[&table],
        )
        .unwrap();
    row.get::<_, i64>(0) > 0
}

fn query_env(
    pg: &Container<Postgres>,
    extra: &[(&'static str, String)],
) -> Vec<(&'static str, String)> {
    let mut env = vec![
        ("PGHOST", "localhost".to_string()),
        ("PGPORT", pg.get_host_port_ipv4(5432).unwrap().to_string()),
        ("PGUSER", "postgres".to_string()),
        ("PGDATABASE", "postgres".to_string()),
        ("PGSSLMODE", "disable".to_string()),
    ];
    env.extend_from_slice(extra);
    env
}

// The default workload creates table pg_tps_optimizer, and removes it after the run
#[test]
#[ignore = "requires a container runtime"]
fn removes_the_table_of_the_default_workload() {
    let pg = start_postgres();
    let home = scratch_dir("removes_the_table_of_the_default_workload").join("home");
    fs::create_dir_all(&home).unwrap();
    run_pg_tps_optimizer(&home, &query_env(&pg, &[])).assert_success();
    assert!(!table_exists(&pg, "pg_tps_optimizer"));
}

// --query runs a custom query, with the number of the client as $1, as the type the query needs;
// it doesn't create table pg_tps_optimizer
#[test]
#[ignore = "requires a container runtime"]
fn runs_a_custom_query() {
    let pg = start_postgres();
    let home = scratch_dir("runs_a_custom_query").join("home");
    fs::create_dir_all(&home).unwrap();
    for (query, prepared) in [
        ("select 1", "false"),
        ("select $1::int", "false"),
        ("select 'client ' || $1::text", "true"),
        ("select $1::bigint * 2", "true"),
    ] {
        let run = run_pg_tps_optimizer(
            &home,
            &query_env(
                &pg,
                &[
                    ("PGTPSQUERY", query.to_string()),
                    ("PGTPSPREPARED", prepared.to_string()),
                ],
            ),
        );
        run.assert_success();
    }
    assert!(!table_exists(&pg, "pg_tps_optimizer"));

    // An invalid query, or one with more than $1, stops before the clients start
    for (query, error) in [
        ("selec 1", "syntax error"),
        ("select $1::bool", "the query uses it as bool"),
        ("select $1::int + $2::int", "only $1"),
    ] {
        let run =
            run_pg_tps_optimizer(&home, &query_env(&pg, &[("PGTPSQUERY", query.to_string())]));
        run.assert_clean_failure();
        // An invalid query is an invalid option: exit code 2
        assert_eq!(run.output.status.code(), Some(2), "{}", run.describe());
        let stderr = String::from_utf8_lossy(&run.output.stderr).to_string();
        assert!(
            stderr.contains("invalid query") && stderr.contains(error),
            "{}: {}",
            query,
            run.describe()
        );
    }
}
