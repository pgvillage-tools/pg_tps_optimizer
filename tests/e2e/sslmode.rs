// sslmode, TLS and certificates
use crate::common::*;

#[derive(Clone, Copy)]
enum Server {
    WithoutTls,
    WithTls,
}

#[derive(Clone, Copy)]
enum RootCert {
    // The CA that signed the server certificate
    Correct,
    // A CA that did not sign the server certificate
    Wrong,
    // No root certificate at all (not set, and no ~/.postgresql/root.crt)
    Missing,
    // A CA that did not sign the server certificate, while the OS trust store (SSL_CERT_FILE)
    // does trust the server certificate: only sslrootcert may be trusted
    WrongButTrustedByOs,
}

#[derive(Clone, Copy)]
enum Expect {
    Connects,
    FailsCleanly,
}

fn check_sslmode(
    name: &str,
    server: Server,
    host: &str,
    sslmode: &str,
    root_cert: RootCert,
    expect: Expect,
) {
    let container = match server {
        Server::WithoutTls => start_postgres(),
        Server::WithTls => start_postgres_with_tls(server_certs(), PG_HBA_TLS_ONLY),
    };
    let dir = scratch_dir(name);
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();

    let mut env = vec![
        ("PGHOST", host.to_string()),
        (
            "PGPORT",
            container.get_host_port_ipv4(5432).unwrap().to_string(),
        ),
        ("PGUSER", "postgres".to_string()),
        ("PGDATABASE", "postgres".to_string()),
        ("PGSSLMODE", sslmode.to_string()),
    ];
    let root_cert_pem = match root_cert {
        RootCert::Correct => Some(server_certs().ca.as_str()),
        RootCert::Wrong | RootCert::WrongButTrustedByOs => Some(other_ca()),
        RootCert::Missing => None,
    };
    if let Some(pem) = root_cert_pem {
        let path = dir.join("root.crt");
        fs::write(&path, pem).unwrap();
        env.push(("PGSSLROOTCERT", path.display().to_string()));
    }
    if let RootCert::WrongButTrustedByOs = root_cert {
        let path = dir.join("os-trust-store.crt");
        fs::write(&path, &server_certs().ca).unwrap();
        env.push(("SSL_CERT_FILE", path.display().to_string()));
    }

    let run = run_pg_tps_optimizer(&home, &env);
    match expect {
        Expect::Connects => run.assert_success(),
        Expect::FailsCleanly => run.assert_clean_failure(),
    }
}

macro_rules! sslmode_tests {
    ($($name:ident: $server:ident, $host:expr, $sslmode:expr, $root_cert:ident => $expect:ident;)*) => {
        $(
            #[test]
            #[ignore = "requires a container runtime"]
            fn $name() {
                check_sslmode(
                    stringify!($name),
                    Server::$server,
                    $host,
                    $sslmode,
                    RootCert::$root_cert,
                    Expect::$expect,
                );
            }
        )*
    };
}

sslmode_tests! {
    // Server without TLS: only modes that don't require TLS can connect
    sslmode_disable_without_tls: WithoutTls, "localhost", "disable", Correct => Connects;
    sslmode_allow_without_tls: WithoutTls, "localhost", "allow", Correct => Connects;
    sslmode_prefer_without_tls: WithoutTls, "localhost", "prefer", Correct => Connects;
    sslmode_require_without_tls: WithoutTls, "localhost", "require", Correct => FailsCleanly;
    sslmode_verify_ca_without_tls: WithoutTls, "localhost", "verify-ca", Correct => FailsCleanly;
    sslmode_verify_full_without_tls: WithoutTls, "localhost", "verify-full", Correct => FailsCleanly;

    // Server that only accepts TLS: every mode that can use TLS connects
    sslmode_disable_with_tls: WithTls, "localhost", "disable", Correct => FailsCleanly;
    sslmode_allow_with_tls: WithTls, "localhost", "allow", Correct => Connects;
    sslmode_prefer_with_tls: WithTls, "localhost", "prefer", Correct => Connects;
    sslmode_require_with_tls: WithTls, "localhost", "require", Correct => Connects;
    sslmode_verify_ca_with_tls: WithTls, "localhost", "verify-ca", Correct => Connects;
    sslmode_verify_full_with_tls: WithTls, "localhost", "verify-full", Correct => Connects;

    // Root certificate that did not sign the server certificate: only verify-ca and verify-full
    // check it (unlike libpq, require never checks the server certificate)
    sslmode_prefer_with_tls_wrong_ca: WithTls, "localhost", "prefer", Wrong => Connects;
    sslmode_require_with_tls_wrong_ca: WithTls, "localhost", "require", Wrong => Connects;
    sslmode_verify_ca_with_tls_wrong_ca: WithTls, "localhost", "verify-ca", Wrong => FailsCleanly;
    sslmode_verify_full_with_tls_wrong_ca: WithTls, "localhost", "verify-full", Wrong => FailsCleanly;

    // No root certificate: require doesn't verify, verify-* can't
    sslmode_require_with_tls_no_root_cert: WithTls, "localhost", "require", Missing => Connects;
    sslmode_verify_ca_with_tls_no_root_cert: WithTls, "localhost", "verify-ca", Missing => FailsCleanly;
    sslmode_verify_full_with_tls_no_root_cert: WithTls, "localhost", "verify-full", Missing => FailsCleanly;

    // The server certificate is for localhost, not 127.0.0.1: only verify-full checks that
    sslmode_verify_ca_with_tls_hostname_mismatch: WithTls, "127.0.0.1", "verify-ca", Correct => Connects;
    sslmode_verify_full_with_tls_hostname_mismatch: WithTls, "127.0.0.1", "verify-full", Correct => FailsCleanly;

    // Only sslrootcert is trusted, not the CAs of the OS trust store
    sslmode_verify_ca_with_tls_wrong_ca_trusted_by_os: WithTls, "localhost", "verify-ca", WrongButTrustedByOs => FailsCleanly;
    sslmode_verify_full_with_tls_wrong_ca_trusted_by_os: WithTls, "localhost", "verify-full", WrongButTrustedByOs => FailsCleanly;

    // sslmode is ignored for unix sockets, but that must not drop TLS for the TCP hosts in a
    // host list that also has a unix socket
    sslmode_prefer_with_socket_and_tcp_hosts: WithoutTls, "/nonexistent,localhost", "prefer", Correct => Connects;
    sslmode_require_with_socket_and_tcp_hosts: WithoutTls, "/nonexistent,localhost", "require", Correct => FailsCleanly;
}

#[test]
#[ignore = "requires a container runtime"]
fn runs_with_client_certificate_and_verify_full() {
    let certs = Certs::generate("postgres");
    let dir = scratch_dir("runs_with_client_certificate_and_verify_full");
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(dir.join("root.crt"), &certs.ca).unwrap();
    fs::write(dir.join("postgresql.crt"), &certs.client_cert).unwrap();
    fs::write(dir.join("postgresql.key"), &certs.client_key).unwrap();

    let pg = start_postgres_with_tls(&certs, PG_HBA_CLIENT_CERT);
    let run = run_pg_tps_optimizer(
        &home,
        &[
            ("PGHOST", "localhost".to_string()),
            ("PGPORT", pg.get_host_port_ipv4(5432).unwrap().to_string()),
            ("PGUSER", "postgres".to_string()),
            ("PGDATABASE", "postgres".to_string()),
            ("PGSSLMODE", "verify-full".to_string()),
            ("PGSSLROOTCERT", dir.join("root.crt").display().to_string()),
            (
                "PGSSLCERT",
                dir.join("postgresql.crt").display().to_string(),
            ),
            ("PGSSLKEY", dir.join("postgresql.key").display().to_string()),
        ],
    );
    run.assert_success();
}

// Against a server that accepts both TLS and plain connections, prefer must use TLS, and allow
// must not (it only uses TLS when a plain connection fails). PostgreSQL logs "SSL enabled" for
// every authorized TLS connection.
fn check_transport(name: &str, sslmode: &str, expect_tls: bool) {
    let pg = start_postgres_with_tls(server_certs(), PG_HBA_TLS_AND_PLAIN);
    let port = pg.get_host_port_ipv4(5432).unwrap().to_string();
    // Log connections (set at runtime, so this admin connection isn't logged)
    let mut admin = postgres::Client::connect(
        &format!("host=localhost port={} user=postgres dbname=postgres", port),
        postgres::NoTls,
    )
    .unwrap();
    admin
        .batch_execute("ALTER SYSTEM SET log_connections = 'on'")
        .unwrap();
    admin.batch_execute("SELECT pg_reload_conf()").unwrap();

    let home = scratch_dir(name).join("home");
    fs::create_dir_all(&home).unwrap();
    run_pg_tps_optimizer(
        &home,
        &[
            ("PGHOST", "localhost".to_string()),
            ("PGPORT", port),
            ("PGUSER", "postgres".to_string()),
            ("PGDATABASE", "postgres".to_string()),
            ("PGSSLMODE", sslmode.to_string()),
        ],
    )
    .assert_success();

    let log = String::from_utf8_lossy(&pg.stderr_to_vec().unwrap()).to_string();
    let connections: Vec<&str> = log
        .lines()
        .filter(|line| line.contains("connection authorized"))
        .collect();
    // At least the sampler and one client
    assert!(
        connections.len() >= 2,
        "too few connections in the log:\n{}",
        log
    );
    for connection in connections {
        assert_eq!(
            connection.contains("SSL enabled"),
            expect_tls,
            "sslmode={}: expected TLS: {}, got: {}",
            sslmode,
            expect_tls,
            connection
        );
    }
}

#[test]
#[ignore = "requires a container runtime"]
fn sslmode_prefer_uses_tls_when_possible() {
    check_transport("sslmode_prefer_uses_tls_when_possible", "prefer", true);
}

#[test]
#[ignore = "requires a container runtime"]
fn sslmode_allow_uses_plain_when_possible() {
    check_transport("sslmode_allow_uses_plain_when_possible", "allow", false);
}

// sslrootcert=system trusts the CAs of the OS (here set with SSL_CERT_FILE), and requires
// verify-full, like libpq
#[test]
#[ignore = "requires a container runtime"]
fn sslrootcert_system() {
    let pg = start_postgres_with_tls(server_certs(), PG_HBA_TLS_ONLY);
    let dir = scratch_dir("sslrootcert_system");
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    let (server_ca, wrong_ca) = (dir.join("server-ca.crt"), dir.join("wrong-ca.crt"));
    fs::write(&server_ca, &server_certs().ca).unwrap();
    fs::write(&wrong_ca, other_ca()).unwrap();
    let env = |extra: &[(&'static str, String)]| {
        let mut env = vec![
            ("PGHOST", "localhost".to_string()),
            ("PGPORT", pg.get_host_port_ipv4(5432).unwrap().to_string()),
            ("PGUSER", "postgres".to_string()),
            ("PGDATABASE", "postgres".to_string()),
            ("PGSSLROOTCERT", "system".to_string()),
        ];
        env.extend_from_slice(extra);
        env
    };

    // The OS trusts the server CA; sslmode defaults to verify-full
    run_pg_tps_optimizer(
        &home,
        &env(&[("SSL_CERT_FILE", server_ca.display().to_string())]),
    )
    .assert_success();

    // The OS doesn't trust the server CA
    run_pg_tps_optimizer(
        &home,
        &env(&[("SSL_CERT_FILE", wrong_ca.display().to_string())]),
    )
    .assert_clean_failure();

    // A configured SSL_CERT_FILE that doesn't exist is an error (not a fallback to the OS CAs)
    let run = run_pg_tps_optimizer(
        &home,
        &env(&[(
            "SSL_CERT_FILE",
            dir.join("missing.crt").display().to_string(),
        )]),
    );
    run.assert_clean_failure();
    assert!(
        String::from_utf8_lossy(&run.output.stderr).contains("SSL_CERT_FILE"),
        "{}",
        run.describe()
    );

    // A weaker sslmode is refused
    let run = run_pg_tps_optimizer(
        &home,
        &env(&[
            ("SSL_CERT_FILE", server_ca.display().to_string()),
            ("PGSSLMODE", "require".to_string()),
        ]),
    );
    run.assert_clean_failure();
    assert!(
        String::from_utf8_lossy(&run.output.stderr)
            .contains("weak sslmode \"require\" may not be used with sslrootcert=system"),
        "{}",
        run.describe()
    );
}
