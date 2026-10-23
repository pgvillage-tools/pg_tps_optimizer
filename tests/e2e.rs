//! End-to-end tests: run the pg_tps_optimizer binary against PostgreSQL in a container.
//!
//! These tests need a container runtime (docker, or podman with its docker socket) and are
//! ignored by default. Run them with:
//!
//!     cargo test --test e2e -- --ignored
//!
//! Set PGVERSION to test against another PostgreSQL major version (default: 18).
//!
//! The sslmode tests describe the expected behavior for every sslmode, against a server without
//! TLS and a server that only accepts TLS connections. Expectations follow libpq (psql), see
//! https://www.postgresql.org/docs/current/libpq-ssl.html#LIBPQ-SSL-PROTECTION

use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::SyncRunner;
use testcontainers_modules::testcontainers::{Container, CopyTargetOptions, ImageExt};

const CERT_DIR: &str = "/certs";
const BINARY_TIMEOUT: Duration = Duration::from_secs(60);

// Only local connections and client certificate authentication over TLS are allowed
const PG_HBA_CLIENT_CERT: &str = "local all all trust
hostssl all all 0.0.0.0/0 cert
hostssl all all ::/0 cert
";

// Only TLS connections are allowed (without client certificate), so a successful connection
// proves TLS was used
const PG_HBA_TLS_ONLY: &str = "local all all trust
hostssl all all 0.0.0.0/0 trust
hostssl all all ::/0 trust
";

// TLS and plain connections are both allowed (host matches both)
const PG_HBA_TLS_AND_PLAIN: &str = "local all all trust
host all all 0.0.0.0/0 trust
host all all ::/0 trust
";

fn pg_version() -> String {
    std::env::var("PGVERSION").unwrap_or_else(|_| "18".to_string())
}

// A test specific scratch directory, to write certificates the binary can use
fn scratch_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

struct Certs {
    ca: String,
    server_cert: String,
    server_key: String,
    client_cert: String,
    client_key: String,
}

impl Certs {
    // A CA, a server certificate for localhost (and not for 127.0.0.1, to test hostname
    // verification) and a client certificate for user `user`
    fn generate(user: &str) -> Certs {
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "pg_tps_optimizer e2e CA");
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().unwrap()).unwrap();

        let mut server_params = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
        server_params
            .distinguished_name
            .push(DnType::CommonName, "localhost");
        server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server_key = KeyPair::generate().unwrap();
        let server_cert = server_params.signed_by(&server_key, &ca).unwrap();

        // With `cert` authentication, PostgreSQL maps the CN to the database user
        let mut client_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        client_params
            .distinguished_name
            .push(DnType::CommonName, user);
        client_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let client_key = KeyPair::generate().unwrap();
        let client_cert = client_params.signed_by(&client_key, &ca).unwrap();

        Certs {
            ca: ca.pem(),
            server_cert: server_cert.pem(),
            server_key: server_key.serialize_pem(),
            client_cert: client_cert.pem(),
            client_key: client_key.serialize_pem(),
        }
    }
}

fn start_postgres() -> Container<Postgres> {
    Postgres::default()
        .with_host_auth()
        .with_tag(pg_version())
        .start()
        .expect("could not start postgres container")
}

fn start_postgres_with_tls(certs: &Certs, pg_hba: &str) -> Container<Postgres> {
    // PostgreSQL refuses a private key that is not owned by postgres with mode 0600 (or root
    // with 0640), and copied files are owned by root. So install the key with the right owner
    // and mode before handing over to the regular entrypoint.
    let cmd = format!(
        "install -o postgres -g postgres -m 0600 {dir}/server.key /tmp/server.key && \
         exec docker-entrypoint.sh postgres -c fsync=off -c ssl=on \
         -c ssl_cert_file={dir}/server.crt -c ssl_key_file=/tmp/server.key \
         -c ssl_ca_file={dir}/ca.crt -c hba_file={dir}/pg_hba.conf",
        dir = CERT_DIR
    );
    Postgres::default()
        .with_tag(pg_version())
        .with_copy_to(
            CopyTargetOptions::new(format!("{}/ca.crt", CERT_DIR)),
            certs.ca.clone().into_bytes(),
        )
        .with_copy_to(
            CopyTargetOptions::new(format!("{}/server.crt", CERT_DIR)),
            certs.server_cert.clone().into_bytes(),
        )
        .with_copy_to(
            CopyTargetOptions::new(format!("{}/server.key", CERT_DIR)).with_mode(0o600),
            certs.server_key.clone().into_bytes(),
        )
        .with_copy_to(
            CopyTargetOptions::new(format!("{}/pg_hba.conf", CERT_DIR)),
            pg_hba.as_bytes().to_vec(),
        )
        .with_cmd(["bash", "-c", cmd.as_str()])
        .start()
        .expect("could not start postgres container with tls")
}

// Every sslmode test starts its own server: containers are only removed when dropped, which
// never happens for containers in a static. The certificates can be shared.
fn server_certs() -> &'static Certs {
    static CERTS: OnceLock<Certs> = OnceLock::new();
    CERTS.get_or_init(|| Certs::generate("postgres"))
}

// A CA that did not sign the server certificate
fn other_ca() -> &'static str {
    static CERTS: OnceLock<Certs> = OnceLock::new();
    &CERTS.get_or_init(|| Certs::generate("postgres")).ca
}

struct Run {
    output: Output,
    timed_out: bool,
}

// Run the pg_tps_optimizer binary with only the given PG* environment variables, and HOME set
// to an empty directory, so settings and ~/.postgresql from the environment running the tests
// can't leak in
fn run_pg_tps_optimizer(home: &Path, env: &[(&str, String)]) -> Run {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_pg_tps_optimizer"));
    for (key, _) in std::env::vars() {
        // SSL_CERT_FILE and SSL_CERT_DIR change the OS trust store OpenSSL uses
        if key.starts_with("PG") || key.starts_with("SSL_CERT_") {
            cmd.env_remove(key);
        }
    }
    // Keep the run short: only 1 client, and don't wait long for it to stabilize
    cmd.env("HOME", home)
        .env("PGTPSRANGE", "1:2")
        .env("PGTPSMAXWAIT", "5s")
        .envs(env.iter().map(|(k, v)| (*k, v)))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("could not start pg_tps_optimizer");
    let start = Instant::now();
    let mut timed_out = false;
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > BINARY_TIMEOUT {
            child.kill().unwrap();
            timed_out = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    Run {
        output: child.wait_with_output().unwrap(),
        timed_out,
    }
}

impl Run {
    fn describe(&self) -> String {
        format!(
            "exit: {}, timed out: {}\nstdout:\n{}\nstderr:\n{}",
            self.output.status,
            self.timed_out,
            String::from_utf8_lossy(&self.output.stdout),
            String::from_utf8_lossy(&self.output.stderr)
        )
    }

    fn assert_success(&self) {
        let stdout = String::from_utf8_lossy(&self.output.stdout);
        assert!(
            !self.timed_out && self.output.status.success() && stdout.contains("Finished"),
            "expected pg_tps_optimizer to succeed, {}",
            self.describe()
        );
    }

    // A clean failure: a non-zero exit code with an error message, not a panic or a hang
    fn assert_clean_failure(&self) {
        let stderr = String::from_utf8_lossy(&self.output.stderr);
        assert!(
            !self.timed_out
                && !self.output.status.success()
                && !stderr.contains("panicked")
                && !stderr.trim().is_empty(),
            "expected pg_tps_optimizer to fail cleanly (non-zero exit, error message, no panic), {}",
            self.describe()
        );
    }
}

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
