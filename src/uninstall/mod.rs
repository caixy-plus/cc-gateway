use anyhow::{Context, Result};
use std::io::Write;
use std::process::Stdio;

use crate::{t, t_fmt};

const UNINSTALL_SH_URL: &str =
    "https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/uninstall.sh";
#[cfg(windows)]
const UNINSTALL_PS1_URL: &str =
    "https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/uninstall.ps1";

/// `cc-gateway uninstall`: the binary only handles the confirmation prompt and
/// then hands off ALL cleanup to `uninstall.sh` / `uninstall.ps1`. The
/// `--keep-data` choice is passed through to the script untouched.
pub fn run(yes: bool, keep_data: bool) -> Result<()> {
    print_plan(keep_data);

    if !yes && !confirm()? {
        println!("{}", t!("uninstall.cancelled"));
        return Ok(());
    }

    run_platform(keep_data)
}

fn print_plan(keep_data: bool) {
    let bin = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "cc-gateway".to_string());

    println!("{}", t!("uninstall.plan_title"));
    println!("{}", t!("uninstall.plan_stop"));
    println!("{}", t!("uninstall.plan_autostart"));
    println!("{}", t_fmt!("uninstall.plan_binary", PATH = bin));
    println!("{}", t!("uninstall.plan_path_entry"));
    if keep_data {
        println!("{}", t!("uninstall.plan_data_keep"));
    } else {
        println!("{}", t!("uninstall.plan_data_delete"));
    }
}

fn confirm() -> Result<bool> {
    print!("{} ", t!("uninstall.confirm_prompt"));
    std::io::stdout().flush().ok();
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .context("failed to read confirmation")?;
    let answer = input.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

#[cfg(unix)]
fn run_platform(keep_data: bool) -> Result<()> {
    // On Unix the running executable can be unlinked while running, so we run the
    // cleanup script attached in the current terminal for visible progress.
    let flag = if keep_data { "--keep-data" } else { "" };
    let cmd = format!(
        "curl -fsSL {} | sh -s -- {}",
        shell_single_quote(UNINSTALL_SH_URL),
        flag
    );
    println!("{}", t!("uninstall.running"));
    let status = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to run uninstall script")?;

    if !status.success() {
        anyhow::bail!(
            "uninstall script failed with exit code: {:?}",
            status.code()
        );
    }
    Ok(())
}

#[cfg(windows)]
fn run_platform(keep_data: bool) -> Result<()> {
    // A running .exe cannot delete itself on Windows, so spawn a detached
    // PowerShell that waits for this process to exit, then fetches and runs the
    // cleanup script (which deletes the binary, install dir, PATH entry, data).
    let self_pid = std::process::id();
    let keep = if keep_data { "1" } else { "0" };
    let url = powershell_quote(UNINSTALL_PS1_URL);
    let script = format!(
        r#"$ErrorActionPreference='SilentlyContinue'; $pidToWait={pid}; while (Get-Process -Id $pidToWait -ErrorAction SilentlyContinue) {{ Start-Sleep -Milliseconds 200 }}; $env:CCG_KEEP_DATA='{keep}'; iex ((New-Object Net.WebClient).DownloadString({url}))"#,
        pid = self_pid,
        keep = keep,
        url = url
    );

    std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to schedule Windows uninstall")?;

    println!("{}", t!("uninstall.running_windows"));
    Ok(())
}

#[cfg(unix)]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(windows)]
fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
