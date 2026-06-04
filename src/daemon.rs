pub mod cleaner;
pub mod engine;
pub mod server;
pub mod state;

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs;
use std::path::PathBuf;
use tracing::info;

use crate::config::loader::ConfigLoader;
use crate::config::model::GatewayConfig;
use crate::{t, t_fmt};

const DAEMON_PID_FILE: &str = "daemon.pid";

/// Start the daemon in background (idempotent).
/// Uses file locking to prevent multiple instances — the lock is held by the
/// daemon process for its entire lifetime and automatically released on exit.
pub async fn start(config_path: Option<PathBuf>) -> Result<()> {
    let config_dir = ConfigLoader::initialize_runtime()?;
    let pid_file = config_dir.join(DAEMON_PID_FILE);

    // --- Singleton guard 1: temporary lock prevents concurrent start() races ---
    let starting_lock_file = config_dir.join(".daemon-starting.lock");
    let starting_lock = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&starting_lock_file)
        .context("Failed to open daemon starting lock file")?;
    if starting_lock.try_lock_exclusive().is_err() {
        println!("Another start operation is in progress. Please wait.");
        return Ok(());
    }

    // Singleton guard 2: check .daemon.lock to see if a daemon is already running.
    // .daemon.lock is exclusively locked by the daemon process; if we can't
    // acquire it, a daemon is already active.
    let lock_file = config_dir.join(".daemon.lock");
    if let Ok(file) = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_file)
    {
        if file.try_lock_exclusive().is_err() {
            if let Ok(pid_str) = fs::read_to_string(&pid_file) {
                println!("{}", t_fmt!("daemon.already_running", PID = pid_str.trim()));
            } else {
                println!("Daemon is already running.");
            }
            return Ok(());
        }
    }

    // Clear stale PID file before spawning.
    let _ = fs::remove_file(&pid_file);

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

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS: the child runs without inheriting the parent's console.
        // This prevents CTRL_CLOSE_EVENT from being delivered when the terminal is closed.
        const DETACHED_PROCESS: u32 = 0x00000008;
        cmd.creation_flags(DETACHED_PROCESS);
    }

    let child = cmd.spawn().context("Failed to spawn daemon process")?;
    let pid = child.id();

    // Wait for the child to enter run() and write its PID into the locked file.
    let mut confirmed = false;
    for _ in 0..10 {
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        if !is_process_alive(pid) {
            break;
        }

        #[cfg(windows)]
        {
            // On Windows the PID file is locked and cannot be read by another
            // process.  Fall back to checking whether the singleton port is
            // already bound (run() binds it before writing the PID file).
            let (port, bind_addr) = if let Some(ref path) = config_path {
                ConfigLoader::load_from(path)
                    .map(|c| (c.port, c.bind_address))
                    .unwrap_or_else(|_| (17534, "127.0.0.1".to_string()))
            } else {
                ConfigLoader::load()
                    .map(|c| (c.port, c.bind_address))
                    .unwrap_or_else(|_| (17534, "127.0.0.1".to_string()))
            };
            let bind_addr_str = format!("{}:{}", bind_addr, port);
            if std::net::TcpListener::bind(&bind_addr_str).is_err() {
                confirmed = true;
                break;
            }
        }

        #[cfg(unix)]
        {
            if let Ok(pid_str) = fs::read_to_string(&pid_file) {
                if let Ok(found_pid) = pid_str.trim().parse::<u32>() {
                    if found_pid == pid {
                        confirmed = true;
                        break;
                    }
                }
            }
        }
    }

    if confirmed {
        println!("{}", t_fmt!("daemon.started", PID = pid));
    } else {
        anyhow::bail!(
            "Daemon process spawned but failed to confirm startup (PID: {}). Check logs.",
            pid
        );
    }

    Ok(())
    // starting_lock drops here, releasing the concurrent-start guard.
}

/// Run the actual daemon engine (called by the _daemon hidden command).
/// Acquires an exclusive file lock on the PID file to prevent multiple
/// daemon instances. The lock is held for the entire process lifetime
/// and automatically released by the OS when the process exits.
pub async fn run(config_path: Option<PathBuf>) -> Result<()> {
    let config = if let Some(path) = &config_path {
        ConfigLoader::load_from(path)?
    } else {
        ConfigLoader::load()?
    };

    // --- Singleton guard 1: bind to configured address & port (will be used as HTTP server) ---
    let bind_addr = format!("{}:{}", config.bind_address, config.port);
    let std_listener = std::net::TcpListener::bind(&bind_addr).with_context(|| {
        format!(
            "Another cc-gateway daemon is already running (port {} in use)",
            config.port
        )
    })?;
    std_listener
        .set_nonblocking(true)
        .context("Failed to set listener to non-blocking")?;
    let tokio_listener = tokio::net::TcpListener::from_std(std_listener)
        .context("Failed to convert std listener to tokio listener")?;

    let config_dir = ConfigLoader::ensure_config_dir()?;
    let pid_file = config_dir.join(DAEMON_PID_FILE);

    // Singleton guard 2: exclusive flock on .daemon.lock.
    // Using a separate lock file keeps daemon.pid readable by stop()/status()
    // on Windows where LockFileEx is mandatory (prevents reads).
    let lock_file_path = config_dir.join(".daemon.lock");
    let pid_lock = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(true)
        .open(&lock_file_path)
        .context("Failed to open daemon lock file")?;

    pid_lock
        .try_lock_exclusive()
        .context("Another cc-gateway daemon is already running")?;

    // Write PID file separately (not locked, readable by stop()/status()).
    let pid = std::process::id();
    fs::write(&pid_file, format!("{}\n", pid)).context("Failed to write PID file")?;

    // Trim log file before initializing logging to avoid race with the writer.
    {
        let log_path = shellexpand::tilde(&config.log.file).to_string();
        let max_lines = config.log.max_lines;
        let max_size_mb = config.log.max_size_mb;
        match tokio::task::spawn_blocking(move || {
            cleaner::trim_log_file(&log_path, max_lines, max_size_mb)
        })
        .await
        {
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

    info!("Starting cc-gateway daemon (PID: {})", pid);

    // Start the daemon engine
    let engine = engine::DaemonEngine::new_with_config_path(config, config_path);
    engine.run(tokio_listener).await?;

    // Cleanup: locks are released when pid_lock drops.
    let _ = fs::remove_file(&pid_file);
    let _ = fs::remove_file(&lock_file_path);
    info!("cc-gateway daemon stopped (PID: {})", pid);
    Ok(())
}

pub async fn stop() -> Result<()> {
    let config_dir = ConfigLoader::ensure_config_dir()?;
    let pid_file = config_dir.join(DAEMON_PID_FILE);

    let mut pid: Option<u32> = None;

    // Try reading the PID file first.
    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(p) = pid_str.trim().parse::<u32>() {
            pid = Some(p);
        }
    }

    #[cfg(windows)]
    {
        // On Windows fall back to tasklist if PID file wasn't readable.
        // Filter out the current process PID (cc-gateway stop is also cc-gateway.exe).
        if pid.is_none() {
            let current_pid = std::process::id();
            pid = std::process::Command::new("tasklist")
                .args(["/FI", "IMAGENAME eq cc-gateway.exe", "/NH", "/FO", "CSV"])
                .output()
                .ok()
                .and_then(|output| {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    stdout
                        .lines()
                        .filter_map(|line| {
                            let pid_str = line.split(',').nth(1)?;
                            let pid = pid_str.trim_matches('"').parse::<u32>().ok()?;
                            if pid != current_pid {
                                Some(pid)
                            } else {
                                None
                            }
                        })
                        .next()
                });
        }
    }

    let Some(pid) = pid else {
        println!("{}", t!("daemon.not_running"));
        return Ok(());
    };

    if !is_process_alive(pid) {
        let _ = fs::remove_file(&pid_file);
        println!("{}", t!("daemon.stopped"));
        return Ok(());
    }

    #[cfg(unix)]
    {
        use nix::sys::signal::{self, Signal};
        use nix::unistd::Pid;
        use std::time::Duration;

        // Graceful shutdown — SIGTERM
        signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM)
            .context("Failed to send SIGTERM to daemon")?;
        println!("{}", t_fmt!("daemon.stop_signal", PID = pid));

        // Give platform shutdown enough time to notify active bot chats and
        // stop Claude child processes before falling back to SIGKILL.
        let mut died = false;
        for _ in 0..30 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if !is_process_alive(pid) {
                died = true;
                break;
            }
        }

        // Force kill if still alive
        if !died {
            eprintln!(
                "Daemon (PID: {}) did not exit after SIGTERM, sending SIGKILL...",
                pid
            );
            let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
            for _ in 0..10 {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if !is_process_alive(pid) {
                    died = true;
                    break;
                }
            }
            if !died {
                anyhow::bail!(
                    "Failed to kill daemon process (PID: {}). Please kill it manually.",
                    pid
                );
            }
        }
    }

    #[cfg(windows)]
    {
        use std::process::Command;
        println!("{}", t_fmt!("daemon.stop_signal", PID = pid));
        let result = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
            .context("Failed to run taskkill")?;
        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            anyhow::bail!(
                "Failed to kill daemon process (PID: {}). taskkill error: {}",
                pid,
                stderr.trim()
            );
        }
        for _ in 0..30 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if !is_process_alive(pid) {
                break;
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

    let mut pid: Option<u32> = None;

    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(p) = pid_str.trim().parse::<u32>() {
            pid = Some(p);
        }
    }

    #[cfg(windows)]
    {
        if pid.is_none() {
            let current_pid = std::process::id();
            pid = std::process::Command::new("tasklist")
                .args(["/FI", "IMAGENAME eq cc-gateway.exe", "/NH", "/FO", "CSV"])
                .output()
                .ok()
                .and_then(|output| {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    stdout
                        .lines()
                        .filter_map(|line| {
                            let pid_str = line.split(',').nth(1)?;
                            let pid = pid_str.trim_matches('"').parse::<u32>().ok()?;
                            if pid != current_pid {
                                Some(pid)
                            } else {
                                None
                            }
                        })
                        .next()
                });
        }
    }

    if let Some(pid) = pid {
        if is_process_alive(pid) {
            println!("{}", t_fmt!("daemon.running", PID = pid));
            return Ok(());
        }
    }

    println!("{}", t!("daemon.not_running"));
    Ok(())
}

/// Open WebUI in the default browser, starting the daemon if not already running.
pub async fn webui(config_path: Option<PathBuf>) -> Result<()> {
    webui_with(
        config_path,
        |path| async move { start(path).await },
        |url| open::that(url).context("Failed to open browser"),
    )
    .await
}

pub(crate) async fn webui_with<FStart, FStartFut, FOpen>(
    config_path: Option<PathBuf>,
    start_fn: FStart,
    open_fn: FOpen,
) -> Result<()>
where
    FStart: FnOnce(Option<PathBuf>) -> FStartFut,
    FStartFut: std::future::Future<Output = Result<()>>,
    FOpen: FnOnce(&str) -> Result<()>,
{
    let config = if let Some(ref path) = config_path {
        ConfigLoader::load_from(path)?
    } else {
        ConfigLoader::load()?
    };

    if !is_daemon_running_for_webui_check().await? {
        println!("{}", t!("daemon.webui_starting"));
        start_fn(config_path).await?;
    }

    let url = if let Some(ref token) = config.webui_token {
        format!("http://127.0.0.1:{}?token={}", config.port, token)
    } else {
        format!("http://127.0.0.1:{}", config.port)
    };
    println!("{}", t_fmt!("daemon.webui_opening", URL = url));
    open_fn(&url)?;
    Ok(())
}

async fn is_daemon_running_for_webui_check() -> Result<bool> {
    let config_dir = ConfigLoader::ensure_config_dir()?;
    let pid_file = config_dir.join(DAEMON_PID_FILE);

    let mut pid: Option<u32> = None;
    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(p) = pid_str.trim().parse::<u32>() {
            pid = Some(p);
        }
    }

    #[cfg(windows)]
    {
        if pid.is_none() {
            let current_pid = std::process::id();
            pid = std::process::Command::new("tasklist")
                .args(["/FI", "IMAGENAME eq cc-gateway.exe", "/NH", "/FO", "CSV"])
                .output()
                .ok()
                .and_then(|output| {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    stdout
                        .lines()
                        .filter_map(|line| {
                            let pid_str = line.split(',').nth(1)?;
                            let pid = pid_str.trim_matches('"').parse::<u32>().ok()?;
                            if pid != current_pid {
                                Some(pid)
                            } else {
                                None
                            }
                        })
                        .next()
                });
        }
    }

    Ok(pid.map(is_process_alive).unwrap_or(false))
}

/// Show or refresh the WebUI access token.
pub async fn webui_token(refresh: bool) -> Result<()> {
    let config_path = ConfigLoader::config_path()?;
    let mut config = if config_path.is_file() {
        ConfigLoader::load_from(&config_path).unwrap_or_default()
    } else {
        ConfigLoader::initialize_runtime()?;
        GatewayConfig::runtime_defaults()
    };

    let generated = if refresh || config.webui_token.is_none() {
        let new_token = crate::web::middleware::generate_webui_token();
        config.webui_token = Some(new_token.clone());
        ConfigLoader::save(&config).context("Failed to save config with new token")?;
        true
    } else {
        false
    };

    let token = config.webui_token.as_ref().unwrap();
    let url = if config.bind_address == "0.0.0.0" || config.bind_address == "::" {
        format!("http://127.0.0.1:{}?token={}", config.port, token)
    } else {
        format!(
            "http://{}:{}?token={}",
            config.bind_address, config.port, token
        )
    };

    println!(
        "{}",
        t_fmt!("daemon.webui_token_header", TOKEN = token.as_str())
    );
    if generated {
        if refresh {
            println!("{}", t!("daemon.webui_token_refreshed"));
        } else {
            println!("{}", t!("daemon.webui_token_generated"));
        }
    }
    println!("{}", t_fmt!("daemon.webui_token_url", URL = url));
    println!("{}", t!("daemon.webui_token_hint"));
    Ok(())
}

pub async fn restart(config_path: Option<PathBuf>) -> Result<()> {
    println!("{}", t!("daemon.restarting"));
    stop().await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    start(config_path).await
}

pub async fn enable() -> Result<()> {
    let exe = std::env::current_exe().context("Failed to determine cc-gateway executable path")?;
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
        println!(
            "{}",
            t_fmt!("daemon.plist_path", PATH = plist_path.display())
        );
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
        println!(
            "{}",
            t_fmt!("daemon.service_path", PATH = service_path.display())
        );
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
        use tokio::io::{AsyncBufReadExt, AsyncSeekExt};
        println!("{}", t!("daemon.following_log"));
        let mut file = tokio::fs::File::open(&log_path).await?;
        file.seek(std::io::SeekFrom::End(0)).await?;
        let mut reader = tokio::io::BufReader::new(file);
        let mut line = String::new();

        loop {
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    // EOF: wait a bit then retry, like tail -f
                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                }
                Ok(_) => {
                    if !line.is_empty() {
                        // Remove trailing newline if present
                        if line.ends_with('\n') {
                            line.pop();
                            if line.ends_with('\r') {
                                line.pop();
                            }
                        }
                        println!("{}", line);
                        line.clear();
                    }
                }
                Err(_) => break,
            }
        }
    }

    Ok(())
}

pub(crate) fn is_process_alive(pid: u32) -> bool {
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
