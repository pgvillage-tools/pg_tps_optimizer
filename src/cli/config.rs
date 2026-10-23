// Reads a YAML config file, with a value per option. The values become the defaults of the
// options, so the command line and environment variables still win.
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Value {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    List(Vec<Value>),
}

impl Value {
    fn to_strings(&self) -> Result<Vec<String>, String> {
        match self {
            Value::Bool(b) => Ok(vec![b.to_string()]),
            Value::Integer(i) => Ok(vec![i.to_string()]),
            Value::Float(f) => Ok(vec![f.to_string()]),
            Value::String(s) => Ok(vec![s.clone()]),
            Value::List(values) => values
                .iter()
                .map(|value| match value {
                    Value::List(_) => Err("lists in lists are not supported".to_string()),
                    value => Ok(value.to_strings()?.remove(0)),
                })
                .collect(),
        }
    }
}

// The values of the config file per option, with the option name as in the long option, like
// `min_samples` (or `min-samples`)
pub fn read(path: &Path) -> Result<Vec<(String, Vec<String>)>, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("could not read config file \"{}\": {}", path.display(), e))?;
    parse(&content).map_err(|e| format!("invalid config file \"{}\": {}", path.display(), e))
}

fn parse(content: &str) -> Result<Vec<(String, Vec<String>)>, String> {
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    let settings: BTreeMap<String, Value> =
        serde_saphyr::from_str(content).map_err(|e| e.to_string())?;
    settings
        .iter()
        .map(|(key, value)| {
            let values = value
                .to_strings()
                .map_err(|e| format!("\"{}\": {}", key, e))?;
            Ok((key.replace('-', "_"), values))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(list: &[&str]) -> Vec<String> {
        list.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn test_parse() {
        let config = parse(
            "
# A comment
dsn: host=db1.example.com dbname=postgres
range: 1:200
min-samples: 20
spread: 5.5
prepared: true
graph_input: [a.json, b.json]
",
        )
        .unwrap();
        assert_eq!(
            config,
            vec![
                (
                    "dsn".to_string(),
                    values(&["host=db1.example.com dbname=postgres"])
                ),
                ("graph_input".to_string(), values(&["a.json", "b.json"])),
                ("min_samples".to_string(), values(&["20"])),
                ("prepared".to_string(), values(&["true"])),
                ("range".to_string(), values(&["1:200"])),
                ("spread".to_string(), values(&["5.5"])),
            ]
        );
        assert_eq!(parse("").unwrap(), vec![]);
    }

    #[test]
    fn test_parse_errors() {
        assert!(parse("not: [valid").is_err());
        assert!(parse("- a list\n- instead of options\n").is_err());
    }
}
