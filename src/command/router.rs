use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::claude::controller::ClaudeController;
use crate::command::builtin::BuiltinCommands;
use crate::{t, t_fmt};

/// Semantic action produced by parsing a user message.
///
/// All command parsing happens in one place (`CommandRouter::route`) and
/// platforms/WebUI/CLI only execute the resulting action.
#[derive(Debug, Clone)]
pub enum CommandAction {
    /// Immediate text reply (e.g. /help, error messages)
    Reply(String),
    /// Start a Claude session with optional work directory and extra args
    StartSession {
        work_dir: Option<PathBuf>,
        args: Vec<String>,
    },
    /// Stop the current Claude session
    StopSession,
    /// Change working directory to the given path
    ChangeDir(PathBuf),
    /// Change working directory to the default directory
    ChangeDirDefault,
    /// Print the current working directory
    PrintWorkingDir,
    /// List directory contents (interactive TUI for CLI, card for platforms)
    ListDir { path: Option<PathBuf> },
    /// Create a new directory
    MakeDir(PathBuf),
    /// Enable showing thinking content
    ShowThinking,
    /// Disable showing thinking content
    HideThinking,
    /// Show recent Claude sessions and allow resuming
    ShowClaudeHistory,
    /// Forward regular text to the active Claude session
    ForwardToClaude(String),
    /// Unknown slash command when no session is active
    UnknownCommand(String),
    /// No operation needed
    NoOp,
}

pub struct CommandRouter {
    builtin: BuiltinCommands,
    controller: Arc<Mutex<ClaudeController>>,
    default_dir: String,
}

impl CommandRouter {
    pub fn new(controller: Arc<Mutex<ClaudeController>>, default_dir: &str) -> Self {
        Self {
            builtin: BuiltinCommands::new(controller.clone(), default_dir),
            controller,
            default_dir: default_dir.to_string(),
        }
    }

    /// Parse a user message into a semantic `CommandAction`.
    ///
    /// This is the single source of truth for command semantics.
    /// No side effects are performed here — callers execute the action.
    pub async fn route(&self, message: &str) -> CommandAction {
        let trimmed = message.trim();
        if trimmed.is_empty() {
            return CommandAction::NoOp;
        }

        let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).map(|s| *s).unwrap_or("");

        // Check if we are in Claude session mode
        let session_active = {
            let ctrl = self.controller.lock().await;
            ctrl.is_session_active().await
        };

        if session_active {
            // Active session: local gateway commands are still handled here;
            // regular text and unknown commands go to Claude.
            match cmd {
                "/quit" | "/close-session" => CommandAction::StopSession,
                "/show-thinking" => CommandAction::ShowThinking,
                "/hide-thinking" => CommandAction::HideThinking,
                "/claude-history" => CommandAction::ShowClaudeHistory,
                "/help" => CommandAction::Reply(self.builtin.help_text()),
                "/cd" => {
                    if arg.is_empty() {
                        CommandAction::Reply(t!("builtin.cd_usage").to_string())
                    } else {
                        let expanded = shellexpand::tilde(arg).to_string();
                        CommandAction::ChangeDir(PathBuf::from(expanded))
                    }
                }
                "/cd_default" => CommandAction::ChangeDirDefault,
                "/pwd" => CommandAction::PrintWorkingDir,
                "/ll" => {
                    let path = if arg.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(shellexpand::tilde(arg).to_string()))
                    };
                    CommandAction::ListDir { path }
                }
                "/mkdir" => {
                    if arg.is_empty() {
                        CommandAction::Reply(t!("builtin.mkdir_usage").to_string())
                    } else {
                        let expanded = shellexpand::tilde(arg).to_string();
                        CommandAction::MakeDir(PathBuf::from(expanded))
                    }
                }
                "/claude" | "/new-session" => {
                    CommandAction::Reply("A session is already active. Use /quit to stop it first.".to_string())
                }
                _ => {
                    if trimmed.starts_with('/') {
                        CommandAction::UnknownCommand(cmd.to_string())
                    } else {
                        CommandAction::ForwardToClaude(trimmed.to_string())
                    }
                }
            }
        } else {
            // No active session: handle gateway commands locally
            match cmd {
                "/help" => CommandAction::Reply(self.builtin.help_text()),
                "/quit" => CommandAction::Reply("No active session to quit. Use /quit in an active session or type /help for available commands.".to_string()),
                "/cd" => {
                    if arg.is_empty() {
                        CommandAction::Reply(t!("builtin.cd_usage").to_string())
                    } else {
                        let expanded = shellexpand::tilde(arg).to_string();
                        CommandAction::ChangeDir(PathBuf::from(expanded))
                    }
                }
                "/cd_default" => CommandAction::ChangeDirDefault,
                "/pwd" => CommandAction::PrintWorkingDir,
                "/ll" => {
                    let path = if arg.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(shellexpand::tilde(arg).to_string()))
                    };
                    CommandAction::ListDir { path }
                }
                "/mkdir" => {
                    if arg.is_empty() {
                        CommandAction::Reply(t!("builtin.mkdir_usage").to_string())
                    } else {
                        let expanded = shellexpand::tilde(arg).to_string();
                        CommandAction::MakeDir(PathBuf::from(expanded))
                    }
                }
                "/claude" | "/new-session" => {
                    let args: Vec<String> = arg
                        .split_whitespace()
                        .filter(|s| *s != "--new")
                        .map(|s| s.to_string())
                        .collect();
                    CommandAction::StartSession {
                        work_dir: None,
                        args,
                    }
                }
                "/show-thinking" => CommandAction::ShowThinking,
                "/hide-thinking" => CommandAction::HideThinking,
                "/claude-history" => CommandAction::ShowClaudeHistory,
                _ => {
                    if trimmed.starts_with('/') {
                        CommandAction::UnknownCommand(cmd.to_string())
                    } else {
                        CommandAction::ForwardToClaude(trimmed.to_string())
                    }
                }
            }
        }
    }

    /// Execute a `CommandAction` and produce an optional immediate reply.
    ///
    /// This bridges the new semantic layer with the existing execution layer.
    /// Callers that want full control should use `route` directly.
    pub async fn execute(&self, action: CommandAction) -> Option<String> {
        match action {
            CommandAction::Reply(text) => Some(text),
            CommandAction::StopSession => {
                let ctrl = self.controller.lock().await;
                match ctrl.stop_session().await {
                    Ok(()) => Some(t!("builtin.session_stopped").to_string()),
                    Err(e) => Some(t_fmt!("builtin.failed_stop_session", ERR = e)),
                }
            }
            CommandAction::ChangeDir(path) => {
                let ctrl = self.controller.lock().await;
                let current_dir = ctrl.get_work_dir().await;
                let base = if current_dir.is_empty() {
                    shellexpand::tilde(&self.default_dir).to_string()
                } else {
                    current_dir
                };
                drop(ctrl);

                let target = PathBuf::from(&base).join(&path);
                let canonical = target.canonicalize().unwrap_or(target);
                if !canonical.is_dir() {
                    return Some(t_fmt!("builtin.invalid_path", PATH = canonical.display()));
                }
                let path_str = canonical.to_string_lossy().to_string();
                if let Err(e) = crate::claude::controller::ensure_under_home(&path_str) {
                    return Some(e.to_string());
                }
                let ctrl = self.controller.lock().await;
                ctrl.init_work_dir(path_str.clone()).await;
                Some(t_fmt!("builtin.dir_changed", PATH = path_str))
            }
            CommandAction::ChangeDirDefault => {
                let dir = shellexpand::tilde(&self.default_dir).to_string();
                let ctrl = self.controller.lock().await;
                ctrl.init_work_dir(dir.clone()).await;
                Some(t_fmt!("builtin.dir_changed", PATH = dir))
            }
            CommandAction::PrintWorkingDir => {
                let ctrl = self.controller.lock().await;
                let work_dir = ctrl.get_work_dir().await;
                let dir = if work_dir.is_empty() {
                    shellexpand::tilde(&self.default_dir).to_string()
                } else {
                    work_dir
                };
                Some(t_fmt!("builtin.current_dir", DIR = dir))
            }
            CommandAction::ListDir { path } => {
                let ctrl = self.controller.lock().await;
                let work_dir = ctrl.get_work_dir().await;
                let dir = if work_dir.is_empty() {
                    shellexpand::tilde(&self.default_dir).to_string()
                } else {
                    work_dir
                };
                drop(ctrl);

                let target = path.unwrap_or_else(|| PathBuf::from(&dir));
                if let Err(e) = crate::claude::controller::ensure_under_home(
                    &target.to_string_lossy(),
                ) {
                    return Some(t_fmt!("builtin.access_denied", ERR = e));
                }

                let items = match crate::command::builtin::list_directory_items(
                    &target.to_string_lossy(),
                ) {
                    Ok(items) => items,
                    Err(e) => return Some(t_fmt!("builtin.failed_list_dir", ERR = e)),
                };

                let dirs: Vec<(String, bool)> = items
                    .into_iter()
                    .filter(|(name, is_dir)| *is_dir && !name.starts_with('.'))
                    .collect();

                if dirs.is_empty() {
                    return Some(t!("builtin.no_subdirs").to_string());
                }

                let dirs_clone = dirs.clone();
                let selected =
                    tokio::task::spawn_blocking(move || {
                        crate::command::builtin::interactive_select(&dirs_clone)
                    })
                    .await
                    .unwrap_or(crate::command::builtin::SelectAction::Cancelled);

                match selected {
                    crate::command::builtin::SelectAction::Selected(idx) => {
                        let name = &dirs[idx].0;
                        let path = PathBuf::from(&dir).join(name);
                        let path_str = path.to_string_lossy().to_string();
                        let ctrl = self.controller.lock().await;
                        ctrl.init_work_dir(path_str.clone()).await;
                        Some(t_fmt!("builtin.changed_dir", PATH = path_str))
                    }
                    _ => Some(t!("builtin.selection_cancelled").to_string()),
                }
            }
            CommandAction::MakeDir(path) => {
                let ctrl = self.controller.lock().await;
                let work_dir = ctrl.get_work_dir().await;
                let base = if work_dir.is_empty() {
                    shellexpand::tilde(&self.default_dir).to_string()
                } else {
                    work_dir
                };
                drop(ctrl);

                let target = PathBuf::from(&base).join(&path);
                let target_str = target.to_string_lossy().to_string();
                if let Err(e) = crate::claude::controller::ensure_under_home(&target_str) {
                    return Some(e.to_string());
                }
                match std::fs::create_dir_all(&target) {
                    Ok(()) => Some(t_fmt!("builtin.dir_created", PATH = target_str)),
                    Err(e) => Some(t_fmt!("builtin.failed_create_dir", ERR = e)),
                }
            }
            CommandAction::StartSession { work_dir, args } => {
                let ctrl = self.controller.lock().await;
                let dir = if let Some(p) = work_dir {
                    p.to_string_lossy().to_string()
                } else {
                    let wd = ctrl.get_work_dir().await;
                    if wd.is_empty() {
                        shellexpand::tilde(&self.default_dir).to_string()
                    } else {
                        wd
                    }
                };
                match ctrl.start_session(dir.clone(), args).await {
                    Ok(()) => Some(t_fmt!("builtin.session_started", DIR = dir)),
                    Err(e) => Some(t_fmt!("builtin.failed_start_claude", ERR = e)),
                }
            }
            CommandAction::ShowThinking => {
                let ctrl = self.controller.lock().await;
                ctrl.set_show_thinking(true);
                Some(t!("builtin.thinking_enabled").to_string())
            }
            CommandAction::HideThinking => {
                let ctrl = self.controller.lock().await;
                ctrl.set_show_thinking(false);
                Some(t!("builtin.thinking_disabled").to_string())
            }
            CommandAction::ShowClaudeHistory => {
                let arg = ""; // TODO: pass arg through CommandAction if needed
                Some(self.builtin.claude_history(arg).await)
            }
            CommandAction::UnknownCommand(cmd) => {
                Some(format!("Unknown command: {}. Available commands: /help, /cd, /claude, /claude-history, /ll, /mkdir, /quit, /pwd, /show-thinking, /hide-thinking", cmd))
            }
            CommandAction::ForwardToClaude(text) => {
                let ctrl = self.controller.lock().await;
                if !ctrl.is_session_active().await {
                    return Some(crate::i18n::dict::tfmt("forward.no_session", &[("MSG", &text)]));
                }
                match ctrl.send_message(&text).await {
                    Ok(()) => None,
                    Err(e) => Some(t_fmt!("forward.failed_send", ERR = e)),
                }
            }
            CommandAction::NoOp => Some(String::new()),
        }
    }
}
