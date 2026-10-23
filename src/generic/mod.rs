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

pub fn get_env_f64(val: f64, env_key: &str, default: f64) -> f64 {
    if val != 0.0_f64 {
        return val;
    }
    match env::var(env_key) {
        Ok(env_val) => match env_val.parse::<f64>() {
            Ok(f) => f,
            Err(_) => default,
        },
        Err(_e) => default,
    }
}

pub fn get_env_u32(val: u32, env_key: &str, default: u32) -> u32 {
    if val != 0 {
        return val;
    }
    match env::var(env_key) {
        Ok(env_val) => match env_val.parse::<u32>() {
            Ok(u) => u,
            Err(_) => default,
        },
        Err(_e) => default,
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

pub fn get_env_bool(val: bool, env_key: &str) -> bool {
    if val {
        return val;
    }
    if env::var(env_key).is_ok() {
        return true;
    }
    false
}
/*
pub fn get_env_int(val: u32, env_key: &str, default: u32) -> u32 {
    if val > 0 {
        return val;
    }
    if let Ok(env_val) = env::var(env_key) {
        if let Ok(env_int_val) = env_val.parse::<u32>() {
            return env_int_val;
        }
    }
    default
}

fn get_bool_default(val: bool, env_key: String) -> bool {
    if val {
        return val;
    }
    if let Ok(mut env_val) = env::var(env_key) {
        env_val.make_ascii_lowercase();
        if let Ok(env_bool_val) = env_val.parse::<bool>() {
            return env_bool_val;
        }
    }
    false
}
*/
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
    #[test]
    fn test_get_env_bool() {
        const TEST_VAR: &str = "TEST_VAR_BOOL";
        const TEST_VAL: &str = "is set";
        env::set_var(TEST_VAR, TEST_VAL);
        for val in [true, false] {
            assert!(get_env_bool(val, TEST_VAR));
        }
        env::remove_var(TEST_VAR);
        for val in [true, false] {
            assert_eq!(get_env_bool(val, TEST_VAR), val);
        }
    }
}
