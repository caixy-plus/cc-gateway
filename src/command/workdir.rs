use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::t_fmt;

pub(crate) fn effective_work_dir(current_dir: &str, default_dir: &str) -> String {
    if current_dir.is_empty() {
        shellexpand::tilde(default_dir).to_string()
    } else {
        shellexpand::tilde(current_dir).to_string()
    }
}

pub(crate) fn resolve_work_dir_target(
    current_dir: &str,
    default_dir: &str,
    requested: &Path,
) -> Result<String> {
    let expanded = shellexpand::tilde(&requested.to_string_lossy()).to_string();
    let requested = PathBuf::from(expanded);
    let base = PathBuf::from(effective_work_dir(current_dir, default_dir));
    let target = if requested.is_absolute() {
        requested
    } else {
        base.join(requested)
    };
    let canonical = target.canonicalize().unwrap_or(target);

    if !canonical.is_dir() {
        anyhow::bail!(
            "{}",
            t_fmt!("builtin.invalid_path", PATH = canonical.display())
        );
    }

    crate::claude::controller::ensure_under_home(&canonical.to_string_lossy())
}
