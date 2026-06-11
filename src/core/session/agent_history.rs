//! Shared `/agent-history` actions: list, resume, start-new-in-work-dir, delete.

use super::channel_manager::{
    ActiveAgentRuntime, StartAgentSessionForPlatformArgs, GLOBAL_CHANNEL_SESSIONS,
};
use super::channel_model::AgentSession;
use crate::command::agents;
use crate::config::model::AgentProfiles;
use crate::runtime::mcp_server::McpContext;
use crate::t;

const LIST_LIMIT: usize = 10;

#[derive(Clone)]
pub struct AgentHistoryEnv {
    pub default_dir: String,
    pub agent_settings: AgentProfiles,
    pub show_thinking: bool,
}

#[derive(Clone)]
pub struct AgentHistoryRequest {
    pub channel_id: String,
    pub title: String,
    pub mcp_context: Option<McpContext>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentHistoryAction {
    List,
    Resume { session_id: String },
    StartNew { work_dir: String },
    Delete { session_id: String },
    ByIndex { index: usize, start_new: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentHistoryStartKind {
    Resumed,
    New,
}

pub enum AgentHistoryOutcome {
    List {
        sessions: Vec<AgentSession>,
    },
    Started {
        active: ActiveAgentRuntime,
        message: String,
        kind: AgentHistoryStartKind,
    },
    Deleted {
        success: bool,
        message: String,
    },
    Error {
        message: String,
    },
}

pub async fn run(
    env: &AgentHistoryEnv,
    req: &AgentHistoryRequest,
    action: AgentHistoryAction,
) -> AgentHistoryOutcome {
    match action {
        AgentHistoryAction::List => AgentHistoryOutcome::List {
            sessions: GLOBAL_CHANNEL_SESSIONS
                .list_agent_sessions_by_channel(&req.channel_id, Some(LIST_LIMIT)),
        },
        AgentHistoryAction::Resume { session_id } => resume(env, req, &session_id).await,
        AgentHistoryAction::StartNew { work_dir } => start_new(env, req, work_dir).await,
        AgentHistoryAction::Delete { session_id } => delete(&session_id),
        AgentHistoryAction::ByIndex { index, start_new } => {
            by_index(env, req, index, start_new).await
        }
    }
}

pub(crate) fn parse_history_arg(arg: &str) -> Result<(usize, bool), String> {
    let tokens: Vec<&str> = arg.split_whitespace().collect();
    let (start_new, idx_token) = match tokens.as_slice() {
        [idx] => (false, *idx),
        [idx, "new"] | [idx, "--new"] => (true, *idx),
        ["new", idx] | ["--new", idx] => (true, *idx),
        _ => return Err(t!("builtin.invalid_history_index").to_string()),
    };
    idx_token
        .parse::<usize>()
        .map(|index| (index, start_new))
        .map_err(|_| t!("builtin.invalid_history_index").to_string())
}

async fn by_index(
    env: &AgentHistoryEnv,
    req: &AgentHistoryRequest,
    index: usize,
    start_new_flag: bool,
) -> AgentHistoryOutcome {
    let sessions = GLOBAL_CHANNEL_SESSIONS.list_agent_sessions_by_channel(&req.channel_id, Some(LIST_LIMIT));
    if index == 0 || index > sessions.len() {
        return AgentHistoryOutcome::Error {
            message: t!("builtin.invalid_history_index").to_string(),
        };
    }
    let target = &sessions[index - 1];
    if start_new_flag {
        start_new(env, req, target.work_dir.clone()).await
    } else {
        resume(env, req, &target.id).await
    }
}

async fn resume(
    env: &AgentHistoryEnv,
    req: &AgentHistoryRequest,
    session_id: &str,
) -> AgentHistoryOutcome {
    let work_dir_override = GLOBAL_CHANNEL_SESSIONS
        .get_agent_session(session_id)
        .map(|s| s.work_dir);
    match GLOBAL_CHANNEL_SESSIONS
        .resume_agent_session_for_platform(
            session_id,
            &env.default_dir,
            env.agent_settings.clone(),
            env.show_thinking,
            work_dir_override,
            req.mcp_context.clone(),
            None,
        )
        .await
    {
        Ok(active) => {
            let provider = active.agent_session.stored_provider();
            AgentHistoryOutcome::Started {
                message: agents::session_restarted_message(
                    &provider,
                    &active.agent_session.work_dir,
                ),
                kind: AgentHistoryStartKind::Resumed,
                active,
            }
        }
        Err(e) => {
            let provider = GLOBAL_CHANNEL_SESSIONS
                .get_agent_session(session_id)
                .map(|s| s.stored_provider())
                .unwrap_or(env.agent_settings.default.clone());
            AgentHistoryOutcome::Error {
                message: agents::failed_start_agent_message(&provider, e),
            }
        }
    }
}

async fn start_new(
    env: &AgentHistoryEnv,
    req: &AgentHistoryRequest,
    work_dir: String,
) -> AgentHistoryOutcome {
    let provider = GLOBAL_CHANNEL_SESSIONS.resolve_start_provider(
        &req.channel_id,
        &env.agent_settings,
        None,
    );
    match GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(StartAgentSessionForPlatformArgs {
            channel_id: req.channel_id.clone(),
            title: req.title.clone(),
            default_dir: env.default_dir.clone(),
            agent_settings: env.agent_settings.clone(),
            show_thinking: env.show_thinking,
            args: vec![],
            resume_session_id: None,
            work_dir_override: Some(work_dir),
            mcp_context: req.mcp_context.clone(),
            provider_override: Some(provider.clone()),
        })
        .await
    {
        Ok(active) => {
            let started_provider = active.agent_session.stored_provider();
            AgentHistoryOutcome::Started {
                message: agents::session_started_message(
                    &started_provider,
                    &active.agent_session.work_dir,
                ),
                kind: AgentHistoryStartKind::New,
                active,
            }
        }
        Err(e) => AgentHistoryOutcome::Error {
            message: agents::failed_start_agent_message(&provider, e),
        },
    }
}

fn delete(session_id: &str) -> AgentHistoryOutcome {
    let success = GLOBAL_CHANNEL_SESSIONS.remove_agent_session(session_id);
    let message = if success {
        t!("builtin.session_deleted").to_string()
    } else {
        t!("builtin.cannot_delete_active").to_string()
    };
    AgentHistoryOutcome::Deleted { success, message }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_history_arg_accepts_new_suffix() {
        assert_eq!(parse_history_arg("2 new").unwrap(), (2, true));
        assert_eq!(parse_history_arg("3 --new").unwrap(), (3, true));
        assert_eq!(parse_history_arg("new 4").unwrap(), (4, true));
        assert_eq!(parse_history_arg("1").unwrap(), (1, false));
    }

    #[test]
    fn parse_history_arg_rejects_garbage() {
        assert!(parse_history_arg("x y").is_err());
    }
}
