//! Qoder CLI CN ACP client (`qoderclicn --acp`, based on stdio NDJSON JSON-RPC protocol).
//!
//! # Design Key Points
//!
//! - Uses **standard ACP** protocol: Reuses [`GenericAcpSession`] + a thin [`AcpHooks`] implementation.
//!   All transport, spawn, prompt, permission, and session update mappings are provided by `acp_session.rs`.
//!   This file only describes the **differences** between Qoder and other ACP providers (Gemini/Codex/Kimi, etc.).
//! - **Authentication**: Reuses CLI credentials cache (user previously ran `qoderclicn login`) or reads the environment variable
//!   `QODER_PERSONAL_ACCESS_TOKEN`. Therefore, [`AcpHooks::authenticate_method_id`] returns `None`,
//!   and the gateway will not send an `authenticate` RPC to Qoder. This is consistent with Gemini and Codex.
//!   If the user has not logged in, Qoder will return a clear "Please login first" error during the `session/new` stage, which the gateway transparently forwards.
//! - **Spawn argv**: `qoderclicn --acp` (uses a flag format, consistent with Gemini, **not** the subcommand `acp`).
//! - **Session Resume**: Employs the generic ACP `session/load` + `provider_session_id` mechanism.
//!   If it fails, falls back to `session/new` (consistent with Gemini/Codex).
//! - **Model Switching**: Tries `session/set_model` first. If Qoder returns `Method not found`,
//!   falls back to `session/set_config_option { configId: "model", value }` (consistent with Kimi).
//! - **MCP**: Uses the standard ACP `mcpServers` field inside `session/new`
//!   ([`build_acp_mcp_servers`]). If Qoder has not implemented this field yet, the ACP specification requires the server to ignore unknown fields,
//!   which prevents spawn failures.
//! - **`--yolo`**: The gateway maps `--yolo` in `default_args` to `permission: allow` semantics,
//!   and then **strips** it from the CLI args, so there is no need to manually append `--yolo` in argv here.

use anyhow::Result;
use serde_json::{json, Value};

use crate::agent::acp_session::{build_base_spawn_args, AcpHooks, GenericAcpSession};
use crate::agent::mcp_attach::build_acp_mcp_servers;
use crate::config::model::AgentConfig;
use crate::runtime::mcp_server::McpContext;

/// Qoder ACP session type.
///
/// All transport, prompt, permission, and lifecycle behaviors are inherited directly from
/// [`GenericAcpSession`]; differences are injected via [`QoderAcpHooks`].
pub type QoderAcpSession = GenericAcpSession<QoderAcpHooks>;

/// Hook implementation for the Qoder provider.
///
/// Implemented as `Copy + Default` because [`GenericAcpSession`] needs to duplicate the hook instance in several internal paths
/// (e.g., inside the asynchronous stdout reader after spawn); using a unit struct ensures zero cost.
#[derive(Debug, Clone, Copy, Default)]
pub struct QoderAcpHooks;

#[async_trait::async_trait]
impl AcpHooks for QoderAcpHooks {
    /// Provider name used in logs and user-visible errors.
    fn log_provider_name(&self) -> &'static str {
        "Qoder"
    }

    /// Skip the ACP `authenticate` RPC — Qoder uses cached CLI credentials or PAT environment variables.
    ///
    /// If `None` is returned, [`GenericAcpSession`] will not call `authenticate` during the session setup stage,
    /// avoiding errors in Qoder due to unknown methodId (consistent with Gemini behavior).
    fn authenticate_method_id(&self) -> Option<&str> {
        None
    }

    /// Default label displayed for permission requests in the WebUI / bot.
    fn default_permission_label(&self) -> &'static str {
        "qoder_permission"
    }

    /// Error description on the gateway side when the prompt channel is closed (child process exited).
    fn prompt_channel_closed_error(&self) -> &'static str {
        "Qoder ACP prompt response channel closed"
    }

    /// User hint when spawning fails (e.g., `qoderclicn` is not on PATH or exits abnormally).
    ///
    /// Displays both `config.cli_path` and the actual `cli_path` resolved, facilitating troubleshooting
    /// when the user has modified `cli_path` in `config.json`.
    fn spawn_failure_message(config: &AgentConfig, cli_path: &str) -> String {
        format!(
            "Failed to spawn Qoder CLI CN. Is '{}' installed and on PATH? Tried '{} --acp'.",
            config.cli_path, cli_path
        )
    }

    /// User-visible error when session resume fails, localized via i18n.
    fn session_resume_error(session_id: &str, err: &str) -> anyhow::Error {
        anyhow::anyhow!(
            "{}",
            crate::t_fmt!("qoder.session_resume_failed", ID = session_id, ERR = err)
        )
    }

    /// Normalizes the user's `work_dir` under `$HOME` to prevent Qoder from encountering permission errors
    /// when writing session files in system directories.
    fn normalize_work_dir(work_dir: &str) -> Result<String> {
        crate::runtime::controller::ensure_under_home(work_dir)
    }

    /// Converts the gateway's MCP context (MCP server list attached to the current chat) into the ACP standard
    /// `mcpServers` JSON field; returns an empty object `{}` if no servers are attached.
    async fn prepare_mcp_servers(
        _work_dir: &str,
        mcp_context: Option<&McpContext>,
    ) -> Result<Value> {
        build_acp_mcp_servers(mcp_context)
    }

    /// Assembles the final argv for `qoderclicn`:
    ///
    /// ```text
    /// <tokenized default_args> <extra_args> --acp
    /// ```
    ///
    /// `--acp` is placed at the **end** to guarantee Qoder starts in ACP mode; `default_args` and
    /// `extra_args` have already been filtered and normalized by [`build_base_spawn_args`].
    fn build_spawn_args(
        config: &AgentConfig,
        extra_args: Vec<String>,
        _mcp_servers: &Value,
    ) -> Vec<String> {
        let mut args = build_base_spawn_args(config, extra_args);
        args.push("--acp".to_string());
        args
    }

    /// Qoder currently has no extension notifications that require special handling by the gateway, so it returns `false`
    /// (allowing [`GenericAcpSession`] to walk the default notification processing route).
    fn handle_extension_notification(
        &self,
        _method: &str,
        _msg: &Value,
        _ctx: &crate::agent::acp_session::AcpNotifyCtx,
    ) -> bool {
        false
    }

    /// Qoder supports switching models in-session, executed by [`set_session_model`].
    fn supports_acp_set_model(&self) -> bool {
        true
    }

    /// In-session model switching: Tries `session/set_model` first, falling back to `session/set_config_option`
    /// if it fails with a `Method not found` error.
    ///
    /// The dual-path fallback is identical to Kimi's behavior: Qoder's ACP implementation might change the method name across versions.
    /// Supporting both styles ensures forward and backward compatibility when switching models.
    async fn set_session_model(&self, session: &QoderAcpSession, model_id: &str) -> Result<()> {
        let session_id = session.acp_session_id();
        let set_model_params = json!({
            "sessionId": session_id,
            "modelId": model_id
        });
        match session
            .acp_request("session/set_model", set_model_params.clone())
            .await
        {
            Ok(_) => Ok(()),
            Err(e) if e.to_string().contains("Method not found") => {
                session
                    .acp_request(
                        "session/set_config_option",
                        json!({
                            "sessionId": session_id,
                            "configId": "model",
                            "value": model_id
                        }),
                    )
                    .await?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::AgentProvider;

    /// The argv must use the `--acp` **flag**, not the `acp` positional subcommand (consistent with Gemini).
    #[test]
    fn spawn_args_use_acp_flag_not_positional_subcommand() {
        let config = AgentConfig::default_for_provider(AgentProvider::Qoder);
        let args = QoderAcpHooks::build_spawn_args(&config, vec![], &Value::Null);
        assert_eq!(args, vec!["--acp".to_string()]);
        assert!(!args.contains(&"acp".to_string()));
    }

    /// Both `default_args` and `extra_args` should be preserved and placed before the `--acp` flag.
    ///
    /// Note: a neutral flag is used here on purpose. In production `--yolo` is stripped by
    /// `parse_gateway_default_args` (→ `permission: allow`) before this runs and is re-applied as
    /// `--permission-mode bypass_permissions`, so passing `--yolo` literally here would misrepresent
    /// the real argv. The yolo path is covered by
    /// [`qoder_yolo_maps_to_permission_mode_bypass_permissions`] and
    /// [`qoder_webui_yolo_chip_produces_real_bypass_permissions_argv`].
    #[test]
    fn spawn_args_keep_default_and_extra_args_before_acp_flag() {
        let mut config = AgentConfig::default_for_provider(AgentProvider::Qoder);
        config.default_args = "--sandbox".to_string();
        let args = QoderAcpHooks::build_spawn_args(
            &config,
            vec!["-m".into(), "auto".into()],
            &Value::Null,
        );
        assert_eq!(args, vec!["--sandbox", "-m", "auto", "--acp"]);
    }

    /// Verifies that no `authenticate` RPC is sent to Qoder.
    #[test]
    fn authenticate_is_skipped() {
        assert_eq!(AcpHooks::authenticate_method_id(&QoderAcpHooks), None);
    }

    /// Session resume errors must include both the session ID and the raw error, allowing users to locate issues in chat.
    #[test]
    fn qoder_session_resume_error_is_user_visible() {
        let err = QoderAcpHooks::session_resume_error("sess-1", "timeout");
        let msg = err.to_string();
        assert!(msg.contains("sess-1"));
        assert!(msg.contains("timeout"));
    }

    /// The default CLI binary name must be `qoderclicn` (the name registered on PATH after Qoder CLI CN installation).
    #[test]
    fn qoder_default_cli_path_is_qoderclicn() {
        let config = AgentConfig::default_for_provider(AgentProvider::Qoder);
        assert_eq!(config.cli_path, "qoderclicn");
    }

    /// Gateway-level `--yolo` (which gets stripped and turned into
    /// `permission: allow`) must be re-applied to the spawned qoderclicn as
    /// `--permission-mode bypass_permissions`. qoderclicn does **not** accept
    /// `--yolo`; this is the only way to get ACP mode to stop emitting
    /// `session/request_permission` notifications.
    #[test]
    fn qoder_yolo_maps_to_permission_mode_bypass_permissions() {
        use crate::agent::acp_session::build_base_spawn_args;
        let mut config = AgentConfig::default_for_provider(AgentProvider::Qoder);
        config.permission = "allow".to_string();
        let args = build_base_spawn_args(&config, vec![]);
        assert!(
            args.windows(2)
                .any(|w| w == ["--permission-mode", "bypass_permissions"]),
            "expected `--permission-mode bypass_permissions` to be injected, got: {args:?}"
        );
    }

    /// When the user did NOT pass `--yolo` (i.e. `permission` stays at the
    /// default `prompt`), qoderclicn must be spawned **without**
    /// `--permission-mode bypass_permissions` — the user's choice to be
    /// asked for permission must be preserved.
    #[test]
    fn qoder_prompt_mode_does_not_inject_bypass() {
        use crate::agent::acp_session::build_base_spawn_args;
        let config = AgentConfig::default_for_provider(AgentProvider::Qoder);
        assert_eq!(config.permission, "prompt");
        let args = build_base_spawn_args(&config, vec![]);
        assert!(
            !args.iter().any(|a| a == "--permission-mode"),
            "did not expect `--permission-mode` when permission is prompt, got: {args:?}"
        );
    }

    /// If the user has already written `--permission-mode bypass_permissions`
    /// into `default_args` themselves, `build_base_spawn_args` must not
    /// duplicate the token pair.
    #[test]
    fn qoder_explicit_bypass_permissions_is_not_duplicated() {
        use crate::agent::acp_session::build_base_spawn_args;
        let mut config = AgentConfig::default_for_provider(AgentProvider::Qoder);
        config.permission = "allow".to_string();
        config.default_args = "--permission-mode bypass_permissions".to_string();
        let args = build_base_spawn_args(&config, vec![]);
        let count = args
            .windows(2)
            .filter(|w| *w == ["--permission-mode", "bypass_permissions"])
            .count();
        assert_eq!(count, 1, "expected exactly one occurrence, got: {args:?}");
    }

    /// Non-Qoder providers (Gemini as representative) with `permission: allow`
    /// must NOT accidentally pick up `--permission-mode bypass_permissions`
    /// from the Qoder-specific capability.
    #[test]
    fn gemini_allow_does_not_inject_qoder_tokens() {
        use crate::agent::acp_session::build_base_spawn_args;
        let mut config = AgentConfig::default_for_provider(AgentProvider::Gemini);
        config.permission = "allow".to_string();
        let args = build_base_spawn_args(&config, vec![]);
        assert!(
            !args.iter().any(|a| a == "--permission-mode"),
            "Gemini must not get Qoder's bypass flag, got: {args:?}"
        );
    }

    /// End-to-end: simulate a user clicking the `--yolo` chip in WebUI
    /// (writing `default_args = "--yolo"` into the qoder profile). The
    /// gateway strips `--yolo`, resolves `permission: allow`, and then
    /// `build_base_spawn_args` injects `--permission-mode bypass_permissions`
    /// for qoderclicn. Final argv must contain the real qoderclicn flag and
    /// must NOT contain `--yolo` (qoderclicn does not recognize it).
    #[test]
    fn qoder_webui_yolo_chip_produces_real_bypass_permissions_argv() {
        use crate::agent::acp_session::build_base_spawn_args;
        use crate::config::model::AgentProfiles;

        let mut profiles = AgentProfiles::default();
        let profile = profiles.profile_mut(&AgentProvider::Qoder);
        profile.enabled = true;
        profile.default_args = Some("--yolo".to_string());

        let config = profiles
            .config_for_provider(Some(AgentProvider::Qoder))
            .expect("qoder profile must resolve");
        // Gateway must have mapped `--yolo` → `permission: allow` and stripped `--yolo`.
        assert_eq!(config.permission, "allow");
        assert!(!config.default_args.contains("--yolo"));

        let args = build_base_spawn_args(&config, vec![]);
        assert!(
            args.windows(2)
                .any(|w| w == ["--permission-mode", "bypass_permissions"]),
            "expected `--permission-mode bypass_permissions` in final argv, got: {args:?}"
        );
        assert!(
            !args.iter().any(|a| a == "--yolo"),
            "`--yolo` must never reach qoderclicn, got: {args:?}"
        );
    }
}
