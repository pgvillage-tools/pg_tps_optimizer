// Connection settings: connection strings, environment variables and service files
use crate::common::*;

#[test]
#[ignore = "requires a container runtime"]
fn runs_with_connection_uri() {
    let pg = start_postgres();
    let home = scratch_dir("runs_with_connection_uri").join("home");
    fs::create_dir_all(&home).unwrap();
    let uri = format!(
        "postgresql://postgres@localhost:{}/postgres?sslmode=disable&application_name=pg%20tps",
        pg.get_host_port_ipv4(5432).unwrap()
    );
    run_pg_tps_optimizer(&home, &[("PGTPSSOURCE", uri)]).assert_success();
}

#[test]
#[ignore = "requires a container runtime"]
fn runs_with_quoted_connection_string_values() {
    let pg = start_postgres();
    let home = scratch_dir("runs_with_quoted_connection_string_values").join("home");
    fs::create_dir_all(&home).unwrap();
    let conninfo = format!(
        "host = localhost port='{}' user=postgres dbname='postgres' sslmode=disable \
         application_name='pg tps \\'optimizer\\''",
        pg.get_host_port_ipv4(5432).unwrap()
    );
    run_pg_tps_optimizer(&home, &[("PGTPSSOURCE", conninfo)]).assert_success();
}

// With only PGHOSTADDR (no PGHOST), TLS must still work
#[test]
#[ignore = "requires a container runtime"]
fn runs_with_hostaddr_and_tls() {
    let pg = start_postgres_with_tls(server_certs(), PG_HBA_TLS_ONLY);
    let home = scratch_dir("runs_with_hostaddr_and_tls").join("home");
    fs::create_dir_all(&home).unwrap();
    let run = run_pg_tps_optimizer(
        &home,
        &[
            ("PGHOSTADDR", "127.0.0.1".to_string()),
            ("PGPORT", pg.get_host_port_ipv4(5432).unwrap().to_string()),
            ("PGUSER", "postgres".to_string()),
            ("PGDATABASE", "postgres".to_string()),
            ("PGSSLMODE", "require".to_string()),
        ],
    );
    run.assert_success();
}

// libpq environment variables for settings without a default are passed on to the connection
#[test]
#[ignore = "requires a container runtime"]
fn reads_libpq_environment_variables() {
    let pg = start_postgres();
    let home = scratch_dir("reads_libpq_environment_variables").join("home");
    fs::create_dir_all(&home).unwrap();
    let base = vec![
        ("PGHOST", "localhost".to_string()),
        ("PGPORT", pg.get_host_port_ipv4(5432).unwrap().to_string()),
        ("PGUSER", "postgres".to_string()),
        ("PGDATABASE", "postgres".to_string()),
        ("PGSSLMODE", "disable".to_string()),
    ];

    let mut env = base.clone();
    env.push(("PGAPPNAME", "pg tps".to_string()));
    env.push(("PGCONNECT_TIMEOUT", "10".to_string()));
    run_pg_tps_optimizer(&home, &env).assert_success();

    // An invalid value shows the variable is read
    let mut env = base.clone();
    env.push(("PGCONNECT_TIMEOUT", "soon".to_string()));
    let run = run_pg_tps_optimizer(&home, &env);
    run.assert_clean_failure();
    assert!(
        String::from_utf8_lossy(&run.output.stderr).contains("connect_timeout"),
        "{}",
        run.describe()
    );

    // The server is a primary, so a read-only session can't be found
    let mut env = base;
    env.push(("PGTARGETSESSIONATTRS", "read-only".to_string()));
    run_pg_tps_optimizer(&home, &env).assert_clean_failure();
}

// Connection settings from a service file (pg_service.conf), named with PGSERVICE or service=
#[test]
#[ignore = "requires a container runtime"]
fn runs_with_service_file() {
    let pg = start_postgres();
    let dir = scratch_dir("runs_with_service_file");
    let home = dir.join("home");
    let sysconfdir = dir.join("sysconfdir");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&sysconfdir).unwrap();
    let service_file = dir.join("pg_service.conf");
    fs::write(
        &service_file,
        format!(
            "# Services for the e2e tests\n[bench]\nhost=localhost\nport={}\nuser=postgres\n\
             dbname=postgres\nsslmode=disable\n",
            pg.get_host_port_ipv4(5432).unwrap()
        ),
    )
    .unwrap();
    let base = vec![
        ("PGSERVICEFILE", service_file.display().to_string()),
        ("PGSYSCONFDIR", sysconfdir.display().to_string()),
        // The service file wins over environment variables
        ("PGPORT", "1".to_string()),
    ];

    let mut env = base.clone();
    env.push(("PGSERVICE", "bench".to_string()));
    run_pg_tps_optimizer(&home, &env).assert_success();

    let mut env = base.clone();
    env.push(("PGTPSSOURCE", "service=bench".to_string()));
    run_pg_tps_optimizer(&home, &env).assert_success();

    // The connection string wins over the service file
    let mut env = base.clone();
    env.push(("PGTPSSOURCE", "service=bench dbname=nope".to_string()));
    let run = run_pg_tps_optimizer(&home, &env);
    run.assert_clean_failure();
    assert!(
        String::from_utf8_lossy(&run.output.stderr).contains("database \"nope\" does not exist"),
        "{}",
        run.describe()
    );

    let mut env = base;
    env.push(("PGSERVICE", "nope".to_string()));
    let run = run_pg_tps_optimizer(&home, &env);
    run.assert_clean_failure();
    assert!(
        String::from_utf8_lossy(&run.output.stderr)
            .contains("definition of service \"nope\" not found"),
        "{}",
        run.describe()
    );
}
