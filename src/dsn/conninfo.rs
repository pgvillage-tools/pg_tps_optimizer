// Parses a connection string in both formats libpq supports:
// https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNSTRING
//
// - keyword/value: `host=localhost dbname='my db'`
// - URI: `postgresql://user:password@host1:5432,host2/dbname?sslmode=require`
//
// The result is a list of (key, value) pairs, in the order they were found.

pub fn parse(conninfo: &str) -> Result<Vec<(String, String)>, String> {
    let conninfo = conninfo.trim();
    for scheme in ["postgresql://", "postgres://"] {
        if let Some(rest) = conninfo.strip_prefix(scheme) {
            return parse_uri(rest);
        }
    }
    parse_keyword_value(conninfo)
}

fn parse_keyword_value(conninfo: &str) -> Result<Vec<(String, String)>, String> {
    let mut pairs = Vec::new();
    let mut chars = conninfo.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        if chars.peek().is_none() {
            return Ok(pairs);
        }

        let mut key = String::new();
        while let Some(c) = chars.next_if(|c| *c != '=' && !c.is_whitespace()) {
            key.push(c);
        }
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        if chars.next() != Some('=') {
            return Err(format!("missing \"=\" after \"{}\"", key));
        }
        while chars.next_if(|c| c.is_whitespace()).is_some() {}

        // A value is either quoted, or runs until the next whitespace. In both, a backslash
        // escapes the next character.
        let mut value = String::new();
        if chars.next_if_eq(&'\'').is_some() {
            loop {
                match chars.next() {
                    Some('\'') => break,
                    Some('\\') => value.extend(chars.next()),
                    Some(c) => value.push(c),
                    None => return Err(format!("unterminated quoted string for \"{}\"", key)),
                }
            }
        } else {
            while let Some(c) = chars.next_if(|c| !c.is_whitespace()) {
                match c {
                    '\\' => value.extend(chars.next()),
                    c => value.push(c),
                }
            }
        }
        pairs.push((key, value));
    }
}

// The part after the scheme: [user[:password]@][host[:port][,...]][/dbname][?key=value[&...]]
fn parse_uri(uri: &str) -> Result<Vec<(String, String)>, String> {
    let mut pairs = Vec::new();
    let (rest, query) = match uri.split_once('?') {
        Some((rest, query)) => (rest, Some(query)),
        None => (uri, None),
    };
    let (authority, dbname) = match rest.split_once('/') {
        Some((authority, dbname)) => (authority, Some(dbname)),
        None => (rest, None),
    };

    let hostspecs = match authority.split_once('@') {
        Some((userinfo, hostspecs)) => {
            let (user, password) = match userinfo.split_once(':') {
                Some((user, password)) => (user, Some(password)),
                None => (userinfo, None),
            };
            if !user.is_empty() {
                pairs.push(("user".to_string(), percent_decode(user)?));
            }
            if let Some(password) = password {
                pairs.push(("password".to_string(), percent_decode(password)?));
            }
            hostspecs
        }
        None => authority,
    };

    if !hostspecs.is_empty() {
        let mut hosts = Vec::new();
        let mut ports = Vec::new();
        for hostspec in hostspecs.split(',') {
            let (host, port) = split_hostspec(hostspec)?;
            hosts.push(percent_decode(host)?);
            ports.push(percent_decode(port)?);
        }
        if hosts.iter().any(|h| !h.is_empty()) {
            pairs.push(("host".to_string(), hosts.join(",")));
        }
        if ports.iter().any(|p| !p.is_empty()) {
            pairs.push(("port".to_string(), ports.join(",")));
        }
    }

    if let Some(dbname) = dbname.filter(|d| !d.is_empty()) {
        pairs.push(("dbname".to_string(), percent_decode(dbname)?));
    }

    for param in query.unwrap_or("").split('&').filter(|p| !p.is_empty()) {
        let (key, value) = param
            .split_once('=')
            .ok_or_else(|| format!("missing \"=\" in URI query parameter \"{}\"", param))?;
        pairs.push((percent_decode(key)?, percent_decode(value)?));
    }
    Ok(pairs)
}

// host[:port], where host can be an IPv6 address in brackets: [::1]:5432
fn split_hostspec(hostspec: &str) -> Result<(&str, &str), String> {
    if let Some(rest) = hostspec.strip_prefix('[') {
        let (host, rest) = rest
            .split_once(']')
            .ok_or_else(|| format!("missing \"]\" in IPv6 host address \"{}\"", hostspec))?;
        return match rest {
            "" => Ok((host, "")),
            _ => match rest.strip_prefix(':') {
                Some(port) => Ok((host, port)),
                None => Err(format!("unexpected \"{}\" after IPv6 host address", rest)),
            },
        };
    }
    Ok(hostspec.split_once(':').unwrap_or((hostspec, "")))
}

fn percent_decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let byte = value
                .get(i + 1..i + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                .ok_or_else(|| format!("invalid percent-encoded token in \"{}\"", value))?;
            decoded.push(byte);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(decoded)
        .map_err(|_| format!("invalid percent-encoded token in \"{}\"", value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_keyword_value() {
        assert_eq!(parse("").unwrap(), pairs(&[]));
        assert_eq!(
            parse("host=localhost port=5432").unwrap(),
            pairs(&[("host", "localhost"), ("port", "5432")])
        );
        // Whitespace around =, quoted values with spaces and escapes, empty values
        assert_eq!(
            parse("  dbname = 'my db'  application_name='it\\'s' password='' user=a\\ b ").unwrap(),
            pairs(&[
                ("dbname", "my db"),
                ("application_name", "it's"),
                ("password", ""),
                ("user", "a b"),
            ])
        );
        assert_eq!(
            parse("options='-c search_path=x'").unwrap(),
            pairs(&[("options", "-c search_path=x")])
        );
    }

    #[test]
    fn test_keyword_value_errors() {
        assert_eq!(parse("host").unwrap_err(), "missing \"=\" after \"host\"");
        assert_eq!(
            parse("dbname='postgres").unwrap_err(),
            "unterminated quoted string for \"dbname\""
        );
    }

    #[test]
    fn test_uri() {
        assert_eq!(parse("postgresql://").unwrap(), pairs(&[]));
        assert_eq!(
            parse("postgres://localhost").unwrap(),
            pairs(&[("host", "localhost")])
        );
        assert_eq!(
            parse("postgresql://user:se%40cret@db1:5433/my%20db?sslmode=verify-full&application_name=tps")
                .unwrap(),
            pairs(&[
                ("user", "user"),
                ("password", "se@cret"),
                ("host", "db1"),
                ("port", "5433"),
                ("dbname", "my db"),
                ("sslmode", "verify-full"),
                ("application_name", "tps"),
            ])
        );
        // Multiple hosts, IPv6 and unix socket directories
        assert_eq!(
            parse("postgresql://db1:5432,[::1]:5433,[fe80::1]/postgres").unwrap(),
            pairs(&[
                ("host", "db1,::1,fe80::1"),
                ("port", "5432,5433,"),
                ("dbname", "postgres"),
            ])
        );
        assert_eq!(
            parse("postgresql://%2Fvar%2Frun%2Fpostgresql/postgres").unwrap(),
            pairs(&[("host", "/var/run/postgresql"), ("dbname", "postgres")])
        );
        assert_eq!(
            parse("postgresql:///postgres?host=/tmp&port=5433").unwrap(),
            pairs(&[("dbname", "postgres"), ("host", "/tmp"), ("port", "5433")])
        );
    }

    #[test]
    fn test_uri_errors() {
        assert_eq!(
            parse("postgresql://[::1/postgres").unwrap_err(),
            "missing \"]\" in IPv6 host address \"[::1\""
        );
        assert_eq!(
            parse("postgresql://localhost/postgres?sslmode").unwrap_err(),
            "missing \"=\" in URI query parameter \"sslmode\""
        );
        assert_eq!(
            parse("postgresql://localhost/my%zzdb").unwrap_err(),
            "invalid percent-encoded token in \"my%zzdb\""
        );
    }
}
