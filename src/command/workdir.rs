use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::t_fmt;

fn expand_tilde(path: &str) -> String {
    let is_home_relative =
        path == "~" || path.starts_with("~/") || cfg!(windows) && path.starts_with(r"~\");
    if is_home_relative {
        let home = std::env::var_os("HOME").filter(|h| !h.is_empty());
        #[cfg(windows)]
        let home = home.or_else(|| std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()));

        if let Some(home) = home {
            let home = PathBuf::from(home);
            if path == "~" {
                return home.to_string_lossy().to_string();
            }
            return home
                .join(path.trim_start_matches("~/").trim_start_matches(r"~\"))
                .to_string_lossy()
                .to_string();
        }
    }
    shellexpand::tilde(path).to_string()
}

pub(crate) fn effective_work_dir(current_dir: &str, default_dir: &str) -> String {
    if current_dir.is_empty() {
        expand_tilde(default_dir)
    } else {
        expand_tilde(current_dir)
    }
}

pub(crate) fn resolve_work_dir_target(
    current_dir: &str,
    default_dir: &str,
    requested: &Path,
) -> Result<String> {
    let expanded = expand_tilde(&requested.to_string_lossy());
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
