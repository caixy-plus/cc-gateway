use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::command::agents;
use crate::command::builtin::BuiltinCommands;
use crate::config::model::AgentProvider;
use crate::runtime::controller::AgentController;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
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
        provider: Option<AgentProvider>,
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
    ShowAgentHistory { arg: String },
    /// Set this channel's default agent (`/agents` picker or `/agents claude|cursor`)
    SelectChannelAgent { provider: Option<AgentProvider> },
    /// Forward regular text to the active Claude session
    ForwardToAgent(String),
    /// Unknown slash command when no session is active
    UnknownCommand(String),
    /// No operation needed
    NoOp,
}

pub struct CommandRouter {
    builtin: BuiltinCommands,
    controller: Arc<Mutex<AgentController>>,
    default_dir: String,
    channel_id: Option<String>,
}

impl CommandRouter {
    pub fn new(controller: Arc<Mutex<AgentController>>, default_dir: &str) -> Self {
        Self {
            builtin: BuiltinCommands::new(controller.clone(), default_dir),
            controller,
            default_dir: default_dir.to_string(),
            channel_id: None,
        }
    }

    pub fn with_channel_id(mut self, channel_id: impl Into<String>) -> Self {
        self.channel_id = Some(channel_id.into());
        self
    }

    #[cfg(test)]
    pub async fn current_work_dir(&self) -> String {
        let ctrl = self.controller.lock().await;
        let work_dir = ctrl.get_work_dir().await;
        if work_dir.is_empty() {
            shellexpand::tilde(&self.default_dir).to_string()
        } else {
            work_dir
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
        let arg = parts.get(1).copied().unwrap_or("");

        // Check if we are in Claude session mode
        let session_active = {
            let ctrl = self.controller.lock().await;
            ctrl.is_session_active().await
        };

        if session_active {
            // Active session: Claude owns the conversation. Keep only gateway
            // controls that affect the gateway process itself.
            match cmd {
                "/quit" => CommandAction::StopSession,
                "/show-thinking" | "/show_thinking" => CommandAction::ShowThinking,
                "/hide-thinking" | "/hide_thinking" => CommandAction::HideThinking,
                _ => CommandAction::ForwardToAgent(trimmed.to_string()),
            }
        } else {
            // No active session: handle gateway commands locally
            match cmd {
                "/help" => CommandAction::Reply(self.builtin.help_text()),
                "/quit" => {
                    CommandAction::Reply(t!("builtin.no_active_session_to_quit").to_string())
                }
                "/cd" => {
                    if arg.is_empty() {
                        CommandAction::ListDir { path: None }
                    } else {
                        let expanded = shellexpand::tilde(arg).to_string();
                        CommandAction::ChangeDir(PathBuf::from(expanded))
                    }
                }
                "/cd_up" => CommandAction::ChangeDir(PathBuf::from("..")),
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
                "/agent" | "/agent_claude" => {
                    let mut args: Vec<String> = arg
                        .split_whitespace()
                        .filter(|s| *s != "--new")
                        .map(|s| s.to_string())
                        .collect();
                    let provider = match cmd {
                        "/agent_claude" => Some(AgentProvider::Claude),
                        _ => parse_provider_prefix(&mut args),
                    };
                    CommandAction::StartSession {
                        work_dir: None,
                        provider,
                        args,
                    }
                }
                "/agent_cursor" => CommandAction::StartSession {
                    work_dir: None,
                    provider: Some(AgentProvider::Cursor),
                    args: if arg.is_empty() {
                        vec![]
                    } else {
                        arg.split_whitespace().map(|s| s.to_string()).collect()
                    },
                },
                "/show-thinking" | "/show_thinking" => CommandAction::ShowThinking,
                "/hide-thinking" | "/hide_thinking" => CommandAction::HideThinking,
                "/agent-history" | "/agent_history" => CommandAction::ShowAgentHistory {
                    arg: arg.to_string(),
                },
                "/agents" | "/agents_claude" | "/agents_cursor" => {
                    let mut args: Vec<String> =
                        arg.split_whitespace().map(|s| s.to_string()).collect();
                    let provider = match cmd {
                        "/agents_claude" => Some(AgentProvider::Claude),
                        "/agents_cursor" => Some(AgentProvider::Cursor),
                        _ => parse_provider_prefix(&mut args),
                    };
                    if provider.is_some() || arg.trim().is_empty() {
                        CommandAction::SelectChannelAgent { provider }
                    } else {
                        CommandAction::UnknownCommand(cmd.to_string())
                    }
                }
                _ => {
                    if trimmed.starts_with('/') {
                        CommandAction::UnknownCommand(cmd.to_string())
                    } else {
                        CommandAction::Reply(t_fmt!("forward.no_session", MSG = trimmed))
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
                let provider =
                    crate::config::model::AgentProvider::parse_str(&ctrl.provider_name().await);
                match ctrl.force_stop_session().await {
                    Ok(()) => Some(crate::command::agents::session_stopped_message(&provider)),
                    Err(e) => Some(t_fmt!("builtin.failed_stop_session", ERR = e)),
                }
            }
            CommandAction::ChangeDir(path) => {
                let ctrl = self.controller.lock().await;
                let current_dir = ctrl.get_work_dir().await;
                drop(ctrl);

                let path_str = match crate::command::workdir::resolve_work_dir_target(
                    &current_dir,
                    &self.default_dir,
                    &path,
                ) {
                    Ok(path) => path,
                    Err(e) => return Some(e.to_string()),
                };
                let ctrl = self.controller.lock().await;
                ctrl.init_work_dir(path_str.clone()).await;
                Some(t_fmt!("builtin.dir_changed", PATH = path_str))
            }
            CommandAction::ChangeDirDefault => {
                let current_dir = {
                    let ctrl = self.controller.lock().await;
                    ctrl.get_work_dir().await
                };
                let dir = match crate::command::workdir::resolve_work_dir_target(
                    &current_dir,
                    &self.default_dir,
                    std::path::Path::new(&self.default_dir),
                ) {
                    Ok(path) => path,
                    Err(e) => return Some(e.to_string()),
                };
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
                let dir = crate::command::workdir::effective_work_dir(&work_dir, &self.default_dir);
                drop(ctrl);

                let requested = path.unwrap_or_else(|| PathBuf::from("."));
                let target = match crate::command::workdir::resolve_work_dir_target(
                    &dir,
                    &self.default_dir,
                    &requested,
                ) {
                    Ok(path) => path,
                    Err(e) => return Some(e.to_string()),
                };

                let items = match crate::command::builtin::list_directory_items(
                    &target,
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
                        let path = PathBuf::from(&target).join(name);
                        let path_str = match crate::command::workdir::resolve_work_dir_target(
                            &target,
                            &self.default_dir,
                            &path,
                        ) {
                            Ok(path) => path,
                            Err(e) => return Some(e.to_string()),
                        };
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
                if let Err(e) = crate::runtime::controller::ensure_under_home(&target_str) {
                    return Some(e.to_string());
                }
                match std::fs::create_dir_all(&target) {
                    Ok(()) => Some(t_fmt!("builtin.dir_created", PATH = target_str)),
                    Err(e) => Some(t_fmt!("builtin.failed_create_dir", ERR = e)),
                }
            }
            CommandAction::SelectChannelAgent { provider: explicit } => {
                let channel_id = match self.channel_id.as_deref() {
                    Some(id) => id.to_string(),
                    None => return Some(t!("builtin.agents_requires_channel").to_string()),
                };
                let profiles = {
                    let ctrl = self.controller.lock().await;
                    ctrl.agent_profiles().clone()
                };
                let current =
                    GLOBAL_CHANNEL_SESSIONS.effective_channel_provider(&channel_id, &profiles);
                let selected = if let Some(p) = explicit {
                    p
                } else {
                    let picked = tokio::task::spawn_blocking(move || {
                        agents::interactive_select_provider(&current)
                    })
                    .await
                    .unwrap_or(crate::command::builtin::SelectAction::Cancelled);
                    match picked {
                        crate::command::builtin::SelectAction::Selected(idx) => {
                            match agents::provider_at_index(idx) {
                                Some(p) => p,
                                None => return Some(t!("builtin.invalid_agent_index").to_string()),
                            }
                        }
                        _ => return Some(t!("builtin.selection_cancelled").to_string()),
                    }
                };
                let name = agents::provider_display_name(&selected);
                match GLOBAL_CHANNEL_SESSIONS.set_channel_default_provider(&channel_id, selected)
                {
                    Ok(()) => Some(t_fmt!("builtin.channel_agent_set", NAME = name)),
                    Err(e) => Some(t_fmt!("builtin.failed_set_channel_agent", ERR = e)),
                }
            }
            CommandAction::StartSession {
                work_dir,
                provider,
                args,
            } => {
                let ctrl = self.controller.lock().await;
                let profiles = ctrl.agent_profiles().clone();
                let channel_id = self.channel_id.clone();
                let resolved_provider = match channel_id.as_ref() {
                    Some(channel_id) => GLOBAL_CHANNEL_SESSIONS.resolve_start_provider(
                        channel_id,
                        &profiles,
                        provider,
                    ),
                    None => provider.unwrap_or_else(|| profiles.default.clone()),
                };
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
                match ctrl
                    .start_session_with_provider(dir.clone(), args, Some(resolved_provider.clone()))
                    .await
                {
                    Ok(()) => Some(agents::session_started_message(&resolved_provider, &dir)),
                    Err(e) => Some(agents::failed_start_agent_message(&resolved_provider, e)),
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
            CommandAction::ShowAgentHistory { arg } => {
                Some(self.builtin.agent_history(&arg).await)
            }
            CommandAction::UnknownCommand(cmd) => {
                Some(format!("Unknown command: {}. Available commands: /help, /cd, /agent, /agents, /agent-history, /ll, /mkdir, /quit, /pwd, /show-thinking, /hide-thinking", cmd))
            }
            CommandAction::ForwardToAgent(text) => {
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

fn parse_provider_prefix(args: &mut Vec<String>) -> Option<AgentProvider> {
    let provider = match args.first().map(|s| s.as_str()) {
        Some("claude") => Some(AgentProvider::Claude),
        Some("cursor") => Some(AgentProvider::Cursor),
        _ => None,
    };
    if provider.is_some() {
        args.remove(0);
    }
    provider
}
