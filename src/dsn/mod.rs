use crate::exit::{self, Failure};
use crate::generic;

mod conninfo;
mod passfile;
mod service;

use openssl::ssl::{SslConnector, SslFiletype, SslMethod, SslVerifyMode};
use openssl::x509::store::X509StoreBuilder;
use postgres::config::SslMode;
use postgres::{Client, Config, NoTls};
use postgres_openssl::MakeTlsConnector;
use std::collections::HashMap;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use uzers::{get_current_uid, get_user_by_uid};

#[derive(Debug, Clone)]
pub struct Dsn {
    kv: HashMap<String, String>,
    ssl_mode: String,
    // Whether sslmode is set (PGSSLMODE or sslmode=), or is the default
    ssl_mode_set: bool,
    // Errors from parsing the connection string or the service file, reported when connecting
    invalid: Vec<String>,
}

// The postgres crate keeps the details (like the server message) in the error source, so add
// the sources to the message (skipping sources that are already part of it)
fn error_chain(err: &dyn Error) -> String {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(err) = source {
        let text = err.to_string();
        if !message.contains(&text) {
            message = format!("{}: {}", message, text);
        }
        source = err.source();
    }
    message
}

// Unix socket directories to look for a running server when no host is set. libpq uses the one
// directory it was built with: /var/run/postgresql on Debian/Ubuntu (and RHEL for newer
// versions), /tmp for most other builds.
const SOCKET_DIRS: [&str; 2] = ["/var/run/postgresql", "/tmp"];

// The first directory with a socket for this port, or the last directory when none has one
fn default_socket_dir(dirs: &[&str], port: &str) -> String {
    dirs.iter()
        .find(|dir| Path::new(dir).join(format!(".s.PGSQL.{}", port)).exists())
        .unwrap_or(&dirs[dirs.len() - 1])
        .to_string()
}

// libpq environment variables (https://www.postgresql.org/docs/current/libpq-envars.html) for
// settings without a default, and the connection setting they set
const ENV_SETTINGS: [(&str, &str); 8] = [
    ("PGHOSTADDR", "hostaddr"),
    ("PGAPPNAME", "application_name"),
    ("PGOPTIONS", "options"),
    ("PGCONNECT_TIMEOUT", "connect_timeout"),
    ("PGTARGETSESSIONATTRS", "target_session_attrs"),
    ("PGCHANNELBINDING", "channel_binding"),
    ("PGLOADBALANCEHOSTS", "load_balance_hosts"),
    ("PGSSLNEGOTIATION", "sslnegotiation"),
];

#[derive(Debug, PartialEq)]
struct CaLocations {
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

// The CA certificates for sslrootcert=system. When SSL_CERT_FILE or SSL_CERT_DIR is set, only those
// (SSL_CERT_DIR can be a list of directories, separated by :), and they must exist. Otherwise the
// usual locations of the OS, as the OpenSSL built into the binary doesn't know them.
fn system_ca_locations(
    cert_file: Option<OsString>,
    cert_dir: Option<OsString>,
) -> Result<CaLocations, String> {
    let cert_file = cert_file.filter(|value| !value.is_empty());
    let cert_dir = cert_dir.filter(|value| !value.is_empty());
    if cert_file.is_none() && cert_dir.is_none() {
        let probe = openssl_probe::probe();
        if probe.cert_file.is_none() && probe.cert_dir.is_empty() {
            return Err(
                "sslrootcert=system: no certificates of the OS found; set SSL_CERT_FILE or \
                 SSL_CERT_DIR"
                    .to_string(),
            );
        }
        return Ok(CaLocations {
            files: probe.cert_file.into_iter().collect(),
            dirs: probe.cert_dir,
        });
    }
    let files: Vec<PathBuf> = cert_file.iter().map(PathBuf::from).collect();
    if let Some(file) = files.iter().find(|file| !file.is_file()) {
        return Err(format!(
            "SSL_CERT_FILE \"{}\" does not exist",
            file.display()
        ));
    }
    let dirs: Vec<PathBuf> = cert_dir
        .iter()
        .flat_map(std::env::split_paths)
        .filter(|dir| !dir.as_os_str().is_empty())
        .collect();
    if let Some(dir) = dirs.iter().find(|dir| !dir.is_dir()) {
        return Err(format!("SSL_CERT_DIR \"{}\" does not exist", dir.display()));
    }
    Ok(CaLocations { files, dirs })
}

fn os_user_name() -> String {
    let mut user = generic::get_env_str("", "PGUSER", "").to_string();
    if user.is_empty() {
        user = match get_user_by_uid(get_current_uid()).unwrap().name().to_str() {
            Some(osuser) => osuser.to_string(),
            None => "".to_string(),
        };
    }
    user.to_string()
}

impl fmt::Display for Dsn {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut vec = Vec::new();
        for (k, mut v) in self.clone().kv {
            v = v.replace('\\', r"\\");
            v = v.replace('\'', r"\'");
            vec.push(format!("{0}='{1}'", k, v))
        }
        vec.sort();
        write!(f, "{}", vec.join(" "))
    }
}

impl Dsn {
    pub fn from_string(from: &str) -> Dsn {
        let mut dsn = Dsn::new();
        let explicit = match conninfo::parse(from) {
            Ok(pairs) => pairs,
            Err(err) => {
                dsn.invalid
                    .push(format!("invalid connection string: {}", err));
                Vec::new()
            }
        };
        // Settings from a service file override environment variables, and are overridden by
        // the connection string, like libpq
        let service = explicit
            .iter()
            .rev()
            .find(|(key, _)| key == "service")
            .map(|(_, value)| value.to_string())
            .or_else(|| std::env::var("PGSERVICE").ok());
        let mut pairs = Vec::new();
        if let Some(name) = service {
            match service::lookup(&name, &service::service_files()) {
                Ok(settings) => pairs.extend(settings),
                Err(err) => dsn.invalid.push(err),
            }
        }
        pairs.extend(explicit.into_iter().filter(|(key, _)| key != "service"));
        let passfile_setting = pairs
            .iter()
            .rev()
            .find(|(key, _)| key == "passfile")
            .map(|(_, value)| value.to_string());
        for (key, value) in pairs.iter().filter(|(key, _)| key != "passfile") {
            dsn.set_value(key, value);
        }
        let has = |name: &str| pairs.iter().any(|(key, _)| key == name);
        if !has("host") {
            if has("hostaddr") {
                // The postgres crate needs a host name for TLS, so use the address, like libpq
                let hostaddr = dsn.get_value("hostaddr", "");
                dsn.set_value("host", &hostaddr);
            } else if std::env::var("PGHOST").is_err() && std::env::var("PGHOSTADDR").is_err() {
                // Look for the socket of the port from the connection string
                let port = dsn.get_value("port", "5432");
                dsn.set_value("host", &default_socket_dir(&SOCKET_DIRS, &port));
            }
        }
        // sslrootcert=system trusts the CAs of the OS, which only protects against a
        // man-in-the-middle with a host name check, so it requires verify-full, like libpq
        if dsn.get_value("sslrootcert", "") == "system" {
            if !dsn.ssl_mode_set {
                dsn.set_value("sslmode", "verify-full");
            } else if dsn.ssl_mode != "verify-full" {
                let err = format!(
                    "weak sslmode \"{}\" may not be used with sslrootcert=system (use \"verify-full\")",
                    dsn.ssl_mode
                );
                dsn.invalid.push(err);
            }
        }

        // Without a password, look in the password file, for the first host and port
        if dsn.get_value("password", "").is_empty() {
            let host = dsn.get_value("host", "");
            let port = dsn.get_value("port", "5432");
            let target = passfile::Target {
                host: host.split(',').next().unwrap_or(""),
                port: port.split(',').next().unwrap_or("5432"),
                dbname: &dsn.get_value("dbname", ""),
                user: &dsn.get_value("user", ""),
            };
            let file = passfile::passfile(passfile_setting.as_deref());
            if let Some(password) = passfile::lookup(&file, &target) {
                dsn.set_value("password", &password);
            }
        }
        dsn
    }
    pub fn cleanse(&self) -> Dsn {
        let mut kv: HashMap<String, String> = HashMap::new();
        kv.extend(self.clone().kv);
        kv.remove("sslmode");
        kv.remove("sslcert");
        kv.remove("sslkey");
        kv.remove("sslrootcert");
        // sslcrl is not supported (yet), ignore it so a DSN that sets it can still connect
        kv.remove("sslcrl");
        let ssl_mode = "disable".to_string();
        Dsn {
            kv,
            ssl_mode,
            ssl_mode_set: self.ssl_mode_set,
            invalid: self.invalid.clone(),
        }
    }
    pub fn new() -> Dsn {
        let mut kv: HashMap<String, String> = HashMap::new();

        kv.insert("user".to_string(), os_user_name());
        kv.insert(
            "dbname".to_string(),
            generic::get_env_str("", "PGDATABASE", os_user_name().as_str()),
        );
        let port = generic::get_env_str("", "PGPORT", "5432");
        // Without PGHOST, use PGHOSTADDR as host (the postgres crate needs a host name for TLS),
        // or look for the default unix socket
        let hostaddr = generic::get_env_str("", "PGHOSTADDR", "");
        let default_host = match hostaddr.is_empty() {
            true => default_socket_dir(&SOCKET_DIRS, &port),
            false => hostaddr,
        };
        kv.insert(
            "host".to_string(),
            generic::get_env_str("", "PGHOST", &default_host),
        );
        kv.insert("port".to_string(), port);
        // Other libpq environment variables, for settings the postgres crate supports
        for (env_key, key) in ENV_SETTINGS {
            if let Ok(value) = std::env::var(env_key) {
                kv.insert(key.to_string(), value);
            }
        }
        let ssl_mode = generic::get_env_str("", "PGSSLMODE", "prefer");
        kv.insert("sslmode".to_string(), ssl_mode.to_string());
        kv.insert(
            "sslcert".to_string(),
            generic::get_env_path("", "PGSSLCERT", "~/.postgresql/postgresql.crt"),
        );
        kv.insert(
            "sslkey".to_string(),
            generic::get_env_path("", "PGSSLKEY", "~/.postgresql/postgresql.key"),
        );
        kv.insert(
            "sslrootcert".to_string(),
            generic::get_env_path("", "PGSSLROOTCERT", "~/.postgresql/root.crt"),
        );
        kv.insert(
            "password".to_string(),
            // Annoyingly, rust module does not work well with empty password
            // So, for empty [assword we use a string containing just a letter p
            generic::get_env_str("", "PGPASSWORD", ""),
        );
        Dsn {
            kv,
            ssl_mode,
            ssl_mode_set: std::env::var("PGSSLMODE").is_ok(),
            invalid: Vec::new(),
        }
    }
    pub fn debug(&self) -> String {
        let mut vec = Vec::new();
        for (k, mut v) in self.clone().kv {
            if k == "password" {
                v = "*****".to_string();
            }
            v = v.replace('\\', r"\\");
            v = v.replace('\'', r"\'");
            vec.push(format!("{0}='{1}'", k, v))
        }
        vec.sort();
        vec.join(" ")
    }
    fn set_value(&mut self, key: &str, value: &str) {
        self.kv.insert(key.to_string(), value.to_string());
        if key.eq("sslmode") {
            self.ssl_mode = value.to_string();
            self.ssl_mode_set = true;
        }
    }
    fn get_value(&self, key: &str, default: &str) -> String {
        match self.kv.get_key_value(key) {
            Some(kv) => {
                let (k, v) = kv;
                if key.eq(k) {
                    return v.to_string();
                }
            }
            None => return default.to_string(),
        }
        default.to_string()
    }
    #[cfg(test)]
    pub fn use_tls(&self) -> bool {
        self.ssl_mode.ne("disable")
    }
    pub fn verify_hostname(&self) -> bool {
        self.ssl_mode.eq("verify-full")
    }
    pub fn verify_ca(&self) -> bool {
        self.ssl_mode.eq("verify-ca") || self.ssl_mode.eq("verify-full")
    }
    // Connect like libpq would for the configured sslmode. The postgres crate only knows
    // disable, prefer and require, so allow, verify-ca and verify-full are built from those, and
    // the certificate checks are configured on the TLS connector.
    // Connect, with errors that carry the exit code: invalid settings are exit::USAGE, a server
    // that can't be reached (or TLS that fails) exit::UNREACHABLE, and a server that refuses the
    // connection exit::REJECTED
    pub fn client(&self) -> Result<Client, Box<dyn Error>> {
        let usage = |message: String| Box::new(Failure::new(exit::USAGE, message));
        if let Some(err) = self.invalid.first() {
            return Err(usage(err.to_string()));
        }
        let mut config: Config = self
            .cleanse()
            .to_string()
            .parse()
            .map_err(|e: postgres::Error| usage(error_chain(&e)))?;
        let tls = || self.tls_connector().map_err(|e| usage(e.to_string()));
        // Like libpq, sslmode is ignored for unix sockets. The postgres crate uses one sslmode
        // for all hosts, so a mix of unix sockets and TCP hosts can't skip TLS for the sockets
        // only, and must not drop TLS for the TCP hosts.
        let host = self.get_value("host", "");
        let sockets = host.split(',').filter(|h| h.starts_with('/')).count();
        let ssl_mode = if sockets == host.split(',').count() {
            "disable"
        } else if sockets > 0
            && ["require", "verify-ca", "verify-full"].contains(&self.ssl_mode.as_str())
        {
            return Err(usage(format!(
                "host \"{}\" mixes unix sockets and TCP hosts, which is not supported with \
                 sslmode={}",
                host, self.ssl_mode
            )));
        } else {
            self.ssl_mode.as_str()
        };
        let result = match ssl_mode {
            "disable" => config.ssl_mode(SslMode::Disable).connect(NoTls),
            // First try without TLS, and only with TLS when that fails
            "allow" => match config.clone().ssl_mode(SslMode::Disable).connect(NoTls) {
                Ok(client) => Ok(client),
                Err(plain) => match config.ssl_mode(SslMode::Require).connect(tls()?) {
                    // When the server refused the plain connection, and TLS fails without an
                    // answer from the server, the refusal is the error that matters
                    Err(tls_error)
                        if tls_error.as_db_error().is_none() && plain.as_db_error().is_some() =>
                    {
                        Err(plain)
                    }
                    result => result,
                },
            },
            "prefer" => config.ssl_mode(SslMode::Prefer).connect(tls()?),
            "require" | "verify-ca" | "verify-full" => {
                config.ssl_mode(SslMode::Require).connect(tls()?)
            }
            other => return Err(usage(format!("invalid sslmode value: \"{}\"", other))),
        };
        result.map_err(|e| {
            // An error from the server means it refused the connection
            let code = match e.as_db_error() {
                Some(_) => exit::REJECTED,
                None => exit::UNREACHABLE,
            };
            let message = format!(
                "connection to server at \"{}\", port {} failed: {}",
                self.get_value("host", ""),
                self.get_value("port", ""),
                error_chain(&e)
            );
            Box::new(Failure::new(code, message)) as Box<dyn Error>
        })
    }
    fn tls_connector(&self) -> Result<MakeTlsConnector, Box<dyn Error>> {
        let mut builder = SslConnector::builder(SslMethod::tls())?;

        // Only use a client certificate when one is set, or the default one exists. The
        // certificate and key only work as a pair.
        let cert_file = self.get_value("sslcert", "");
        let key_file = self.get_value("sslkey", "");
        if cert_file.is_empty() && !key_file.is_empty() {
            return Err(format!(
                "sslkey is set (\"{}\"), but sslcert is not; a client certificate needs both",
                key_file
            )
            .into());
        }
        if !cert_file.is_empty() && key_file.is_empty() {
            return Err(format!(
                "sslcert is set (\"{}\"), but sslkey is not; a client certificate needs both",
                cert_file
            )
            .into());
        }
        if !cert_file.is_empty() {
            builder
                .set_certificate_chain_file(&cert_file)
                .map_err(|e| {
                    format!("could not load client certificate \"{}\": {}", cert_file, e)
                })?;
            builder
                .set_private_key_file(&key_file, SslFiletype::PEM)
                .map_err(|e| format!("could not load private key \"{}\": {}", key_file, e))?;
        }

        // Only verify-ca and verify-full check the server certificate
        if self.verify_ca() {
            let root_cert = self.get_value("sslrootcert", "");
            if root_cert == "system" {
                return self.system_tls_connector(builder);
            }
            if root_cert.is_empty() || !Path::new(&root_cert).exists() {
                return Err(format!(
                    "root certificate file \"{}\" does not exist; either provide the file or \
                     change sslmode to disable server certificate verification",
                    match root_cert.is_empty() {
                        true => "~/.postgresql/root.crt",
                        false => root_cert.as_str(),
                    }
                )
                .into());
            }
            // SslConnector::builder trusts the CAs of the OS trust store, but only sslrootcert
            // may be trusted, so start from an empty store
            builder.set_cert_store(X509StoreBuilder::new()?.build());
            builder
                .set_ca_file(&root_cert)
                .map_err(|e| format!("could not load root certificate \"{}\": {}", root_cert, e))?;
        } else {
            builder.set_verify(SslVerifyMode::NONE);
        }

        self.finish_tls_connector(builder)
    }
    // sslrootcert=system: trust the CAs of the OS (see system_ca_locations)
    fn system_tls_connector(
        &self,
        mut builder: openssl::ssl::SslConnectorBuilder,
    ) -> Result<MakeTlsConnector, Box<dyn Error>> {
        builder.set_cert_store(X509StoreBuilder::new()?.build());
        let locations = system_ca_locations(
            std::env::var_os("SSL_CERT_FILE"),
            std::env::var_os("SSL_CERT_DIR"),
        )?;
        for file in &locations.files {
            builder
                .load_verify_locations(Some(file), None)
                .map_err(|e| format!("could not load \"{}\": {}", file.display(), e))?;
        }
        for dir in &locations.dirs {
            builder
                .load_verify_locations(None, Some(dir))
                .map_err(|e| format!("could not load \"{}\": {}", dir.display(), e))?;
        }
        self.finish_tls_connector(builder)
    }
    fn finish_tls_connector(
        &self,
        builder: openssl::ssl::SslConnectorBuilder,
    ) -> Result<MakeTlsConnector, Box<dyn Error>> {
        let mut connector = MakeTlsConnector::new(builder.build());
        let verify_hostname = self.verify_hostname();
        connector.set_callback(move |config, _| {
            config.set_verify_hostname(verify_hostname);
            Ok(())
        });
        Ok(connector)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use postgres::Error;
    use std::fs;

    #[test]
    fn test_new() {
        // Lets record the envvars we set
        let mut envvars = HashMap::new();
        envvars.insert("PGHOST", "here");
        envvars.insert("PGDATABASE", "there");
        envvars.insert("PGUSER", "me");
        envvars.insert("PGSSLMODE", "disable");
        envvars.insert("PGSSLCERT", "~/cert");
        envvars.insert("PGSSLKEY", "~/key");
        envvars.insert("PGSSLROOTCERT", "~/root");
        // and set them
        for (key, value) in envvars.iter() {
            std::env::set_var(key, value);
        }

        // Lets test with these set
        let mut d = Dsn::new();
        assert!(!d.use_tls());
        assert!(!d.verify_hostname());
        d.set_value("sslmode", "verify-full");
        assert!(d.use_tls());
        assert!(d.verify_hostname());
        let home_dir = home::home_dir().unwrap().display().to_string();
        let expected = concat!(
            "dbname='there' ",
            "host='here' ",
            "password='' ",
            "port='5432' ",
            "sslcert='~/cert' ",
            "sslkey='~/key' ",
            "sslmode='verify-full' ",
            "sslrootcert='~/root' ",
            "user='me'",
        );
        assert_eq!(d.to_string(), expected.replace("~", home_dir.as_str()),);
        // and unset them
        for key in envvars.keys() {
            std::env::remove_var(key);
        }
        // And test without them being set
        d = Dsn::new();
        assert!(d.use_tls());
        assert_eq!(
            d.cleanse().to_string(),
            format!(
                "dbname='{0}' host='{1}' password='' port='5432' user='{0}'",
                os_user_name(),
                default_socket_dir(&SOCKET_DIRS, "5432")
            )
        );
        let sslcert = generic::shell_exists("~/.postgresql/postgresql.crt");
        let sslkey = generic::shell_exists("~/.postgresql/postgresql.key");
        let sslrootcert = generic::shell_exists("~/.postgresql/root.crt");
        assert_eq!(
            d.to_string(),
            format!(
                concat!(
                    "dbname='{0}' ",
                    "host='{4}' ",
                    "password='' ",
                    "port='5432' ",
                    "sslcert='{1}' ",
                    "sslkey='{2}' ",
                    "sslmode='prefer' ",
                    "sslrootcert='{3}' ",
                    "user='{0}'"
                ),
                os_user_name(),
                sslcert,
                sslkey,
                sslrootcert,
                default_socket_dir(&SOCKET_DIRS, "5432")
            )
        );
        // And reset them to the value they had before runnignt his test
        for (key, value) in std::env::vars() {
            print!("{}={}", key, value);
            if envvars.contains_key(key.as_str()) {
                std::env::set_var(key, value);
            }
        }
    }

    #[test]
    fn test_default_socket_dir() {
        let base = std::env::temp_dir().join(format!("pg_tps_socket_dirs_{}", std::process::id()));
        let (first, second) = (base.join("first"), base.join("second"));
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let dirs = [first.to_str().unwrap(), second.to_str().unwrap()];

        // No socket anywhere: the last directory
        assert_eq!(default_socket_dir(&dirs, "5432"), dirs[1]);
        // The first directory with a socket for the port
        fs::write(second.join(".s.PGSQL.5432"), "").unwrap();
        assert_eq!(default_socket_dir(&dirs, "5432"), dirs[1]);
        fs::write(first.join(".s.PGSQL.5433"), "").unwrap();
        assert_eq!(default_socket_dir(&dirs, "5433"), dirs[0]);
        assert_eq!(default_socket_dir(&dirs, "5432"), dirs[1]);

        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn test_system_ca_locations() {
        let base = std::env::temp_dir().join(format!("pg_tps_ca_{}", std::process::id()));
        let (dir1, dir2) = (base.join("dir1"), base.join("dir2"));
        fs::create_dir_all(&dir1).unwrap();
        fs::create_dir_all(&dir2).unwrap();
        let file = base.join("ca.crt");
        fs::write(&file, "").unwrap();
        let os = |p: &Path| Some(p.as_os_str().to_os_string());

        // Only the configured file, no OS directories
        assert_eq!(
            system_ca_locations(os(&file), None).unwrap(),
            CaLocations {
                files: vec![file.clone()],
                dirs: vec![]
            }
        );
        // A list of directories
        let list = std::env::join_paths([&dir1, &dir2]).unwrap();
        assert_eq!(
            system_ca_locations(None, Some(list)).unwrap(),
            CaLocations {
                files: vec![],
                dirs: vec![dir1.clone(), dir2.clone()]
            }
        );
        // Configured paths must exist
        let err = system_ca_locations(os(&base.join("missing.crt")), None).unwrap_err();
        assert!(err.starts_with("SSL_CERT_FILE"), "{}", err);
        let list = std::env::join_paths([&dir1, &base.join("missing")]).unwrap();
        let err = system_ca_locations(None, Some(list)).unwrap_err();
        assert!(
            err.starts_with("SSL_CERT_DIR") && err.contains("missing"),
            "{}",
            err
        );
        // Empty values count as not set
        assert_eq!(
            system_ca_locations(Some(OsString::new()), Some(OsString::new())).is_ok(),
            system_ca_locations(None, None).is_ok()
        );

        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn test_invalid_connection_string() {
        let err = Dsn::from_string("host").client().err().unwrap().to_string();
        assert!(
            err.contains("invalid connection string: missing \"=\" after \"host\""),
            "{}",
            err
        );

        let err = Dsn::from_string("host=/tmp unknown_key=value")
            .client()
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("unknown option `unknown_key`"), "{}", err);
    }

    #[test]
    fn test_client_certificate_needs_key_and_cert() {
        let err = Dsn::from_string("host=localhost sslmode=require sslcert='' sslkey=/my.key")
            .client()
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("sslkey is set"), "{}", err);

        let err = Dsn::from_string("host=localhost sslmode=require sslcert=/my.crt sslkey=''")
            .client()
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("sslcert is set"), "{}", err);
    }

    #[test]
    #[ignore]
    fn test_dsn_client() -> Result<(), Error> {
        let constr = generic::get_env_str("", "TEST_CONNSTR", "").to_string();
        if constr.is_empty() {
            return Ok(());
        }
        let dsn = Dsn::from_string(constr.as_str());
        let mut client = dsn.client().unwrap();
        let query = "select oid, datname from pg_database";
        println!("query: {}", query);
        for row in &client.query(query, &[])? {
            let id: u32 = row.get(0);
            let name: &str = row.get(1);
            println!("oid: {}, name: {}", id, name);
        }
        Ok(())
    }
}
