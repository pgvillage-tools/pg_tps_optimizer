// Looks up a password in a password file (~/.pgpass), like libpq:
// https://www.postgresql.org/docs/current/libpq-pgpass.html

use std::fs;
use std::path::{Path, PathBuf};

// The password file: passfile from the connection string, PGPASSFILE, or ~/.pgpass
pub fn passfile(from_connection_string: Option<&str>) -> PathBuf {
    let file = match from_connection_string {
        Some(file) => file.to_string(),
        None => std::env::var("PGPASSFILE").unwrap_or_else(|_| "~/.pgpass".to_string()),
    };
    PathBuf::from(shellexpand::tilde(&file).to_string())
}

pub struct Target<'a> {
    pub host: &'a str,
    pub port: &'a str,
    pub dbname: &'a str,
    pub user: &'a str,
}

// The password of the first line that matches the connection, if any. Like libpq, a file that
// others can read is ignored (with a warning), and a file that can't be read is skipped.
pub fn lookup(file: &Path, target: &Target) -> Option<String> {
    if !file.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(file) {
            if metadata.permissions().mode() & 0o077 != 0 {
                eprintln!(
                    "warning: password file \"{}\" has group or world access; permissions \
                     should be u=rw (0600) or less",
                    file.display()
                );
                return None;
            }
        }
    }
    let content = fs::read_to_string(file).ok()?;
    find_password(&content, target)
}

fn find_password(content: &str, target: &Target) -> Option<String> {
    // A unix socket matches the host name localhost
    let host = match target.host.starts_with('/') {
        true => "localhost",
        false => target.host,
    };
    let wanted = [host, target.port, target.dbname, target.user];
    for line in content.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = split_fields(line);
        if fields.len() < 5 {
            continue;
        }
        let matches = wanted
            .iter()
            .zip(&fields)
            .all(|(wanted, field)| field == "*" || field == wanted);
        if matches {
            return Some(fields[4].clone());
        }
    }
    None
}

// Splits host:port:database:user:password; a backslash escapes the next character (like : or \)
fn split_fields(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => fields.last_mut().unwrap().extend(chars.next()),
            // The password is the rest of the line
            ':' if fields.len() < 5 => fields.push(String::new()),
            c => fields.last_mut().unwrap().push(c),
        }
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    const PGPASS: &str = "# comment
db1.example.com:5432:bench:postgres:secret1
db1.example.com:*:*:postgres:secret2
*:5433:*:*:p\\:a\\\\ss:word
localhost:5432:postgres:postgres:local
incomplete:line
";

    fn find(host: &str, port: &str, dbname: &str, user: &str) -> Option<String> {
        find_password(
            PGPASS,
            &Target {
                host,
                port,
                dbname,
                user,
            },
        )
    }

    #[test]
    fn test_find_password() {
        // The first matching line wins
        assert_eq!(
            find("db1.example.com", "5432", "bench", "postgres"),
            Some("secret1".to_string())
        );
        assert_eq!(
            find("db1.example.com", "5432", "other", "postgres"),
            Some("secret2".to_string())
        );
        // Escaped : and \ in the password, and : in the rest of the line
        assert_eq!(
            find("db2.example.com", "5433", "x", "y"),
            Some("p:a\\ss:word".to_string())
        );
        // A unix socket matches localhost
        assert_eq!(
            find("/var/run/postgresql", "5432", "postgres", "postgres"),
            Some("local".to_string())
        );
        assert_eq!(find("db2.example.com", "5432", "bench", "postgres"), None);
    }

    #[test]
    fn test_lookup_file() {
        let dir = std::env::temp_dir().join(format!("pg_tps_pgpass_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("pgpass");
        fs::write(&file, "*:*:*:*:secret\n").unwrap();
        let target = Target {
            host: "db",
            port: "5432",
            dbname: "x",
            user: "y",
        };
        assert_eq!(lookup(&dir.join("missing"), &target), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Readable for others: ignored
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(lookup(&file, &target), None);
            fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert_eq!(lookup(&file, &target), Some("secret".to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }
}
