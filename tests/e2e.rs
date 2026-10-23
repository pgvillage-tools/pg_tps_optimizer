//! End-to-end tests: run the pg_tps_optimizer binary against PostgreSQL in a container.
//!
//! These tests need a container runtime (docker, or podman with its docker socket) and are
//! ignored by default. Run them with:
//!
//!     cargo test --test e2e -- --ignored
//!
//! Set PGVERSION to test against another PostgreSQL major version (default: 18).

use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::SyncRunner;
use testcontainers_modules::testcontainers::{Container, CopyTargetOptions, ImageExt};

const CERT_DIR: &str = "/certs";
const BINARY_TIMEOUT: Duration = Duration::from_secs(120);

// Only local connections and client certificate authentication over TLS are allowed
const PG_HBA: &str = "local all all trust
hostssl all all 0.0.0.0/0 cert
hostssl all all ::/0 cert
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
    // A CA, a server certificate for localhost and a client certificate for user `user`
    fn generate(user: &str) -> Certs {
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "pg_tps_optimizer e2e CA");
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().unwrap()).unwrap();

        let mut server_params =
            CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()]).unwrap();
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

fn start_postgres_with_tls(certs: &Certs) -> Container<Postgres> {
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
            PG_HBA.as_bytes().to_vec(),
        )
        .with_cmd(["bash", "-c", cmd.as_str()])
        .start()
        .expect("could not start postgres container with tls")
}

// Run the pg_tps_optimizer binary with only the given PG* environment variables, so settings
// from the environment running the tests can't leak in
fn run_pg_tps_optimizer(env: &[(&str, String)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_pg_tps_optimizer"));
    for (key, _) in std::env::vars() {
        if key.starts_with("PG") {
            cmd.env_remove(key);
        }
    }
    // Keep the run short: only 1 client, and don't wait long for it to stabilize
    cmd.env("PGTPSRANGE", "1:2")
        .env("PGTPSMAXWAIT", "5s")
        .envs(env.iter().map(|(k, v)| (*k, v)))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("could not start pg_tps_optimizer");
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > BINARY_TIMEOUT {
            child.kill().unwrap();
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    child.wait_with_output().unwrap()
}

fn assert_success(output: &Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("Finished"),
        "pg_tps_optimizer failed ({})\nstdout:\n{}\nstderr:\n{}",
        output.status,
        stdout,
        stderr
    );
}

#[test]
#[ignore = "requires a container runtime"]
fn runs_without_tls() {
    let pg = start_postgres();
    let output = run_pg_tps_optimizer(&[
        ("PGHOST", pg.get_host().unwrap().to_string()),
        ("PGPORT", pg.get_host_port_ipv4(5432).unwrap().to_string()),
        ("PGUSER", "postgres".to_string()),
        ("PGDATABASE", "postgres".to_string()),
        ("PGSSLMODE", "disable".to_string()),
    ]);
    assert_success(&output);
}

#[test]
#[ignore = "requires a container runtime"]
fn runs_with_client_certificate_and_verify_full() {
    let certs = Certs::generate("postgres");
    let dir = scratch_dir("runs_with_client_certificate_and_verify_full");
    fs::write(dir.join("root.crt"), &certs.ca).unwrap();
    fs::write(dir.join("postgresql.crt"), &certs.client_cert).unwrap();
    fs::write(dir.join("postgresql.key"), &certs.client_key).unwrap();

    let pg = start_postgres_with_tls(&certs);
    let output = run_pg_tps_optimizer(&[
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
    ]);
    assert_success(&output);
}
