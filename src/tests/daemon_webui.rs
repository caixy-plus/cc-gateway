use crate::config::model::GatewayConfig;
use crate::daemon;
use crate::tests::helpers::TestEnv;
use std::sync::{Arc, Mutex};

fn write_default_config(home: &std::path::Path) {
    let config_dir = home.join(".cc-gateway");
    std::fs::create_dir_all(&config_dir).unwrap();
    let config = GatewayConfig::default();
    std::fs::write(
        config_dir.join("config.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn webui_starts_daemon_when_not_running_then_opens_url() {
    let env = TestEnv::new();
    write_default_config(env.home());

    let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let opened = Arc::new(Mutex::new(None::<String>));

    daemon::webui_with(
        None,
        {
            let started = Arc::clone(&started);
            move |_config_path| async move {
                started.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        },
        {
            let opened = Arc::clone(&opened);
            move |url| {
                *opened.lock().unwrap() = Some(url.to_string());
                Ok(())
            }
        },
    )
    .await
    .unwrap();

    assert!(started.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        opened.lock().unwrap().as_deref(),
        Some("http://127.0.0.1:17534")
    );
}

#[tokio::test]
async fn webui_does_not_start_when_daemon_is_running() {
    let env = TestEnv::new();
    write_default_config(env.home());
    // Make it look like a daemon is running by writing our current PID into the
    // pid file. is_process_alive(pid) should return true for the current process.
    let pid_file = env.home().join(".cc-gateway").join("daemon.pid");
    std::fs::write(&pid_file, std::process::id().to_string()).unwrap();

    let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let opened = Arc::new(Mutex::new(None::<String>));

    daemon::webui_with(
        None,
        {
            let started = Arc::clone(&started);
            move |_config_path| async move {
                started.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        },
        {
            let opened = Arc::clone(&opened);
            move |url| {
                *opened.lock().unwrap() = Some(url.to_string());
                Ok(())
            }
        },
    )
    .await
    .unwrap();

    assert!(!started.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        opened.lock().unwrap().as_deref(),
        Some("http://127.0.0.1:17534")
    );
}
