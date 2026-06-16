//! `cc-gateway`: A gateway that enables remote control of local agent CLIs
//! (Claude Code, Codex, Cursor, Pi, OpenCode, Kimi, Gemini, **Qoder**, etc.) via Feishu / Lark, Telegram, and WebUI.
//!
//! # Architecture Layers
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │ Chat platform entry (Feishu / Telegram / WebUI)        │
//! ├─────────────────────────────────────────────────────────────┤
//! │ Command routing (`core::command`) + Session orchestration (`core::session`) │
//! ├─────────────────────────────────────────────────────────────┤
//! │ Agent runtime (`core::runtime`) + Agent backend             │
//! │ （`core::agent`：Claude stream-json、Codex/Cursor/OpenCode/ │
//! │  Kimi/Gemini/Qoder ACP、Pi JSON-RPC）                       │
//! ├─────────────────────────────────────────────────────────────┤
//! │ Configuration (`core::config`) + History (`core::history`)  │
//! │ + Prompt (`core::prompt`)                                   │
//! ├─────────────────────────────────────────────────────────────┤
//! │ HTTP/WebUI API (`api::web`) + Persistence (`database`)      │
//! ├─────────────────────────────────────────────────────────────┤
//! │ Daemon lifecycle (`daemon`) + Platform implementation (`platform`) │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Submodule Overview
//!
//! - [`core`]: Core business logic (agents, commands, configuration, history, prompt, runtime, session).
//! - [`api`]: HTTP / WebUI interface (Axum).
//! - [`daemon`]: Daemon lifecycle (PID file, port singleton lock, signal handling).
//! - [`database`]: SQLite persistence (sessions, channels, status).
//! - [`platform`]: Chat platform adapters (Feishu, Telegram).
//! - [`types`]: Shared type re-exports.
//! - [`update`]: GitHub Releases version checker and auto-upgrade.
//! - [`uninstall`]: Uninstall logic (binaries, autostart, PATH, data).
//! - [`utils`]: General utilities (environment variables, i18n, path handling).
//! - [`tests`]: Cross-module integration tests (unit tests are located in the same file within `#[cfg(test)] mod tests`).
//!
//! # Root-level Re-exports
//!
//! To use shorter paths like `crate::web`, `crate::db`, or `crate::i18n` throughout the codebase,
//! we re-export commonly used submodules here. When introducing cross-module references, prefer
//! using these shorter paths to avoid repeating long `crate::...` paths.

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

// Stable crate root paths: macros, WebUI handlers, platforms, and tests might directly reference these.
pub use api::web;
pub use core::{agent, command, config, history, prompt, runtime, session};
/// Short alias for the SQLite persistence module (`crate::db::*`).
pub use database as db;
pub use utils::i18n;
