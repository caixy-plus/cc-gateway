use crate::daemon::is_process_alive;

#[test]
fn test_is_process_alive_current_pid() {
    let current_pid = std::process::id();
    assert!(
        is_process_alive(current_pid),
        "is_process_alive should return true for the current process"
    );
}

#[test]
fn test_is_process_alive_nonexistent_pid() {
    assert!(
        !is_process_alive(999_999),
        "is_process_alive should return false for a non-existent PID"
    );
}
