use regex::Regex;
use std::env;

pub fn substitute_env_vars(input: &str) -> String {
    let re = Regex::new(r"\$\{(\w+)(?::([^}]*))?\}").unwrap();
    re.replace_all(input, |caps: &regex::Captures| {
        let var_name = &caps[1];
        let default_value = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        env::var(var_name).unwrap_or_else(|_| default_value.to_string())
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_substitute_env_vars() {
        env::set_var("TEST_VAR", "hello");
        assert_eq!(
            substitute_env_vars("${TEST_VAR}"),
            "hello"
        );
        assert_eq!(
            substitute_env_vars("${NONEXISTENT:default}"),
            "default"
        );
        assert_eq!(
            substitute_env_vars("prefix-${TEST_VAR}-suffix"),
            "prefix-hello-suffix"
        );
    }
}
