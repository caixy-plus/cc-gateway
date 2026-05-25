use crate::utils::env::substitute_env_vars;
use std::env;

#[test]
fn test_substitute_env_vars() {
    env::set_var("TEST_VAR", "hello");
    assert_eq!(substitute_env_vars("${TEST_VAR}"), "hello");
    assert_eq!(substitute_env_vars("${NONEXISTENT:default}"), "default");
    assert_eq!(
        substitute_env_vars("prefix-${TEST_VAR}-suffix"),
        "prefix-hello-suffix"
    );
}
