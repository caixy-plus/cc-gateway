use std::path::PathBuf;

use anyhow::Result;

use crate::claude::mcp_server::McpContext;
use crate::command::router::CommandAction;
use crate::config::model::ClaudeConfig;
use crate::session::channel_manager::{ActiveClaudeRuntime, GLOBAL_CHANNEL_SESSIONS};
use crate::session::channel_model::ClaudeSession;
use crate::{t, t_fmt};

#[derive(Clone)]
pub(crate) struct ChatCommandContext {
    pub(crate) channel_id: String,
    pub(crate) title: String,
    pub(crate) channel_work_dir: String,
    pub(crate) active_claude: Option<ActiveClaudeRuntime>,
    pub(crate) mcp_context: Option<McpContext>,
}

impl ChatCommandContext {
    pub(crate) fn new(
        channel_id: String,
        title: String,
        channel_work_dir: String,
        active_claude: Option<ActiveClaudeRuntime>,
    ) -> Self {
        Self {
            channel_id,
            title,
            channel_work_dir,
            active_claude,
            mcp_context: None,
        }
    }

    pub(crate) fn with_mcp_context(mut self, mcp_context: McpContext) -> Self {
        self.mcp_context = Some(mcp_context);
        self
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
    DirCreated {
        path: String,
        message: String,
    },
    Started {
        active: ActiveClaudeRuntime,
        work_dir: String,
        message: String,
    },
    History {
        sessions: Vec<ClaudeSession>,
    },
    ForwardToClaude {
        active: ActiveClaudeRuntime,
        text: String,
    },
    Error(String),
}

pub(crate) struct ChatCommandExecutor {
    default_dir: String,
    claude_config: ClaudeConfig,
    show_thinking: bool,
}

impl ChatCommandExecutor {
    pub(crate) fn new(default_dir: &str, claude_config: ClaudeConfig, show_thinking: bool) -> Self {
        Self {
            default_dir: default_dir.to_string(),
            claude_config,
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
            CommandAction::UnknownCommand(_) => Ok(ChatCommandOutcome::Reply(
                crate::t!("feishu.unknown_command").to_string(),
            )),
            CommandAction::StopSession => {
                GLOBAL_CHANNEL_SESSIONS
                    .stop_active_runtime_for_channel(
                        &context.channel_id,
                        context.active_claude.as_ref(),
                    )
                    .await?;
                context.active_claude = None;
                Ok(ChatCommandOutcome::Stopped {
                    message: t!("builtin.session_stopped").to_string(),
                })
            }
            CommandAction::ShowThinking => {
                if let Some(ref active) = context.active_claude {
                    let ctrl = active.controller.lock().await;
                    ctrl.set_show_thinking(true);
                }
                Ok(ChatCommandOutcome::ThinkingShown {
                    message: t!("builtin.thinking_enabled").to_string(),
                })
            }
            CommandAction::HideThinking => {
                if let Some(ref active) = context.active_claude {
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
                if let Err(e) = crate::claude::controller::ensure_under_home(&target_str) {
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
            CommandAction::StartSession { work_dir, args } => {
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
                    .start_claude_session_for_platform(
                        &context.channel_id,
                        &context.title,
                        &self.default_dir,
                        self.claude_config.clone(),
                        self.show_thinking,
                        args,
                        None,
                        Some(effective_dir.clone()),
                        context.mcp_context.clone(),
                    )
                    .await
                {
                    Ok(active) => {
                        context.channel_work_dir = active.claude_session.work_dir.clone();
                        context.active_claude = Some(active.clone());
                        Ok(ChatCommandOutcome::Started {
                            active,
                            work_dir: context.channel_work_dir.clone(),
                            message: t_fmt!("builtin.session_started", DIR = effective_dir),
                        })
                    }
                    Err(e) => Ok(ChatCommandOutcome::Error(t_fmt!(
                        "builtin.failed_start_claude",
                        ERR = e
                    ))),
                }
            }
            CommandAction::ShowClaudeHistory { .. } => {
                let sessions = GLOBAL_CHANNEL_SESSIONS
                    .list_claude_sessions_by_channel(&context.channel_id, Some(10));
                Ok(ChatCommandOutcome::History { sessions })
            }
            CommandAction::ForwardToClaude(text) => {
                let active = match context.active_claude.clone() {
                    Some(active) => {
                        let ctrl = active.controller.lock().await;
                        if ctrl.is_session_active().await {
                            drop(ctrl);
                            active
                        } else {
                            drop(ctrl);
                            let work_dir = crate::command::workdir::effective_work_dir(
                                &context.channel_work_dir,
                                &self.default_dir,
                            );
                            match GLOBAL_CHANNEL_SESSIONS
                                .start_claude_session_for_platform(
                                    &context.channel_id,
                                    &context.title,
                                    &self.default_dir,
                                    self.claude_config.clone(),
                                    self.show_thinking,
                                    vec![],
                                    None,
                                    Some(work_dir),
                                    context.mcp_context.clone(),
                                )
                                .await
                            {
                                Ok(active) => {
                                    context.channel_work_dir =
                                        active.claude_session.work_dir.clone();
                                    context.active_claude = Some(active.clone());
                                    active
                                }
                                Err(e) => {
                                    context.active_claude = None;
                                    return Ok(ChatCommandOutcome::Error(t_fmt!(
                                        "builtin.failed_start_claude",
                                        ERR = e
                                    )));
                                }
                            }
                        }
                    }
                    None => {
                        return Ok(ChatCommandOutcome::Error(
                            t!("controller.no_active_session").to_string(),
                        ));
                    }
                };
                Ok(ChatCommandOutcome::ForwardToClaude { active, text })
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
