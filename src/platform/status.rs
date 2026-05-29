use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ConnectionState {
    Disconnected = 0,
    Connecting = 1,
    Connected = 2,
}

impl ConnectionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
        }
    }
}

static FEISHU_STATE: AtomicU8 = AtomicU8::new(ConnectionState::Disconnected as u8);
static TELEGRAM_STATE: AtomicU8 = AtomicU8::new(ConnectionState::Disconnected as u8);

pub fn set_state(name: &str, state: ConnectionState) {
    let atom = match name {
        "feishu" => &FEISHU_STATE,
        "telegram" => &TELEGRAM_STATE,
        _ => return,
    };
    atom.store(state as u8, Ordering::Relaxed);
}

pub fn get_state(name: &str) -> ConnectionState {
    let val = match name {
        "feishu" => FEISHU_STATE.load(Ordering::Relaxed),
        "telegram" => TELEGRAM_STATE.load(Ordering::Relaxed),
        _ => return ConnectionState::Disconnected,
    };
    match val {
        0 => ConnectionState::Disconnected,
        1 => ConnectionState::Connecting,
        2 => ConnectionState::Connected,
        _ => ConnectionState::Disconnected,
    }
}
