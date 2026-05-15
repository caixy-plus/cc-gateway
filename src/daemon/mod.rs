pub mod engine;
pub mod server;
pub mod state;

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use tracing::{info, warn};

use crate::config::loader::ConfigLoader;

const DAEMON_PID_FILE: &str = "daemon.pid";
const DAEMON_SOCKET_FILE: &str = "daemon.sock";

pub async fn start(config_path: Option<PathBuf>) -> Result<()> {
    let config_dir = ConfigLoader::ensure_config_dir()?;
    let pid_file = config_dir.join(DAEMON_PID_FILE);

    // Check if already running
    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            if is_process_alive(pid) {
                println!("cc-gateway daemon is already running (PID: {})", pid);
                return Ok(());
            }
        }
    }

    let config = if let Some(path) = config_path {
        ConfigLoader::load_from(&path)?
    } else {
        ConfigLoader::load()?
    };

    info!("Starting cc-gateway daemon");

    #[cfg(unix)]
    {
        use std::env;
        if env::var("CC_GATEWAY_DAEMONIZED").is_err() {
            // Fork to background
            let daemonize = daemonize::Daemonize::new()
                .pid_file(&pid_file)
                .working_directory(&config_dir);

            match daemonize.start() {
                Ok(()) => {
                    env::set_var("CC_GATEWAY_DAEMONIZED", "1");
                }
                Err(e) => {
                    warn!("Failed to daemonize (running in foreground): {}", e);
                }
            }
        }
    }

    // Write PID file
    let pid = std::process::id();
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
                println!("Sent stop signal to daemon (PID: {})", pid);

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
    println!("Daemon stopped.");
    Ok(())
}

pub async fn restart(config_path: Option<PathBuf>) -> Result<()> {
    stop().await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    start(config_path).await?;
    Ok(())
}

pub async fn log(follow: bool, lines: usize) -> Result<()> {
    let config = ConfigLoader::load()?;
    let log_path = shellexpand::tilde(&config.log.file).to_string();

    if !std::path::Path::new(&log_path).exists() {
        println!("Log file not found: {}", log_path);
        return Ok(());
    }

    let content = fs::read_to_string(&log_path)?;
    let log_lines: Vec<&str> = content.lines().collect();

    let start = log_lines.len().saturating_sub(lines);
    for line in &log_lines[start..] {
        println!("{}", line);
    }

    if follow {
        use std::io::{self, BufRead};
        println!("\n-- Following log (Ctrl+C to exit) --");
        let file = std::fs::File::open(&log_path)?;
        let reader = io::BufReader::new(file);
        for line in reader.lines().skip(log_lines.len()) {
            match line {
                Ok(l) => println!("{}", l),
                Err(_) => break,
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    Ok(())
}

fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        use nix::sys::signal::{self, Signal};
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
