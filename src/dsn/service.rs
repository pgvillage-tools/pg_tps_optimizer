// Looks up a service in the connection service files, like libpq:
// https://www.postgresql.org/docs/current/libpq-pgservice.html

use std::fs;
use std::path::{Path, PathBuf};

// System service file directories when PGSYSCONFDIR is not set. libpq uses the directory it was
// built with, which differs per distribution: Debian/Ubuntu, then RHEL (PGDG packages).
const SYSCONF_DIRS: [&str; 2] = ["/etc/postgresql-common", "/etc/sysconfig/pgsql"];

// The service files to search, in order: the user file, then the system file
pub fn service_files() -> Vec<PathBuf> {
    let mut files = vec![match std::env::var("PGSERVICEFILE") {
        Ok(file) => PathBuf::from(shellexpand::tilde(&file).to_string()),
        Err(_) => PathBuf::from(shellexpand::tilde("~/.pg_service.conf").to_string()),
    }];
    match std::env::var("PGSYSCONFDIR") {
        Ok(dir) => files.push(Path::new(&dir).join("pg_service.conf")),
        Err(_) => files.extend(
            SYSCONF_DIRS
                .iter()
                .map(|dir| Path::new(dir).join("pg_service.conf")),
        ),
    }
    files
}

// The settings of a service, from the first file that defines it. Files that don't exist are
// skipped.
pub fn lookup(name: &str, files: &[PathBuf]) -> Result<Vec<(String, String)>, String> {
    for file in files.iter().filter(|file| file.exists()) {
        let content = fs::read_to_string(file)
            .map_err(|e| format!("could not read service file \"{}\": {}", file.display(), e))?;
        if let Some(settings) = find_service(&content, name, file)? {
            return Ok(settings);
        }
    }
    Err(format!("definition of service \"{}\" not found", name))
}

fn find_service(
    content: &str,
    name: &str,
    file: &Path,
) -> Result<Option<Vec<(String, String)>>, String> {
    let mut settings: Option<Vec<(String, String)>> = None;
    for (number, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[') {
            // A new section ends the service we were reading
            if settings.is_some() {
                break;
            }
            if section.strip_suffix(']') == Some(name) {
                settings = Some(Vec::new());
            }
            continue;
        }
        if let Some(settings) = settings.as_mut() {
            let syntax_error = || {
                format!(
                    "syntax error in service file \"{}\", line {}",
                    file.display(),
                    number + 1
                )
            };
            let (key, value) = line.split_once('=').ok_or_else(syntax_error)?;
            let key = key.trim();
            if key.is_empty() {
                return Err(syntax_error());
            }
            if key == "service" {
                return Err(format!(
                    "nested service specifications not supported in service file \"{}\", line {}",
                    file.display(),
                    number + 1
                ));
            }
            settings.push((key.to_string(), value.trim().to_string()));
        }
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVICES: &str = "
# A comment
[bench]
host=db1.example.com
port = 5433
dbname=bench db
application_name=a=b

[other]
host=db2.example.com
";

    fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let file = dir.join(name);
        fs::write(&file, content).unwrap();
        file
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_lookup() {
        let dir = std::env::temp_dir().join(format!("pg_tps_service_{}", std::process::id()));
        let user = write_file(&dir, "user.conf", SERVICES);
        let system = write_file(&dir, "system.conf", "[system]\nhost=db3.example.com\n");
        let files = [dir.join("missing.conf"), user, system];

        assert_eq!(
            lookup("bench", &files).unwrap(),
            pairs(&[
                ("host", "db1.example.com"),
                ("port", "5433"),
                ("dbname", "bench db"),
                ("application_name", "a=b"),
            ])
        );
        assert_eq!(
            lookup("other", &files).unwrap(),
            pairs(&[("host", "db2.example.com")])
        );
        // Found in the second file
        assert_eq!(
            lookup("system", &files).unwrap(),
            pairs(&[("host", "db3.example.com")])
        );
        assert_eq!(
            lookup("nope", &files).unwrap_err(),
            "definition of service \"nope\" not found"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_lookup_errors() {
        let dir = std::env::temp_dir().join(format!("pg_tps_service_err_{}", std::process::id()));
        let file = write_file(&dir, "bad.conf", "[bench]\nhost\n");
        let err = lookup("bench", std::slice::from_ref(&file)).unwrap_err();
        assert!(
            err.contains("syntax error in service file") && err.ends_with("line 2"),
            "{}",
            err
        );

        let file = write_file(&dir, "nested.conf", "[bench]\nservice=other\n");
        let err = lookup("bench", &[file]).unwrap_err();
        assert!(err.contains("nested service specifications"), "{}", err);

        fs::remove_dir_all(&dir).unwrap();
    }
}
