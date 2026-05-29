use std::sync::atomic::{AtomicBool, Ordering};

static FEISHU_CONNECTED: AtomicBool = AtomicBool::new(false);
static TELEGRAM_CONNECTED: AtomicBool = AtomicBool::new(false);

pub fn set_connected(name: &str, connected: bool) {
    let atom = match name {
        "feishu" => &FEISHU_CONNECTED,
        "telegram" => &TELEGRAM_CONNECTED,
        _ => return,
    };
    atom.store(connected, Ordering::Relaxed);
}

pub fn is_connected(name: &str) -> bool {
    match name {
        "feishu" => FEISHU_CONNECTED.load(Ordering::Relaxed),
        "telegram" => TELEGRAM_CONNECTED.load(Ordering::Relaxed),
        _ => false,
    }
}
