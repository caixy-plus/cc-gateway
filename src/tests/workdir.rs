use std::path::Path;

use super::helpers::TestEnv;

#[test]
fn resolves_relative_target_from_current_work_dir() {
    let env = TestEnv::new();
    let root = env.home().join("project");
    let child = root.join("child");
    std::fs::create_dir_all(&child).unwrap();

    let resolved = crate::command::workdir::resolve_work_dir_target(
        child.to_str().unwrap(),
        env.home().to_str().unwrap(),
        Path::new(".."),
    )
    .unwrap();

    assert_eq!(resolved, root.canonicalize().unwrap().to_string_lossy());
}

#[test]
fn rejects_target_outside_home() {
    let env = TestEnv::new();
    let outside = std::env::current_dir().unwrap();

    let err = crate::command::workdir::resolve_work_dir_target(
        env.home().to_str().unwrap(),
        env.home().to_str().unwrap(),
        outside.as_path(),
    )
    .unwrap_err();

    assert!(err.to_string().contains("Access denied"));
}

#[test]
fn expands_tilde_current_work_dir_before_resolving_relative_target() {
    let env = TestEnv::new();
    let project = env.home().join("project");
    std::fs::create_dir_all(&project).unwrap();

    let resolved = crate::command::workdir::resolve_work_dir_target(
        "~",
        env.home().to_str().unwrap(),
        Path::new("project"),
    )
    .unwrap();

    assert_eq!(resolved, project.canonicalize().unwrap().to_string_lossy());
}
