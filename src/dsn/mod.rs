use crate::generic;
use openssl::ssl::{SslConnector, SslFiletype, SslMethod};
use postgres::{Client, NoTls};
use postgres_openssl::MakeTlsConnector;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::fmt;
use uzers::{get_current_uid, get_user_by_uid};

#[derive(Debug, Clone)]
pub struct Dsn {
    kv: HashMap<String, String>,
    ssl_mode: String,
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
        let split = from.split(' ');
        for s in split {
            if let Some((key, value)) = s.split_once('=') {
                dsn.set_value(key, value)
            }
        }
        dsn
    }
    pub fn copy(&self) -> Dsn {
        let mut kv: HashMap<String, String> = HashMap::new();
        for (k, v) in self.kv.borrow() {
            kv.insert(k.to_string(), v.to_string());
        }
        Dsn {
            kv,
            ssl_mode: self.ssl_mode.to_string(),
        }
    }
    pub fn cleanse(&self) -> Dsn {
        let mut kv: HashMap<String, String> = HashMap::new();
        kv.extend(self.clone().kv);
        kv.remove("sslmode");
        kv.remove("sslcert");
        kv.remove("sslkey");
        kv.remove("sslrootcert");
        kv.remove("sslcrl");
        let ssl_mode = "disable".to_string();
        Dsn { kv, ssl_mode }
    }
    pub fn new() -> Dsn {
        let mut kv: HashMap<String, String> = HashMap::new();

        kv.insert("user".to_string(), os_user_name());
        kv.insert(
            "dbname".to_string(),
            generic::get_env_str("", "PGDATABASE", os_user_name().as_str()),
        );
        kv.insert(
            "port".to_string(),
            generic::get_env_str("", "PGPORT", "5432"),
        );
        kv.insert(
            "host".to_string(),
            generic::get_env_str("", "PGHOST", "/tmp"),
        );
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
        kv.insert(
            "sslcrl".to_string(),
            generic::get_env_path("", "PGSSLCRL", "~/.postgresql/root.crl"),
        );
        Dsn { kv, ssl_mode }
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
            self.ssl_mode = value.to_string()
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
    pub fn use_tls(&self) -> bool {
        self.ssl_mode.ne("disable")
    }
    pub fn verify_hostname(&self) -> bool {
        self.ssl_mode.eq("verify-full")
    }
    pub fn client(self) -> Result<Client, Box<dyn std::error::Error>> {
        let copy = self.cleanse().to_string();
        let conn_string = copy.as_str();
        let cert_file = self.get_value("sslcert", "");
        if !self.copy().use_tls() || cert_file.is_empty() {
            let client = postgres::Client::connect(conn_string, NoTls)?;
            return Ok(client);
            // The source_connection object performs the actual communication
            // with the database, so spawn it off to run on its own.
        }
        let mut builder = SslConnector::builder(SslMethod::tls())?;
        if let Err(error) = builder.set_certificate_chain_file(cert_file) {
            eprintln!("set_certificate_file: {}", error);
        }
        let private_key = self.get_value("sslkey", "~/.postgresql/postgresql.key");
        if let Err(error) = builder.set_private_key_file(private_key, SslFiletype::PEM) {
            eprintln!("set_client_key_file: {}", error);
        }
        let root_cert = self.get_value("sslrootcert", "~/.postgresql/root.crt");
        if let Err(error) = builder.set_ca_file(root_cert) {
            eprintln!("set_ca_file: {}", error);
        }

        let mut connector = MakeTlsConnector::new(builder.build());
        connector.set_callback(move |config, _| {
            config.set_verify_hostname(self.verify_hostname());
            Ok(())
        });
        let client = postgres::Client::connect(conn_string, connector)?;
        Ok(client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use postgres::Error;

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
        envvars.insert("PGSSLCRL", "~/crl");
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
            "sslcrl='~/crl' ",
            "sslkey='~/key' ",
            "sslmode='verify-full' ",
            "sslrootcert='~/root' ",
            "user='me'",
        );
        assert_eq!(d.to_string(), expected.replace("~", home_dir.as_str()),);
        // and unset them
        for (key, _) in envvars.iter() {
            std::env::remove_var(key);
        }
        // And test without them being set
        d = Dsn::new();
        assert!(d.use_tls());
        assert_eq!(
            d.cleanse().to_string(),
            format!(
                "dbname='{0}' host='/tmp' password='' port='5432' user='{0}'",
                os_user_name()
            )
        );
        let sslcert = generic::shell_exists("~/.postgresql/postgresql.crt");
        let sslcrl = generic::shell_exists("~/.postgresql/root.crl");
        let sslkey = generic::shell_exists("~/.postgresql/postgresql.key");
        let sslrootcert = generic::shell_exists("~/.postgresql/root.crt");
        assert_eq!(
            d.to_string(),
            format!(
                concat!(
                    "dbname='{0}' ",
                    "host='/tmp' ",
                    "password='' ",
                    "port='5432' ",
                    "sslcert='{1}' ",
                    "sslcrl='{2}' ",
                    "sslkey='{3}' ",
                    "sslmode='prefer' ",
                    "sslrootcert='{4}' ",
                    "user='{0}'"
                ),
                os_user_name(),
                sslcert,
                sslcrl,
                sslkey,
                sslrootcert
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
