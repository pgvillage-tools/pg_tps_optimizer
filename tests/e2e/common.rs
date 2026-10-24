// Helpers for the e2e tests: certificates, PostgreSQL containers and running the binary
pub use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair,
};
pub use std::fs;
pub use std::path::{Path, PathBuf};
pub use std::process::{Command, Output, Stdio};
pub use std::sync::OnceLock;
pub use std::thread;
pub use std::time::{Duration, Instant};
pub use testcontainers_modules::postgres::Postgres;
pub use testcontainers_modules::testcontainers::runners::SyncRunner;
pub use testcontainers_modules::testcontainers::{Container, CopyTargetOptions, ImageExt};

pub const CERT_DIR: &str = "/certs";
pub const BINARY_TIMEOUT: Duration = Duration::from_secs(60);

// Only local connections and client certificate authentication over TLS are allowed
pub const PG_HBA_CLIENT_CERT: &str = "local all all trust
hostssl all all 0.0.0.0/0 cert
hostssl all all ::/0 cert
";

// Only TLS connections are allowed (without client certificate), so a successful connection
// proves TLS was used
pub const PG_HBA_TLS_ONLY: &str = "local all all trust
hostssl all all 0.0.0.0/0 trust
hostssl all all ::/0 trust
";

// TLS and plain connections are both allowed (host matches both)
pub const PG_HBA_TLS_AND_PLAIN: &str = "local all all trust
host all all 0.0.0.0/0 trust
host all all ::/0 trust
";

pub fn pg_version() -> String {
    std::env::var("PGVERSION").unwrap_or_else(|_| "18".to_string())
}

// A test specific scratch directory, to write certificates the binary can use
pub fn scratch_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

pub struct Certs {
    pub ca: String,
    pub server_cert: String,
    pub server_key: String,
    pub client_cert: String,
    pub client_key: String,
}

impl Certs {
    // A CA, a server certificate for localhost (and not for 127.0.0.1, to test hostname
    // verification) and a client certificate for user `user`
    pub fn generate(user: &str) -> Certs {
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

pub fn start_postgres() -> Container<Postgres> {
    Postgres::default()
        .with_host_auth()
        .with_tag(pg_version())
        .start()
        .expect("could not start postgres container")
}

pub fn start_postgres_with_tls(certs: &Certs, pg_hba: &str) -> Container<Postgres> {
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
pub fn server_certs() -> &'static Certs {
    static CERTS: OnceLock<Certs> = OnceLock::new();
    CERTS.get_or_init(|| Certs::generate("postgres"))
}

// A CA that did not sign the server certificate
pub fn other_ca() -> &'static str {
    static CERTS: OnceLock<Certs> = OnceLock::new();
    &CERTS.get_or_init(|| Certs::generate("postgres")).ca
}

pub struct Run {
    pub output: Output,
    pub timed_out: bool,
}

// Run the pg_tps_optimizer binary with only the given PG* environment variables, and HOME set
// to an empty directory, so settings and ~/.postgresql from the environment running the tests
// can't leak in
pub fn run_pg_tps_optimizer(home: &Path, env: &[(&str, String)]) -> Run {
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
    pub fn describe(&self) -> String {
        format!(
            "exit: {}, timed out: {}\nstdout:\n{}\nstderr:\n{}",
            self.output.status,
            self.timed_out,
            String::from_utf8_lossy(&self.output.stdout),
            String::from_utf8_lossy(&self.output.stderr)
        )
    }

    pub fn assert_success(&self) {
        let stdout = String::from_utf8_lossy(&self.output.stdout);
        assert!(
            !self.timed_out && self.output.status.success() && stdout.contains("Finished"),
            "expected pg_tps_optimizer to succeed, {}",
            self.describe()
        );
    }

    // A clean failure: a non-zero exit code with an error message, not a panic or a hang
    pub fn assert_clean_failure(&self) {
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
