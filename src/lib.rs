//! cc-gateway: agent gateway core, HTTP API, and chat platforms.

pub mod api;
pub mod core;
pub mod daemon;
pub mod database;
pub mod platform;
pub mod types;
pub mod uninstall;
pub mod update;
pub mod utils;

#[cfg(test)]
mod tests;

// Stable crate-root paths (macros, handlers, platforms, tests).
pub use api::web;
pub use core::{agent, command, config, history, prompt, runtime, session};
pub use database as db;
pub use utils::i18n;
