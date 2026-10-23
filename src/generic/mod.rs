use std::env;

pub fn get_env_str(val: &str, env_key: &str, default: &str) -> String {
    if !val.is_empty() {
        return val.to_string();
    }
    match env::var(env_key) {
        Ok(env_val) => env_val,
        Err(_e) => default.to_string(),
    }
}

fn shell_expand(path: &str) -> String {
    shellexpand::tilde(path).to_string()
}

pub fn shell_exists(path: &str) -> String {
    let path = shellexpand::tilde(path).to_string();
    if std::path::Path::new(path.as_str()).exists() {
        return path;
    }
    "".to_string()
}

pub fn get_env_path(val: &str, env_key: &str, default: &str) -> String {
    if !val.is_empty() {
        return shell_expand(val).to_string();
    }
    match env::var(env_key) {
        Ok(env_val) => shell_expand(env_val.as_str()),
        Err(_e) => shell_exists(default).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_env_str() {
        const TEST_ISSET: &str = "is set";
        const TEST_VAR: &str = "TEST_VAR_STR";
        const TEST_VAL: &str = "from env";
        const TEST_DEFAULT: &str = "default";
        env::set_var(TEST_VAR, TEST_VAL);
        assert_eq!(get_env_str("", TEST_VAR, ""), TEST_VAL);
        assert_eq!(get_env_str(TEST_ISSET, TEST_VAR, ""), TEST_ISSET);
        assert_eq!(get_env_str("", TEST_VAR, TEST_DEFAULT), TEST_VAL);
        env::remove_var(TEST_VAR);
        assert_eq!(get_env_str("", TEST_VAR, ""), "");
        assert_eq!(get_env_str(TEST_ISSET, TEST_VAR, ""), TEST_ISSET);
        assert_eq!(get_env_str("", TEST_VAR, TEST_DEFAULT), TEST_DEFAULT);
    }
}
