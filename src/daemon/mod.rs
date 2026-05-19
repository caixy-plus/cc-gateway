pub mod engine;
pub mod log_cleaner;
pub mod server;
pub mod state;

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use tracing::info;

use crate::config::loader::ConfigLoader;
use crate::{t, t_fmt};

const DAEMON_PID_FILE: &str = "daemon.pid";
#[allow(dead_code)]
const DAEMON_SOCKET_FILE: &str = "daemon.sock";

/// Start the daemon in background (idempotent).
/// If already running, prints status and returns immediately.
/// Otherwise spawns a detached child process and exits.
pub async fn start(config_path: Option<PathBuf>) -> Result<()> {
    let config_dir = ConfigLoader::ensure_config_dir()?;
    let pid_file = config_dir.join(DAEMON_PID_FILE);

    // Check if already running
    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            if is_process_alive(pid) {
                println!("{}", t_fmt!("daemon.already_running", PID = pid));
                return Ok(());
            }
        }
    }

    // Spawn detached child process that runs the actual daemon
    let exe = std::env::current_exe().context("Failed to get current executable path")?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("_daemon");
    if let Some(path) = &config_path {
        cmd.arg("--config").arg(path);
    }
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let child = cmd.spawn().context("Failed to spawn daemon process")?;
    let pid = child.id();
    // Write PID immediately so subsequent start() calls see it
    let _ = fs::write(&pid_file, pid.to_string());
    println!("{}", t_fmt!("daemon.started", PID = pid));
    Ok(())
}

/// Run the actual daemon engine (called by the _daemon hidden command).
pub async fn run(config_path: Option<PathBuf>) -> Result<()> {
    let config_dir = ConfigLoader::ensure_config_dir()?;
    let pid_file = config_dir.join(DAEMON_PID_FILE);

    let config = if let Some(path) = config_path {
        ConfigLoader::load_from(&path)?
    } else {
        ConfigLoader::load()?
    };

    // Trim log file before initializing logging to avoid race with the writer.
    {
        let log_path = shellexpand::tilde(&config.log.file).to_string();
        let max_lines = config.log.max_lines;
        let max_size_mb = config.log.max_size_mb;
        match tokio::task::spawn_blocking(move || {
            log_cleaner::trim_log_file(&log_path, max_lines, max_size_mb)
        }).await {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => {}
            Ok(Err(e)) => eprintln!("Warning: failed to trim log file: {}", e),
            Err(e) => eprintln!("Warning: log trim task panicked: {}", e),
        }
    }

    // Initialize logging before anything else so we can see errors.
    let log_path = shellexpand::tilde(&config.log.file).to_string();
    let log_dir = std::path::Path::new(&log_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(log_dir)?;

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let (non_blocking, _guard) = tracing_appender::non_blocking(log_file);

    let env_filter = tracing_subscriber::EnvFilter::try_new(&config.log.level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
        .init();

    info!("Starting cc-gateway daemon");

    // Write PID file, but refuse if another daemon is already running
    let pid = std::process::id();
    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(existing_pid) = pid_str.trim().parse::<u32>() {
            if existing_pid != pid && is_process_alive(existing_pid) {
                anyhow::bail!(
                    "Another cc-gateway daemon is already running (PID: {}). Use 'cc-gateway restart' instead.",
                    existing_pid
                );
            }
        }
    }
    fs::write(&pid_file, pid.to_string())?;

    // Start the daemon engine
    let engine = engine::DaemonEngine::new(config);
    engine.run().await?;

    // Cleanup
    let _ = fs::remove_file(&pid_file);
    Ok(())
}

pub async fn stop() -> Result<()> {
    let config_dir = ConfigLoader::ensure_config_dir()?;
    let pid_file = config_dir.join(DAEMON_PID_FILE);

    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            if is_process_alive(pid) {
                #[cfg(unix)]
                {
                    use nix::sys::signal::{self, Signal};
                    use nix::unistd::Pid;
                    signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM)
                        .context("Failed to send SIGTERM to daemon")?;
                }
                #[cfg(windows)]
                {
                    use std::process::Command;
                    Command::new("taskkill")
                        .args(["/PID", &pid.to_string(), "/F"])
                        .output()?;
                }
                println!("{}", t_fmt!("daemon.stop_signal", PID = pid));

                // Wait for process to exit
                for _ in 0..30 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    if !is_process_alive(pid) {
                        break;
                    }
                }
            }
        }
    }

    let _ = fs::remove_file(&pid_file);
    println!("{}", t!("daemon.stopped"));
    Ok(())
}

pub async fn status() -> Result<()> {
    let config_dir = ConfigLoader::ensure_config_dir()?;
    let pid_file = config_dir.join(DAEMON_PID_FILE);

    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            if is_process_alive(pid) {
                println!("{}", t_fmt!("daemon.running", PID = pid));
                return Ok(());
            }
        }
    }

    println!("{}", t!("daemon.not_running"));
    Ok(())
}

pub async fn restart(config_path: Option<PathBuf>) -> Result<()> {
    stop().await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    start(config_path).await
}

pub async fn enable() -> Result<()> {
    let exe = std::env::current_exe()
        .context("Failed to determine cc-gateway executable path")?;
    let exe_str = exe.to_string_lossy();
    let config_dir = ConfigLoader::ensure_config_dir()?;
    let config_dir_str = config_dir.to_string_lossy();

    #[cfg(target_os = "macos")]
    {
        let plist_dir = dirs::home_dir()
            .context("Failed to get home directory")?
            .join("Library/LaunchAgents");
        fs::create_dir_all(&plist_dir)?;
        let plist_path = plist_dir.join("com.cc-gateway.daemon.plist");

        let plist_content = format!(
            r##"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.cc-gateway.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>start</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{}/logs/daemon.stdout</string>
    <key>StandardErrorPath</key>
    <string>{}/logs/daemon.stderr</string>
</dict>
</plist>"##,
            exe_str, config_dir_str, config_dir_str
        );
        fs::write(&plist_path, plist_content)?;

        let status = std::process::Command::new("launchctl")
            .args(["load", "-w", plist_path.to_str().unwrap()])
            .status()?;
        if !status.success() {
            anyhow::bail!("{}", t!("daemon.launchctl_load_failed"));
        }
        println!("{}", t!("daemon.auto_start_enabled_macos"));
        println!("{}", t_fmt!("daemon.plist_path", PATH = plist_path.display()));
    }

    #[cfg(target_os = "linux")]
    {
        let systemd_dir = dirs::home_dir()
            .context("Failed to get home directory")?
            .join(".config/systemd/user");
        fs::create_dir_all(&systemd_dir)?;
        let service_path = systemd_dir.join("cc-gateway.service");

        let service_content = format!(
            r#"[Unit]
Description=cc-gateway daemon
After=network.target

[Service]
Type=simple
ExecStart={} start
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#,
            exe_str
        );
        fs::write(&service_path, service_content)?;

        let _ = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status()?;
        let status = std::process::Command::new("systemctl")
            .args(["--user", "enable", "cc-gateway.service"])
            .status()?;
        if !status.success() {
            anyhow::bail!("{}", t!("daemon.systemctl_enable_failed"));
        }
        println!("{}", t!("daemon.auto_start_enabled_linux"));
        println!("{}", t_fmt!("daemon.service_path", PATH = service_path.display()));
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("{}", t!("daemon.auto_start_unsupported"));
    }

    Ok(())
}

pub async fn disable() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = dirs::home_dir()
            .context("Failed to get home directory")?
            .join("Library/LaunchAgents/com.cc-gateway.daemon.plist");

        if plist_path.exists() {
            let _ = std::process::Command::new("launchctl")
                .args(["unload", "-w", plist_path.to_str().unwrap()])
                .status()?;
            fs::remove_file(&plist_path)?;
        }
        println!("{}", t!("daemon.auto_start_disabled_macos"));
    }

    #[cfg(target_os = "linux")]
    {
        let service_path = dirs::home_dir()
            .context("Failed to get home directory")?
            .join(".config/systemd/user/cc-gateway.service");

        if service_path.exists() {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "disable", "cc-gateway.service"])
                .status()?;
            fs::remove_file(&service_path)?;
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .status()?;
        }
        println!("{}", t!("daemon.auto_start_disabled_linux"));
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("{}", t!("daemon.auto_start_unsupported"));
    }

    Ok(())
}

pub async fn log(follow: bool, lines: usize) -> Result<()> {
    let config = ConfigLoader::load()?;
    let log_path = shellexpand::tilde(&config.log.file).to_string();

    if !std::path::Path::new(&log_path).exists() {
        println!("{}", t_fmt!("daemon.log_not_found", PATH = log_path));
        return Ok(());
    }

    let content = fs::read_to_string(&log_path)?;
    let log_lines: Vec<&str> = content.lines().collect();

    let start = log_lines.len().saturating_sub(lines);
    for line in &log_lines[start..] {
        println!("{}", line);
    }

    if follow {
        use std::io::{self, BufRead, Seek};
        println!("{}", t!("daemon.following_log"));
        let mut file = std::fs::File::open(&log_path)?;
        file.seek(io::SeekFrom::End(0))?;
        let mut reader = io::BufReader::new(file);
        let mut buf = String::new();

        loop {
            match reader.read_line(&mut buf) {
                Ok(0) => {
                    // EOF: wait a bit then retry, like tail -f
                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                }
                Ok(_) => {
                    if !buf.is_empty() {
                        // Remove trailing newline if present
                        if buf.ends_with('\n') {
                            buf.pop();
                            if buf.ends_with('\r') {
                                buf.pop();
                            }
                        }
                        println!("{}", buf);
                        buf.clear();
                    }
                }
                Err(_) => break,
            }
        }
    }

    Ok(())
}

fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        use nix::sys::signal;
        use nix::unistd::Pid;
        signal::kill(Pid::from_raw(pid as i32), None).is_ok()
    }
    #[cfg(windows)]
    {
        use std::process::Command;
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
