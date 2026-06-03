use std::path::PathBuf;

use anyhow::Result;

use crate::command::router::CommandAction;
use crate::command::models;
use crate::config::model::{AgentProfiles, AgentProvider};
use crate::runtime::mcp_server::McpContext;
use crate::session::channel_manager::{ActiveAgentRuntime, GLOBAL_CHANNEL_SESSIONS};
use crate::session::channel_model::AgentSession;
use crate::{t, t_fmt};

#[derive(Clone)]
pub(crate) struct ChatCommandContext {
    pub(crate) platform: String,
    pub(crate) channel_id: String,
    pub(crate) title: String,
    pub(crate) channel_work_dir: String,
    pub(crate) active_agent: Option<ActiveAgentRuntime>,
    pub(crate) mcp_context: Option<McpContext>,
}

impl ChatCommandContext {
    pub(crate) fn new(
        platform: impl Into<String>,
        channel_id: String,
        title: String,
        channel_work_dir: String,
        active_agent: Option<ActiveAgentRuntime>,
    ) -> Self {
        Self {
            platform: platform.into(),
            channel_id,
            title,
            channel_work_dir,
            active_agent,
            mcp_context: None,
        }
    }

    pub(crate) fn with_mcp_context(mut self, mcp_context: McpContext) -> Self {
        self.mcp_context = Some(mcp_context);
        self
    }

    fn unknown_command_message(&self) -> String {
        match self.platform.as_str() {
            "feishu" => crate::t!("feishu.unknown_command").to_string(),
            "qq" => crate::t!("qq.unknown_command").to_string(),
            _ => crate::t!("builtin.unknown_command").to_string(),
        }
    }
}

pub(crate) enum ChatCommandOutcome {
    Reply(String),
    NoOp,
    Stopped {
        message: String,
    },
    ThinkingShown {
        message: String,
    },
    ThinkingHidden {
        message: String,
    },
    WorkDirChanged {
        work_dir: String,
        message: String,
    },
    CurrentDir {
        work_dir: String,
        message: String,
    },
    ListDir {
        dir: String,
        dirs: Vec<(String, String)>,
    },
    SelectAgent {
        current: AgentProvider,
        options: Vec<(String, String)>,
    },
    SelectModel {
        /// Current provider (for display).
        provider: AgentProvider,
        /// Known active model id, if any.
        current: Option<String>,
        /// Model ids (provider-specific).
        options: Vec<String>,
    },
    DirCreated {
        path: String,
        message: String,
    },
    Started {
        message: String,
    },
    History {
        sessions: Vec<AgentSession>,
    },
    ForwardToAgent {
        active: ActiveAgentRuntime,
        text: String,
    },
    Error(String),
}

pub(crate) struct ChatCommandExecutor {
    default_dir: String,
    agent_settings: AgentProfiles,
    show_thinking: bool,
}

impl ChatCommandExecutor {
    pub(crate) fn new<C: Into<AgentProfiles>>(
        default_dir: &str,
        agent_settings: C,
        show_thinking: bool,
    ) -> Self {
        Self {
            default_dir: default_dir.to_string(),
            agent_settings: agent_settings.into(),
            show_thinking,
        }
    }

    pub(crate) async fn execute(
        &self,
        context: &mut ChatCommandContext,
        action: CommandAction,
    ) -> Result<ChatCommandOutcome> {
        match action {
            CommandAction::Reply(text) => Ok(ChatCommandOutcome::Reply(text)),
            CommandAction::NoOp => Ok(ChatCommandOutcome::NoOp),
            CommandAction::UnknownCommand(_) => {
                Ok(ChatCommandOutcome::Reply(context.unknown_command_message()))
            }
            CommandAction::StopSession => {
                let stopped_provider = context
                    .active_agent
                    .as_ref()
                    .map(|a| a.agent_session.stored_provider());
                GLOBAL_CHANNEL_SESSIONS
                    .stop_active_runtime_for_channel(
                        &context.channel_id,
                        context.active_agent.as_ref(),
                    )
                    .await?;
                context.active_agent = None;
                let message = stopped_provider
                    .map(|p| crate::command::agents::session_stopped_message(&p))
                    .unwrap_or_else(|| {
                        t_fmt!(
                            "builtin.session_stopped",
                            NAME = t!("builtin.agent_fallback_name")
                        )
                    });
                Ok(ChatCommandOutcome::Stopped { message })
            }
            CommandAction::ShowThinking => {
                if let Some(ref active) = context.active_agent {
                    let ctrl = active.controller.lock().await;
                    ctrl.set_show_thinking(true);
                }
                Ok(ChatCommandOutcome::ThinkingShown {
                    message: t!("builtin.thinking_enabled").to_string(),
                })
            }
            CommandAction::HideThinking => {
                if let Some(ref active) = context.active_agent {
                    let ctrl = active.controller.lock().await;
                    ctrl.set_show_thinking(false);
                }
                Ok(ChatCommandOutcome::ThinkingHidden {
                    message: t!("builtin.thinking_disabled").to_string(),
                })
            }
            CommandAction::ChangeDir(path) => self.change_dir(context, path).await,
            CommandAction::ChangeDirDefault => {
                self.change_dir(context, PathBuf::from(&self.default_dir))
                    .await
            }
            CommandAction::PrintWorkingDir => {
                let work_dir = crate::command::workdir::effective_work_dir(
                    &context.channel_work_dir,
                    &self.default_dir,
                );
                Ok(ChatCommandOutcome::CurrentDir {
                    message: t_fmt!("builtin.current_dir", DIR = work_dir),
                    work_dir,
                })
            }
            CommandAction::ListDir { path } => {
                let requested = path.unwrap_or_else(|| PathBuf::from("."));
                let dir = match crate::command::workdir::resolve_work_dir_target(
                    &context.channel_work_dir,
                    &self.default_dir,
                    &requested,
                ) {
                    Ok(dir) => dir,
                    Err(e) => return Ok(ChatCommandOutcome::Error(e.to_string())),
                };
                match crate::command::builtin::list_directory_paths(&dir) {
                    Ok(dirs) => Ok(ChatCommandOutcome::ListDir { dir, dirs }),
                    Err(e) => Ok(ChatCommandOutcome::Error(t_fmt!(
                        "builtin.failed_list_dir",
                        ERR = e
                    ))),
                }
            }
            CommandAction::MakeDir(path) => {
                let base = crate::command::workdir::effective_work_dir(
                    &context.channel_work_dir,
                    &self.default_dir,
                );
                let target = PathBuf::from(&base).join(&path);
                let target_str = target.to_string_lossy().to_string();
                if let Err(e) = crate::runtime::controller::ensure_under_home(&target_str) {
                    return Ok(ChatCommandOutcome::Error(e.to_string()));
                }
                match std::fs::create_dir_all(&target) {
                    Ok(()) => Ok(ChatCommandOutcome::DirCreated {
                        path: target_str.clone(),
                        message: t_fmt!("builtin.dir_created", PATH = target_str),
                    }),
                    Err(e) => Ok(ChatCommandOutcome::Error(t_fmt!(
                        "builtin.failed_create_dir",
                        ERR = e
                    ))),
                }
            }
            CommandAction::SelectChannelAgent { provider: explicit } => {
                if let Some(selected) = explicit {
                    let name = crate::command::agents::provider_display_name(&selected);
                    match GLOBAL_CHANNEL_SESSIONS
                        .set_channel_default_provider(&context.channel_id, selected)
                    {
                        Ok(()) => Ok(ChatCommandOutcome::Reply(t_fmt!(
                            "builtin.channel_agent_set",
                            NAME = name
                        ))),
                        Err(e) => Ok(ChatCommandOutcome::Error(t_fmt!(
                            "builtin.failed_set_channel_agent",
                            ERR = e
                        ))),
                    }
                } else {
                    let current = GLOBAL_CHANNEL_SESSIONS
                        .effective_channel_provider(&context.channel_id, &self.agent_settings);
                    let options: Vec<(String, String)> =
                        crate::command::agents::available_providers(&self.agent_settings)
                            .into_iter()
                            .map(|p| {
                                (
                                    p.to_string(),
                                    crate::command::agents::provider_display_name(&p).to_string(),
                                )
                            })
                            .collect();
                    Ok(ChatCommandOutcome::SelectAgent { current, options })
                }
            }
            CommandAction::StartSession {
                work_dir,
                provider,
                args,
            } => {
                let resolved_provider = GLOBAL_CHANNEL_SESSIONS.resolve_start_provider(
                    &context.channel_id,
                    &self.agent_settings,
                    provider,
                );
                let provider = Some(resolved_provider.clone());
                let effective_dir = work_dir
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| {
                        crate::command::workdir::effective_work_dir(
                            &context.channel_work_dir,
                            &self.default_dir,
                        )
                    });
                match GLOBAL_CHANNEL_SESSIONS
                    .start_agent_session_for_platform(
                        crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                            channel_id: context.channel_id.clone(),
                            title: context.title.clone(),
                            default_dir: self.default_dir.clone(),
                            agent_settings: self.agent_settings.clone(),
                            show_thinking: self.show_thinking,
                            args,
                            resume_session_id: None,
                            work_dir_override: Some(effective_dir.clone()),
                            mcp_context: context.mcp_context.clone(),
                            provider_override: provider,
                        },
                    )
                    .await
                {
                    Ok(active) => {
                        context.channel_work_dir = active.agent_session.work_dir.clone();
                        context.active_agent = Some(active.clone());
                        Ok(ChatCommandOutcome::Started {
                            message: crate::command::agents::session_started_message(
                                &resolved_provider,
                                &effective_dir,
                            ),
                        })
                    }
                    Err(e) => Ok(ChatCommandOutcome::Error(
                        crate::command::agents::failed_start_agent_message(&resolved_provider, e),
                    )),
                }
            }
            CommandAction::ShowAgentHistory { .. } => {
                let sessions = GLOBAL_CHANNEL_SESSIONS
                    .list_agent_sessions_by_channel(&context.channel_id, Some(10));
                Ok(ChatCommandOutcome::History { sessions })
            }
            CommandAction::FlushQueue { prompt } => match context.active_agent.as_ref() {
                Some(active) => {
                    let ctrl = active.controller.lock().await;
                    let has_buffered = ctrl.has_buffered_messages().await;
                    let busy = ctrl.is_busy();
                    let provider = active.agent_session.stored_provider();
                    if !busy && prompt.is_none() && !has_buffered {
                        return Ok(ChatCommandOutcome::Reply(
                            crate::command::agents::esc_already_idle_message(&provider),
                        ));
                    }
                    if busy || has_buffered {
                        if let Err(e) = ctrl.flush_queued_messages().await {
                            return Ok(ChatCommandOutcome::Error(t_fmt!(
                                "builtin.failed_esc",
                                ERR = e
                            )));
                        }
                    }
                    if let Some(ref text) = prompt {
                        match ctrl.send_message(text).await {
                            Ok(()) => Ok(ChatCommandOutcome::Reply(
                                crate::command::agents::esc_with_prompt_sent_message(
                                    &provider, text,
                                ),
                            )),
                            Err(e) => Ok(ChatCommandOutcome::Error(t_fmt!(
                                "builtin.failed_esc",
                                ERR = e
                            ))),
                        }
                    } else {
                        Ok(ChatCommandOutcome::Reply(
                            crate::command::agents::esc_sent_message(&provider),
                        ))
                    }
                }
                None => Ok(ChatCommandOutcome::Error(
                    t!("controller.no_active_session").to_string(),
                )),
            },
            CommandAction::StopGeneration => match context.active_agent.as_ref() {
                Some(active) => {
                    let ctrl = active.controller.lock().await;
                    let provider = active.agent_session.stored_provider();
                    if !ctrl.is_busy() {
                        return Ok(ChatCommandOutcome::Reply(
                            crate::command::agents::stop_already_idle_message(&provider),
                        ));
                    }
                    match ctrl.send_stop_generation().await {
                        Ok(()) => Ok(ChatCommandOutcome::Reply(
                            crate::command::agents::stop_sent_message(&provider),
                        )),
                        Err(e) => Ok(ChatCommandOutcome::Error(t_fmt!(
                            "builtin.failed_stop_generation",
                            ERR = e
                        ))),
                    }
                }
                None => Ok(ChatCommandOutcome::Error(
                    t!("controller.no_active_session").to_string(),
                )),
            },
            CommandAction::Status => {
                let summary = match context.active_agent.as_ref() {
                    Some(active) => {
                        let ctrl = active.controller.lock().await;
                        ctrl.status_summary().await
                    }
                    None => t!("builtin.status_no_session").to_string(),
                };
                Ok(ChatCommandOutcome::Reply(summary))
            }
            CommandAction::ClearSession => match context.active_agent.as_ref() {
                Some(active) => {
                    let agent_session_id = active.agent_session.id.clone();
                    let ctrl = active.controller.lock().await;
                    match ctrl.clear_session().await {
                        Ok(_) => {
                            drop(ctrl);
                            GLOBAL_CHANNEL_SESSIONS
                                .refresh_agent_session_from_controller(
                                    &agent_session_id,
                                    &active.controller,
                                )
                                .await;
                            Ok(ChatCommandOutcome::Reply(
                                t!("builtin.context_cleared").to_string(),
                            ))
                        }
                        Err(e) => Ok(ChatCommandOutcome::Error(t_fmt!(
                            "builtin.failed_clear",
                            ERR = e
                        ))),
                    }
                }
                None => Ok(ChatCommandOutcome::Error(
                    t!("controller.no_active_session").to_string(),
                )),
            },
            CommandAction::Models { arg } => match context.active_agent.as_ref() {
                Some(active) => {
                    let provider = active.agent_session.stored_provider();
                    let provider_name =
                        crate::command::agents::provider_display_name(&provider);
                    if models::is_platform_bound_agent(&provider) {
                        return Ok(ChatCommandOutcome::Reply(t_fmt!(
                            "models.not_supported_platform_agent",
                            NAME = provider_name
                        )));
                    }
                    if arg.trim().is_empty() {
                        let ctrl = active.controller.lock().await;
                        let current = ctrl.current_model_id().await;
                        let mut opts = match ctrl.list_available_models().await {
                            Ok(opts) => opts,
                            Err(e) => {
                                return Ok(ChatCommandOutcome::Reply(e.to_string()));
                            }
                        };
                        // Keep UX safe: avoid huge keyboards/cards.
                        if opts.len() > 20 {
                            opts.truncate(20);
                        }
                        if opts.is_empty() {
                            // Fallback: show text hints.
                            let mut lines = Vec::new();
                            lines.push(t_fmt!(
                                "models.title",
                                NAME = crate::command::agents::provider_display_name(&provider)
                            ));
                            lines.push(models::current_model_line(current.as_deref()));
                            lines.push(t!("models.no_known_models").to_string());
                            lines.push(t!("models.switch_hint_raw").to_string());
                            return Ok(ChatCommandOutcome::Reply(lines.join("\n")));
                        }
                        return Ok(ChatCommandOutcome::SelectModel {
                            provider,
                            current,
                            options: opts,
                        });
                    }

                    let agent_session_id = active.agent_session.id.clone();
                    let ctrl = active.controller.lock().await;
                    let model_id = arg.trim().to_string();
                    match ctrl.switch_model(&model_id).await {
                        Ok(()) => {
                            drop(ctrl);
                            GLOBAL_CHANNEL_SESSIONS
                                .refresh_agent_session_from_controller(
                                    &agent_session_id,
                                    &active.controller,
                                )
                                .await;
                            Ok(ChatCommandOutcome::Reply(t_fmt!(
                                "models.switched",
                                NAME = provider_name,
                                MODEL = model_id
                            )))
                        }
                        Err(e) => Ok(ChatCommandOutcome::Reply(e.to_string())),
                    }
                }
                None => Ok(ChatCommandOutcome::Error(
                    t!("controller.no_active_session").to_string(),
                )),
            },
            CommandAction::ForwardToAgent(text) => {
                let active = match context.active_agent.clone() {
                    Some(active) => {
                        let ctrl = active.controller.lock().await;
                        if ctrl.is_session_active().await {
                            drop(ctrl);
                            active
                        } else {
                            drop(ctrl);
                            // Session died (e.g. daemon restart): try to resume it.
                            let session_id = active.agent_session.id.clone();
                            match resume_or_fallback(self, context, Some(&session_id)).await {
                                Ok(a) => a,
                                Err(e) => {
                                    return Ok(ChatCommandOutcome::Error(e.to_string()))
                                }
                            }
                        }
                    }
                    None => {
                        // No active agent: try to resume the latest session for this
                        // channel, or start a new one.
                        match resume_or_fallback(self, context, None).await {
                            Ok(a) => a,
                            Err(e) => {
                                return Ok(ChatCommandOutcome::Error(e.to_string()))
                            }
                        }
                    }
                };
                Ok(ChatCommandOutcome::ForwardToAgent { active, text })
            }
            CommandAction::PermissionAllow { request_id } => {
                let id = match request_id {
                    Some(id) => id,
                    None => match context.active_agent.as_ref() {
                        Some(active) => {
                            let ctrl = active.controller.lock().await;
                            match ctrl.get_pending_request().await {
                                Some((id, _)) => id,
                                None => {
                                    return Ok(ChatCommandOutcome::Reply(
                                        t!("controller.no_pending_request").to_string(),
                                    ));
                                }
                            }
                        }
                        None => {
                            return Ok(ChatCommandOutcome::Error(
                                t!("controller.no_active_session").to_string(),
                            ));
                        }
                    },
                };
                match context.active_agent.as_ref() {
                    Some(active) => {
                        let ctrl = active.controller.lock().await;
                        let msg = crate::runtime::protocol::build_permission_allow(&id, None);
                        match ctrl.send_input(msg).await {
                            Ok(()) => {
                                ctrl.clear_pending_request().await;
                                Ok(ChatCommandOutcome::Reply(t_fmt!(
                                    "controller.permission_allowed",
                                    ID = id
                                )))
                            }
                            Err(e) => Ok(ChatCommandOutcome::Error(t_fmt!(
                                "controller.failed_permission",
                                ERR = e
                            ))),
                        }
                    }
                    None => Ok(ChatCommandOutcome::Error(
                        t!("controller.no_active_session").to_string(),
                    )),
                }
            }
            CommandAction::PermissionDeny { request_id, reason } => {
                let id = match request_id {
                    Some(id) => id,
                    None => match context.active_agent.as_ref() {
                        Some(active) => {
                            let ctrl = active.controller.lock().await;
                            match ctrl.get_pending_request().await {
                                Some((id, _)) => id,
                                None => {
                                    return Ok(ChatCommandOutcome::Reply(
                                        t!("controller.no_pending_request").to_string(),
                                    ));
                                }
                            }
                        }
                        None => {
                            return Ok(ChatCommandOutcome::Error(
                                t!("controller.no_active_session").to_string(),
                            ));
                        }
                    },
                };
                let reason = reason.unwrap_or_else(|| "Denied by user".to_string());
                match context.active_agent.as_ref() {
                    Some(active) => {
                        let ctrl = active.controller.lock().await;
                        let msg = crate::runtime::protocol::build_permission_deny(&id, &reason);
                        match ctrl.send_input(msg).await {
                            Ok(()) => {
                                ctrl.clear_pending_request().await;
                                Ok(ChatCommandOutcome::Reply(t_fmt!(
                                    "controller.permission_denied",
                                    ID = id
                                )))
                            }
                            Err(e) => Ok(ChatCommandOutcome::Error(t_fmt!(
                                "controller.failed_permission",
                                ERR = e
                            ))),
                        }
                    }
                    None => Ok(ChatCommandOutcome::Error(
                        t!("controller.no_active_session").to_string(),
                    )),
                }
            }
        }
    }

    async fn change_dir(
        &self,
        context: &mut ChatCommandContext,
        path: PathBuf,
    ) -> Result<ChatCommandOutcome> {
        let work_dir = match crate::command::workdir::resolve_work_dir_target(
            &context.channel_work_dir,
            &self.default_dir,
            &path,
        ) {
            Ok(path) => path,
            Err(e) => return Ok(ChatCommandOutcome::Error(e.to_string())),
        };
        GLOBAL_CHANNEL_SESSIONS
            .switch_work_dir(&context.channel_id, PathBuf::from(&work_dir))
            .await?;
        context.channel_work_dir = work_dir.clone();
        Ok(ChatCommandOutcome::WorkDirChanged {
            message: t_fmt!("builtin.dir_changed", PATH = work_dir),
            work_dir,
        })
    }
}

/// Try to resume an existing session; only falls back to a new session when
/// the channel has no previous sessions to resume.
///
/// When `resume_session_id` is Some, it names a specific session to resume
/// (e.g. when a previously-active session's process has died).
/// When None, we look for the most recent session in this channel.
///
/// Resume failures are returned as errors so the user sees the reason rather
/// than silently getting a new session that lacks conversation history.
async fn resume_or_fallback(
    executor: &ChatCommandExecutor,
    context: &mut ChatCommandContext,
    resume_session_id: Option<&str>,
) -> Result<ActiveAgentRuntime> {
    let work_dir = crate::command::workdir::effective_work_dir(
        &context.channel_work_dir,
        &executor.default_dir,
    );
    let provider = GLOBAL_CHANNEL_SESSIONS.resolve_start_provider(
        &context.channel_id,
        &executor.agent_settings,
        None,
    );

    // 1) Try to resume a named session.
    if let Some(sid) = resume_session_id {
        match GLOBAL_CHANNEL_SESSIONS
            .resume_agent_session_for_platform(
                sid,
                &executor.default_dir,
                executor.agent_settings.clone(),
                executor.show_thinking,
                Some(work_dir.clone()),
                context.mcp_context.clone(),
            )
            .await
        {
            Ok(active) => {
                context.channel_work_dir = active.agent_session.work_dir.clone();
                context.active_agent = Some(active.clone());
                return Ok(active);
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to resume session {} for channel {}: {}",
                    sid,
                    context.channel_id,
                    e
                );
                let detail = crate::command::agents::friendly_spawn_error(&e.to_string());
                anyhow::bail!(crate::t_fmt!(
                    "builtin.failed_resume_session",
                    ERR = detail
                ))
            }
        }
    }

    // 2) If no session was specified, try to resume the latest session for this channel.
    if resume_session_id.is_none() {
        let sessions = GLOBAL_CHANNEL_SESSIONS
            .list_agent_sessions_by_channel(&context.channel_id, Some(1));
        if let Some(latest) = sessions.first() {
            match GLOBAL_CHANNEL_SESSIONS
                .resume_agent_session_for_platform(
                    &latest.id,
                    &executor.default_dir,
                    executor.agent_settings.clone(),
                    executor.show_thinking,
                    Some(work_dir.clone()),
                    context.mcp_context.clone(),
                )
                .await
            {
                Ok(active) => {
                    context.channel_work_dir = active.agent_session.work_dir.clone();
                    context.active_agent = Some(active.clone());
                    return Ok(active);
                }
                Err(e) => {
                    let detail = crate::command::agents::friendly_spawn_error(&e.to_string());
                    tracing::warn!(
                        "Failed to auto-resume latest session {} for channel {}: {}",
                        latest.id,
                        context.channel_id,
                        e
                    );
                    anyhow::bail!(crate::t_fmt!(
                        "builtin.failed_resume_session",
                        ERR = detail
                    ))
                }
            }
        }
    }

    // 3) No existing session to resume: start a brand-new session.
    match GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(
            crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                channel_id: context.channel_id.clone(),
                title: context.title.clone(),
                default_dir: executor.default_dir.clone(),
                agent_settings: executor.agent_settings.clone(),
                show_thinking: executor.show_thinking,
                args: vec![],
                resume_session_id: None,
                work_dir_override: Some(work_dir),
                mcp_context: context.mcp_context.clone(),
                provider_override: Some(provider.clone()),
            },
        )
        .await
    {
        Ok(active) => {
            context.channel_work_dir = active.agent_session.work_dir.clone();
            context.active_agent = Some(active.clone());
            Ok(active)
        }
        Err(e) => {
            context.active_agent = None;
            anyhow::bail!(crate::command::agents::failed_start_agent_message(
                &provider, e
            ))
        }
    }
}
