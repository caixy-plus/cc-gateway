use anyhow::{Context, Result};
use std::io::Write;
use std::process::Stdio;

use crate::{t, t_fmt};

const UNINSTALL_SH_URL: &str =
    "https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/uninstall.sh";

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
    // A running .exe cannot delete itself on Windows, so we write the uninstall
    // script to a temp file and spawn a PowerShell that waits for this process
    // to exit, then runs the cleanup in the same terminal (visible output).
    //
    // The script content is embedded at compile time (not downloaded at runtime)
    // to avoid the "download + iex" pattern that triggers AV false positives.
    let self_pid = std::process::id();

    let temp_dir = std::env::var("TEMP").unwrap_or_else(|_| ".".to_string());
    let temp_file = format!(r"{}\cc-gateway-uninstall.ps1", temp_dir.trim_end_matches('\\'));
    std::fs::write(&temp_file, include_str!("../../uninstall.ps1"))
        .context("failed to write uninstall script to temp")?;

    let keep = if keep_data { "$true" } else { "$false" };
    let script = format!(
        r#"$ErrorActionPreference = 'Stop'; $pidToWait={pid}; while (Get-Process -Id $pidToWait -ErrorAction SilentlyContinue) {{ Start-Sleep -Milliseconds 200 }}; & '{temp}' -KeepData:{keep}; Remove-Item '{temp}' -ErrorAction SilentlyContinue"#,
        pid = self_pid,
        temp = temp_file.replace('\'', "''"),
        keep = keep,
    );

    std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to schedule Windows uninstall")?;

    std::process::exit(0);
}

#[cfg(unix)]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

