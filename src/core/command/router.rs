//! Command routing: parses chat messages into [`CommandAction`], then passes them to [`super::executor`] for execution.
//!
//! # Flow
//!
//! 1. The user sends a text message in Feishu / Telegram / QQ / WebUI;
//! 2. [`CommandRouter::route`] evaluates the text to determine if it is a gateway control command (e.g., `/help`, `/agent`,
//!    `/cd`, `/ll`, `/models`, `/allow`, `/deny`, etc.) or a regular text message;
//! 3. It returns a [`CommandAction`] semantic action;
//! 4. The caller (platform / WebUI) passes the action to [`super::executor::ChatCommandExecutor`]
//!    for execution.
//!
//! Parsing rules are consolidated into a single function, [`CommandRouter::route`], so the platform layer does not need to duplicate string matching.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::command::agents;
use crate::command::builtin::BuiltinCommands;
use crate::command::models;
use crate::config::model::AgentProvider;
use crate::runtime::controller::AgentController;
use crate::runtime::protocol::{build_permission_allow, build_permission_deny};
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::{t, t_fmt};

/// Semantic action parsed from a user message.
///
/// All command parsing is completed in one place ([`CommandRouter::route`]). The platform/WebUI only executes
/// the resulting action and does not need to repeat string matching.
#[derive(Debug, Clone)]
pub enum CommandAction {
    /// Reply directly with text (e.g., `/help`, error messages).
    Reply(String),
    /// Start a new provider session (with optional work_dir, provider, and additional CLI arguments).
    StartSession {
        work_dir: Option<PathBuf>,
        provider: Option<AgentProvider>,
        args: Vec<String>,
    },
    /// Terminate the current provider session (`/quit`).
    StopSession,
    /// `/cd <path>`: Switch the working directory.
    ChangeDir(PathBuf),
    /// `/cd`: Switch to the configured default directory.
    ChangeDirDefault,
    /// `/pwd`: Print the current working directory.
    PrintWorkingDir,
    /// `/ll [path]`: List directory; Feishu renders a card, Telegram renders an inline keyboard,
    /// while QQ and WebUI render plain text.
    ListDir { path: Option<PathBuf> },
    /// `/mkdir <name>`: Create a new directory.
    MakeDir(PathBuf),
    /// `/show-thinking`: Enable rendering of the Thinking block.
    ShowThinking,
    /// `/hide-thinking`: Disable rendering of the Thinking block.
    HideThinking,
    /// `/history [filter]`: Display recent session history, which can be restored using `/resume <id>`.
    ShowAgentHistory { arg: String },
    /// `/agents [provider]`: Select the default agent for this channel.
    SelectChannelAgent { provider: Option<AgentProvider> },
    /// `/stop`: Cancel the current generation; the session process remains alive.
    StopGeneration,
    /// `/clear`: Restart the provider session in the same directory (clearing context).
    ClearSession,
    /// `/compact [hint]`: Compact session history (provider-specific; hint is an optional focus suggestion).
    CompactSession { arg: String },
    /// `/memory [arg]`: Initialize the provider's memory file (e.g., Claude's `CLAUDE.md`).
    InitSessionMemory { arg: String },
    /// `/models [arg]`: List or switch the current provider's model.
    Models { arg: String },
    /// `/status`: Display the current session status (idle / generating).
    Status,
    /// Regular text message: Forward to the currently active provider session.
    ForwardToAgent(String),
    /// Unknown slash command (when no session is active).
    UnknownCommand(String),
    /// `/allow [request_id]`: Allow a pending permission / confirmation / choice / question request.
    /// Uses the controller's currently pending request if `request_id = None`.
    PermissionAllow { request_id: Option<String> },
    /// `/deny [request_id] [reason]`: Deny a pending permission / confirmation / choice / question request.
    PermissionDeny {
        request_id: Option<String>,
        reason: Option<String>,
    },
    /// No operation (parsing completed but no action needs to be executed, e.g., empty message).
    NoOp,
}

/// Command router.
///
/// Holds [`BuiltinCommands`] (e.g., `/help`, `/ll` text rendering) and an optional
/// [`AgentController`] (currently active agent session). All platforms share the same router instance.
pub struct CommandRouter {
    builtin: BuiltinCommands,
    controller: Arc<Mutex<AgentController>>,
    default_dir: String,
    channel_id: Option<String>,
}

fn parse_provider_prefix(args: &mut Vec<String>) -> Option<AgentProvider> {
    let provider = args
        .first()
        .and_then(|s| crate::config::agent_registry::parse_provider_id(s));
    if provider.is_some() {
        args.remove(0);
    }
    provider
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
            // Active session: all slash commands are handled by cc-gateway; plain text goes to the agent.
            if !trimmed.starts_with('/') {
                return CommandAction::ForwardToAgent(trimmed.to_string());
            }
            let session_help = || CommandAction::Reply(self.builtin.session_help_text());
            match cmd {
                "/help" => session_help(),
                "/quit" => CommandAction::StopSession,
                "/stop" => CommandAction::StopGeneration,
                "/clear" => CommandAction::ClearSession,
                "/compact" => CommandAction::CompactSession {
                    arg: arg.to_string(),
                },
                "/init" => CommandAction::InitSessionMemory {
                    arg: arg.to_string(),
                },
                "/models" | "/model" => CommandAction::Models {
                    arg: arg.to_string(),
                },
                "/status" => CommandAction::Status,
                "/show-thinking" | "/show_thinking" => CommandAction::ShowThinking,
                "/hide-thinking" | "/hide_thinking" => CommandAction::HideThinking,
                _ => session_help(),
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
                "/agent" => {
                    let mut args: Vec<String> = arg
                        .split_whitespace()
                        .filter(|s| *s != "--new")
                        .map(|s| s.to_string())
                        .collect();
                    let provider = parse_provider_prefix(&mut args);
                    CommandAction::StartSession {
                        work_dir: None,
                        provider,
                        args,
                    }
                }
                "/show-thinking" | "/show_thinking" => CommandAction::ShowThinking,
                "/hide-thinking" | "/hide_thinking" => CommandAction::HideThinking,
                "/agent-history" | "/agent_history" => CommandAction::ShowAgentHistory {
                    arg: arg.to_string(),
                },
                "/agents" => {
                    let mut args: Vec<String> =
                        arg.split_whitespace().map(|s| s.to_string()).collect();
                    let provider = parse_provider_prefix(&mut args);
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
                        // Regular text with no active session: do not auto-start.
                        // Require the user to explicitly run /agent to begin a session.
                        CommandAction::Reply(crate::t_fmt!(
                            "forward.failed_send",
                            ERR = crate::t!("controller.no_active_session")
                        ))
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
            CommandAction::StopGeneration => {
                let ctrl = self.controller.lock().await;
                let provider =
                    crate::config::model::AgentProvider::parse_str(&ctrl.provider_name().await);
                if !ctrl.is_busy() {
                    return Some(crate::command::agents::stop_already_idle_message(&provider));
                }
                match ctrl.send_stop_generation().await {
                    Ok(()) => Some(crate::command::agents::stop_sent_message(&provider)),
                    Err(e) => Some(t_fmt!("builtin.failed_stop_generation", ERR = e)),
                }
            }
            CommandAction::ClearSession => {
                let ctrl = self.controller.lock().await;
                match ctrl.clear_session().await {
                    Ok(_) => {
                        drop(ctrl);
                        if let Some(ref channel_id) = self.channel_id {
                            GLOBAL_CHANNEL_SESSIONS
                                .refresh_active_controller_session(channel_id, &self.controller)
                                .await;
                        }
                        Some(t!("builtin.context_cleared").to_string())
                    }
                    Err(e) => Some(t_fmt!("builtin.failed_clear", ERR = e)),
                }
            }
            CommandAction::CompactSession { arg } => {
                let ctrl = self.controller.lock().await;
                let provider =
                    crate::config::model::AgentProvider::parse_str(&ctrl.provider_name().await);
                if !crate::command::agents::provider_supports_context_compact(&provider) {
                    return Some(t_fmt!(
                        "builtin.compact_not_supported",
                        NAME = crate::command::agents::provider_display_name(&provider)
                    ));
                }
                let hint = arg.trim();
                if crate::command::agents::provider_compact_via_user_message(&provider) {
                    let text = if hint.is_empty() {
                        "/compact".to_string()
                    } else {
                        format!("/compact {hint}")
                    };
                    let _ = ctrl.send_stop_generation().await;
                    match ctrl.send_message(&text).await {
                        Ok(()) => None,
                        Err(e) => Some(t_fmt!("builtin.failed_compact", ERR = e)),
                    }
                } else {
                    let instructions = if hint.is_empty() { None } else { Some(hint) };
                    match ctrl.compact_session(instructions).await {
                        Ok(summary) => {
                            Some(crate::command::agents::compact_success_message(&summary))
                        }
                        Err(e) => Some(t_fmt!("builtin.failed_compact", ERR = e)),
                    }
                }
            }
            CommandAction::InitSessionMemory { arg } => {
                let ctrl = self.controller.lock().await;
                let provider =
                    crate::config::model::AgentProvider::parse_str(&ctrl.provider_name().await);
                if !crate::command::agents::provider_supports_memory_init(&provider) {
                    return Some(t_fmt!(
                        "builtin.init_not_supported",
                        NAME = crate::command::agents::provider_display_name(&provider)
                    ));
                }
                let hint = arg.trim();
                let text = if hint.is_empty() {
                    "/init".to_string()
                } else {
                    format!("/init {hint}")
                };
                let _ = ctrl.send_stop_generation().await;
                match ctrl.send_message(&text).await {
                    Ok(()) => None,
                    Err(e) => Some(t_fmt!("builtin.failed_init", ERR = e)),
                }
            }
            CommandAction::Models { arg } => {
                let ctrl = self.controller.lock().await;
                let provider = AgentProvider::parse_str(&ctrl.provider_name().await);
                let provider_name = crate::command::agents::provider_display_name(&provider);
                if models::is_platform_bound_agent(&provider) {
                    return Some(t_fmt!(
                        "models.not_supported_platform_agent",
                        NAME = provider_name
                    ));
                }
                if arg.trim().is_empty() {
                    let mut lines = Vec::new();
                    lines.push(t_fmt!("models.title", NAME = provider_name));
                    let current = ctrl.current_model_id().await;
                    lines.push(models::current_model_line(current.as_deref()));
                    match ctrl.list_available_models().await {
                        Ok(models) if models.is_empty() => {
                            lines.push(t!("models.no_known_models").to_string());
                            lines.push(models::switch_hint_for_provider(&provider));
                        }
                        Ok(models) => {
                            for (i, m) in models.iter().take(20).enumerate() {
                                lines.push(models::format_model_list_entry(
                                    i,
                                    m,
                                    current.as_deref() == Some(m.as_str()),
                                ));
                            }
                            lines.push(models::switch_hint_for_provider(&provider));
                        }
                        Err(e) => lines.push(e.to_string()),
                    }
                    return Some(lines.join("\n"));
                }
                let model_arg = arg.trim().to_string();
                match ctrl.switch_model(&model_arg).await {
                    Ok(canonical) => Some(t_fmt!(
                        "models.switched",
                        NAME = provider_name,
                        MODEL = canonical
                    )),
                    Err(e) => Some(e.to_string()),
                }
            }
            CommandAction::Status => {
                let ctrl = self.controller.lock().await;
                Some(ctrl.status_summary().await)
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

                let items = match crate::command::builtin::list_directory_items(&target) {
                    Ok(items) => items,
                    Err(e) => return Some(t_fmt!("builtin.failed_list_dir", ERR = e)),
                };

                let dirs: Vec<(String, bool)> = items
                    .into_iter()
                    .filter(|(name, is_dir)| *is_dir && !name.starts_with('.'))
                    .collect();

                Some(crate::command::builtin::format_directory_list(
                    &target, &dirs,
                ))
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
                    return Some(agents::format_provider_picker_message(&profiles, &current));
                };
                let name = agents::provider_display_name(&selected);
                match GLOBAL_CHANNEL_SESSIONS.set_channel_default_provider(&channel_id, selected) {
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
                    Some(channel_id) => GLOBAL_CHANNEL_SESSIONS
                        .resolve_start_provider(channel_id, &profiles, provider),
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
            CommandAction::ShowAgentHistory { arg } => Some(self.builtin.agent_history(&arg).await),
            CommandAction::UnknownCommand(_) => Some(self.builtin.help_text()),
            CommandAction::ForwardToAgent(text) => {
                let ctrl = self.controller.lock().await;
                if !ctrl.is_session_active().await {
                    return Some(crate::i18n::dict::tfmt(
                        "forward.no_session",
                        &[("MSG", &text)],
                    ));
                }
                match ctrl.send_message(&text).await {
                    Ok(()) => None,
                    Err(e) => Some(t_fmt!("forward.failed_send", ERR = e)),
                }
            }
            CommandAction::PermissionAllow { request_id } => {
                let ctrl = self.controller.lock().await;
                let id = match request_id {
                    Some(id) => id,
                    None => match ctrl.get_pending_request().await {
                        Some((id, _)) => id,
                        None => {
                            return Some(t!("controller.no_pending_request").to_string());
                        }
                    },
                };
                let msg = build_permission_allow(&id, None);
                match ctrl.send_input(msg).await {
                    Ok(()) => {
                        ctrl.clear_pending_request().await;
                        Some(t_fmt!("controller.permission_allowed", ID = id))
                    }
                    Err(e) => Some(t_fmt!("controller.failed_permission", ERR = e)),
                }
            }
            CommandAction::PermissionDeny { request_id, reason } => {
                let ctrl = self.controller.lock().await;
                let id = match request_id {
                    Some(id) => id,
                    None => match ctrl.get_pending_request().await {
                        Some((id, _)) => id,
                        None => {
                            return Some(t!("controller.no_pending_request").to_string());
                        }
                    },
                };
                let reason = reason.unwrap_or_else(|| "Denied by user".to_string());
                let msg = build_permission_deny(&id, &reason);
                match ctrl.send_input(msg).await {
                    Ok(()) => {
                        ctrl.clear_pending_request().await;
                        Some(t_fmt!("controller.permission_denied", ID = id))
                    }
                    Err(e) => Some(t_fmt!("controller.failed_permission", ERR = e)),
                }
            }
            CommandAction::NoOp => Some(String::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::builtin::BuiltinCommands;
    use crate::config::model::AgentProfiles;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    async fn router_with_active_session(
        default_dir: &str,
    ) -> (CommandRouter, Arc<Mutex<AgentController>>) {
        let ctrl = Arc::new(Mutex::new(AgentController::new(
            AgentProfiles::default(),
            false,
        )));
        ctrl.lock()
            .await
            .test_set_active_with_provider_session_id("test-session".into())
            .await;
        (CommandRouter::new(ctrl.clone(), default_dir), ctrl)
    }

    #[tokio::test]
    async fn compact_routes_in_active_session() {
        let (router, _) = router_with_active_session("/tmp").await;
        assert!(matches!(
            router.route("/compact").await,
            CommandAction::CompactSession { ref arg } if arg.is_empty()
        ));
        assert!(matches!(
            router.route("/compact keep API changes").await,
            CommandAction::CompactSession { ref arg } if arg == "keep API changes"
        ));
    }

    #[tokio::test]
    async fn init_routes_in_active_session() {
        let (router, _) = router_with_active_session("/tmp").await;
        assert!(matches!(
            router.route("/init").await,
            CommandAction::InitSessionMemory { ref arg } if arg.is_empty()
        ));
        assert!(matches!(
            router.route("/init focus on tests").await,
            CommandAction::InitSessionMemory { ref arg } if arg == "focus on tests"
        ));
    }

    #[tokio::test]
    async fn session_mode_slash_commands_stay_on_gateway() {
        let (router, ctrl) = router_with_active_session("/tmp").await;
        let help = BuiltinCommands::new(ctrl, "/tmp").session_help_text();

        match router.route("/help").await {
            CommandAction::Reply(text) => assert_eq!(text, help),
            other => panic!("expected Reply for /help, got {other:?}"),
        }
        match router.route("/pwd").await {
            CommandAction::Reply(text) => assert_eq!(text, help),
            other => panic!("expected Reply for unknown /pwd, got {other:?}"),
        }
        match router.route("/not-a-command").await {
            CommandAction::Reply(text) => assert_eq!(text, help),
            other => panic!("expected Reply for unknown slash, got {other:?}"),
        }
        assert!(matches!(
            router.route("hello agent").await,
            CommandAction::ForwardToAgent(text) if text == "hello agent"
        ));
    }

    #[tokio::test]
    async fn model_alias_routes_like_models_in_active_session() {
        let (router, _) = router_with_active_session("/tmp").await;

        assert!(matches!(
            router.route("/models").await,
            CommandAction::Models { ref arg } if arg.is_empty()
        ));
        assert!(matches!(
            router.route("/model").await,
            CommandAction::Models { ref arg } if arg.is_empty()
        ));
        assert!(matches!(
            router.route("/model sonnet").await,
            CommandAction::Models { ref arg } if arg == "sonnet"
        ));
    }
}
