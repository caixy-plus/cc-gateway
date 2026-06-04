use std::sync::atomic::{AtomicU8, Ordering};

use dashmap::DashMap;
use once_cell::sync::Lazy;

use crate::config::platform_registry::PLATFORM_DEFS;

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

static PLATFORM_STATES: Lazy<DashMap<&'static str, AtomicU8>> = Lazy::new(|| {
    let map = DashMap::new();
    for def in PLATFORM_DEFS {
        map.insert(def.id, AtomicU8::new(ConnectionState::Disconnected as u8));
    }
    map
});

pub fn set_state(name: &str, state: ConnectionState) {
    let Some(atom) = PLATFORM_STATES.get(name) else {
        return;
    };
    atom.store(state as u8, Ordering::Relaxed);
}

pub fn get_state(name: &str) -> ConnectionState {
    let val = PLATFORM_STATES
        .get(name)
        .map(|a| a.load(Ordering::Relaxed))
        .unwrap_or(ConnectionState::Disconnected as u8);
    match val {
        0 => ConnectionState::Disconnected,
        1 => ConnectionState::Connecting,
        2 => ConnectionState::Connected,
        _ => ConnectionState::Disconnected,
    }
}
