//! Canonical list of chat platforms integrated into cc-gateway.
//!
//! Drives daemon startup, WebUI `GET /api/platforms`, pairing, connection status,
//! init wizard menus, and config restart policy — new platforms add a registry entry
//! plus `src/platform/<name>/` implementation.

use anyhow::Result;
use serde_json::{json, Value};

use super::model::{AgentProfiles, FeishuConfig, GatewayConfig, TelegramConfig};
use super::secrets::{is_masked_secret, mask_secret};
use crate::platform::Platform;
use crate::platform::{feishu::FeishuPlatform, telegram::TelegramPlatform};
use crate::session::channel_model::SessionSource;
use crate::session::pairing::GLOBAL_PAIRING_MANAGER;
use tracing::error;

/// How the platform connects to the vendor API (documentation / WebUI label).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformTransport {
    WsProtobuf,
    LongPoll,
    WsJson,
}

impl PlatformTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WsProtobuf => "ws_protobuf",
            Self::LongPoll => "long_poll",
            Self::WsJson => "ws_json",
        }
    }
}

/// Settings field type for WebUI / init wizard rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformFieldKind {
    Bool,
    Text,
    Secret,
}

/// One editable key in `config.json` → `platforms.<id>.<key>`.
#[derive(Debug, Clone, Copy)]
pub struct PlatformFieldDef {
    pub key: &'static str,
    pub kind: PlatformFieldKind,
    /// WebUI i18n suffix under `settings.` (e.g. `app_id` → `settings.app_id`).
    pub label_key: &'static str,
    pub hint_key: Option<&'static str>,
    /// Prompt during `cc-gateway init` bot setup (text/secret fields only).
    pub wizard: bool,
}

pub type PlatformApplySettingsFn = fn(&mut GatewayConfig, &Value, &dyn Fn(&str, &str) -> bool);

/// Feature flags exposed to WebUI and integration docs.
#[derive(Debug, Clone, Copy)]
pub struct PlatformCapabilities {
    pub mcp_send_file: bool,
    pub interactive_ll: bool,
    pub interactive_agent_picker: bool,
    pub interactive_model_picker: bool,
}

/// Read/write hooks for typed `GatewayConfig` platform sections (Phase 1).
#[derive(Clone, Copy)]
pub struct PlatformConfigHooks {
    pub is_enabled: fn(&GatewayConfig) -> bool,
    pub set_enabled: fn(&mut GatewayConfig, bool),
    pub require_pairing: fn(&GatewayConfig) -> bool,
    pub set_require_pairing: fn(&mut GatewayConfig, bool),
    pub diff_config: PlatformConfigDiffFn,
    pub restart_paths: &'static [&'static str],
    pub live_paths: &'static [&'static str],
}

pub type PlatformConfigDiffFn =
    fn(&GatewayConfig, &GatewayConfig, &mut Vec<String>, &mut Vec<String>);

pub struct PlatformSpawnCtx<'a> {
    pub config: &'a GatewayConfig,
    pub default_dir: &'a str,
    pub agent_profiles: AgentProfiles,
    pub show_thinking: bool,
}

pub type PlatformSpawnFn = fn(PlatformSpawnCtx<'_>) -> Result<Box<dyn Platform>>;

pub type PlatformRunHandle = (Box<dyn Platform>, tokio::task::JoinHandle<()>);

#[derive(Clone)]
pub struct PlatformDef {
    pub id: &'static str,
    pub display_name: &'static str,
    pub session_source: SessionSource,
    pub transport: PlatformTransport,
    pub capabilities: PlatformCapabilities,
    pub config: PlatformConfigHooks,
    pub settings_fields: &'static [PlatformFieldDef],
    pub config_to_json: fn(&GatewayConfig) -> Value,
    pub apply_settings: PlatformApplySettingsFn,
    pub mask_secrets_in_json: fn(&mut Value),
    pub spawn: PlatformSpawnFn,
}

const FEISHU_FIELDS: &[PlatformFieldDef] = &[
    PlatformFieldDef {
        key: "enabled",
        kind: PlatformFieldKind::Bool,
        label_key: "enabled",
        hint_key: None,
        wizard: false,
    },
    PlatformFieldDef {
        key: "require_pairing",
        kind: PlatformFieldKind::Bool,
        label_key: "require_pairing",
        hint_key: Some("require_pairing_hint"),
        wizard: false,
    },
    PlatformFieldDef {
        key: "app_id",
        kind: PlatformFieldKind::Text,
        label_key: "app_id",
        hint_key: None,
        wizard: true,
    },
    PlatformFieldDef {
        key: "app_secret",
        kind: PlatformFieldKind::Secret,
        label_key: "app_secret",
        hint_key: None,
        wizard: true,
    },
];

const TELEGRAM_FIELDS: &[PlatformFieldDef] = &[
    PlatformFieldDef {
        key: "enabled",
        kind: PlatformFieldKind::Bool,
        label_key: "enabled",
        hint_key: None,
        wizard: false,
    },
    PlatformFieldDef {
        key: "require_pairing",
        kind: PlatformFieldKind::Bool,
        label_key: "require_pairing",
        hint_key: Some("require_pairing_hint"),
        wizard: false,
    },
    PlatformFieldDef {
        key: "bot_token",
        kind: PlatformFieldKind::Secret,
        label_key: "bot_token",
        hint_key: None,
        wizard: true,
    },
    PlatformFieldDef {
        key: "proxy",
        kind: PlatformFieldKind::Text,
        label_key: "telegram_proxy",
        hint_key: Some("telegram_proxy_hint"),
        wizard: false,
    },
];

pub const PLATFORM_DEFS: &[PlatformDef] = &[
    PlatformDef {
        id: "feishu",
        display_name: "Feishu / Lark",
        session_source: SessionSource::Feishu,
        transport: PlatformTransport::WsProtobuf,
        capabilities: PlatformCapabilities {
            mcp_send_file: true,
            interactive_ll: true,
            interactive_agent_picker: true,
            interactive_model_picker: true,
        },
        config: PlatformConfigHooks {
            is_enabled: feishu_is_enabled,
            set_enabled: feishu_set_enabled,
            require_pairing: feishu_require_pairing,
            set_require_pairing: feishu_set_require_pairing,
            diff_config: diff_feishu_config,
            restart_paths: &["feishu.enabled", "feishu.app_id", "feishu.app_secret"],
            live_paths: &["feishu.require_pairing"],
        },
        settings_fields: FEISHU_FIELDS,
        config_to_json: feishu_config_to_json,
        apply_settings: apply_feishu_settings,
        mask_secrets_in_json: mask_feishu_secrets_json,
        spawn: spawn_feishu,
    },
    PlatformDef {
        id: "telegram",
        display_name: "Telegram",
        session_source: SessionSource::Telegram,
        transport: PlatformTransport::LongPoll,
        capabilities: PlatformCapabilities {
            mcp_send_file: true,
            interactive_ll: false,
            interactive_agent_picker: false,
            interactive_model_picker: false,
        },
        config: PlatformConfigHooks {
            is_enabled: telegram_is_enabled,
            set_enabled: telegram_set_enabled,
            require_pairing: telegram_require_pairing,
            set_require_pairing: telegram_set_require_pairing,
            diff_config: diff_telegram_config,
            restart_paths: &["telegram.enabled", "telegram.bot_token", "telegram.proxy"],
            live_paths: &["telegram.require_pairing"],
        },
        settings_fields: TELEGRAM_FIELDS,
        config_to_json: telegram_config_to_json,
        apply_settings: apply_telegram_settings,
        mask_secrets_in_json: mask_telegram_secrets_json,
        spawn: spawn_telegram,
    },
];

/// Init wizard: numeric menu index (1-based) or platform id.
pub fn def_by_menu_choice(choice: &str) -> Option<&'static PlatformDef> {
    if let Ok(n) = choice.parse::<usize>() {
        if n >= 1 && n <= PLATFORM_DEFS.len() {
            return PLATFORM_DEFS.get(n - 1);
        }
    }
    def_by_id(choice)
}

pub fn def_by_id(id: &str) -> Option<&'static PlatformDef> {
    let key = id.trim().to_ascii_lowercase();
    PLATFORM_DEFS.iter().find(|d| d.id == key)
}

pub fn session_source_for_platform(id: &str) -> SessionSource {
    def_by_id(id)
        .map(|d| d.session_source.clone())
        .unwrap_or(SessionSource::WebUI)
}

pub fn apply_pairing_flags_from_config(config: &GatewayConfig) {
    for def in PLATFORM_DEFS {
        GLOBAL_PAIRING_MANAGER.set_require_pairing(def.id, (def.config.require_pairing)(config));
    }
}

pub fn spawn_enabled_platforms(config: &GatewayConfig) -> Result<Vec<Box<dyn Platform>>> {
    let mut out = Vec::new();
    for def in PLATFORM_DEFS {
        if !(def.config.is_enabled)(config) {
            continue;
        }
        out.push((def.spawn)(platform_spawn_ctx(config))?);
    }
    Ok(out)
}

/// Spawn `run()` tasks for each enabled platform; returns handles for graceful shutdown.
pub fn start_enabled_platforms(config: &GatewayConfig) -> Result<Vec<PlatformRunHandle>> {
    let mut out = Vec::new();
    for def in PLATFORM_DEFS {
        if !(def.config.is_enabled)(config) {
            continue;
        }
        let platform = (def.spawn)(platform_spawn_ctx(config))?;
        let runner = platform.clone_for_run();
        let def_id = def.id;
        let handle = tokio::spawn(async move {
            if let Err(e) = runner.run().await {
                error!("{def_id} platform error: {e}");
            }
        });
        out.push((platform, handle));
    }
    Ok(out)
}

fn platform_spawn_ctx(config: &GatewayConfig) -> PlatformSpawnCtx<'_> {
    PlatformSpawnCtx {
        config,
        default_dir: &config.default_dir,
        agent_profiles: config.effective_agent_settings(),
        show_thinking: config.show_thinking,
    }
}

pub fn set_require_pairing_in_config(config: &mut GatewayConfig, id: &str, required: bool) -> bool {
    let Some(def) = def_by_id(id) else {
        return false;
    };
    (def.config.set_require_pairing)(config, required);
    true
}

fn field_kind_str(kind: PlatformFieldKind) -> &'static str {
    match kind {
        PlatformFieldKind::Bool => "bool",
        PlatformFieldKind::Text => "text",
        PlatformFieldKind::Secret => "secret",
    }
}

fn fields_schema_json(def: &PlatformDef) -> Value {
    let fields: Vec<Value> = def
        .settings_fields
        .iter()
        .map(|f| {
            json!({
                "key": f.key,
                "kind": field_kind_str(f.kind),
                "label_key": f.label_key,
                "hint_key": f.hint_key,
            })
        })
        .collect();
    Value::Array(fields)
}

pub fn mask_platform_secrets_in_config(config: &mut GatewayConfig) {
    config.platforms.feishu.app_secret = mask_secret(&config.platforms.feishu.app_secret);
    config.platforms.telegram.bot_token = mask_secret(&config.platforms.telegram.bot_token);
}

pub fn apply_platforms_from_json(
    config: &mut GatewayConfig,
    platforms: &serde_json::Map<String, Value>,
    is_masked: &dyn Fn(&str, &str) -> bool,
) {
    for def in PLATFORM_DEFS {
        let Some(section) = platforms.get(def.id) else {
            continue;
        };
        (def.apply_settings)(config, section, is_masked);
    }
}

/// Legacy WebUI saves may still POST top-level `feishu` / `telegram`.
pub fn apply_legacy_platform_sections_from_json(
    config: &mut GatewayConfig,
    body: &Value,
    is_masked: &dyn Fn(&str, &str) -> bool,
) {
    for def in PLATFORM_DEFS {
        let Some(section) = body.get(def.id) else {
            continue;
        };
        (def.apply_settings)(config, section, is_masked);
    }
}

pub fn build_platforms_api_response(config: &GatewayConfig) -> Value {
    let platforms: Vec<Value> = PLATFORM_DEFS
        .iter()
        .map(|def| {
            let enabled = (def.config.is_enabled)(config);
            let state = if enabled {
                crate::platform::status::get_state(def.id).as_str()
            } else {
                "off"
            };
            json!({
                "name": def.id,
                "id": def.id,
                "display_name": def.display_name,
                "enabled": enabled,
                "state": state,
                "transport": def.transport.as_str(),
                "require_pairing": GLOBAL_PAIRING_MANAGER.require_pairing(def.id),
                "capabilities": {
                    "mcp_send_file": def.capabilities.mcp_send_file,
                    "interactive_ll": def.capabilities.interactive_ll,
                    "interactive_agent_picker": def.capabilities.interactive_agent_picker,
                    "interactive_model_picker": def.capabilities.interactive_model_picker,
                },
                "fields": fields_schema_json(def),
                "config": (def.config_to_json)(config),
            })
        })
        .collect();
    json!({ "platforms": platforms })
}

pub fn daemon_restart_field_paths() -> Vec<&'static str> {
    let mut paths = vec![
        "port",
        "bind_address",
        "allowed_ips",
        "webui_token",
        "default_dir",
        "show_thinking",
        "media_retention_days",
        "session_retention_per_channel",
        "log",
        "agent",
    ];
    for def in PLATFORM_DEFS {
        paths.extend(def.config.restart_paths);
    }
    paths
}

pub fn live_field_paths() -> Vec<&'static str> {
    let mut paths = Vec::new();
    for def in PLATFORM_DEFS {
        paths.extend(def.config.live_paths);
    }
    paths
}

pub fn assess_platform_config_changes(
    before: &GatewayConfig,
    after: &GatewayConfig,
    restart_fields: &mut Vec<String>,
    live_fields: &mut Vec<String>,
) {
    for def in PLATFORM_DEFS {
        (def.config.diff_config)(before, after, restart_fields, live_fields);
    }
}

// --- Feishu config hooks ---

fn feishu_is_enabled(c: &GatewayConfig) -> bool {
    c.platforms.feishu.enabled
}
fn feishu_set_enabled(c: &mut GatewayConfig, v: bool) {
    c.platforms.feishu.enabled = v;
}
fn feishu_require_pairing(c: &GatewayConfig) -> bool {
    c.platforms.feishu.require_pairing
}
fn feishu_set_require_pairing(c: &mut GatewayConfig, v: bool) {
    c.platforms.feishu.require_pairing = v;
}
fn feishu_config_to_json(c: &GatewayConfig) -> Value {
    serde_json::to_value(&c.platforms.feishu).unwrap_or(Value::Null)
}
fn apply_feishu_settings(
    config: &mut GatewayConfig,
    section: &Value,
    _is_masked: &dyn Fn(&str, &str) -> bool,
) {
    let Ok(mut incoming) = serde_json::from_value::<FeishuConfig>(section.clone()) else {
        return;
    };
    if is_masked_secret(&incoming.app_secret, &config.platforms.feishu.app_secret) {
        incoming.app_secret = config.platforms.feishu.app_secret.clone();
    }
    config.platforms.feishu = incoming;
}
fn mask_feishu_secrets_json(value: &mut Value) {
    if let Some(secret) = value
        .get("app_secret")
        .and_then(|v| v.as_str())
        .map(mask_secret)
    {
        if let Some(obj) = value.as_object_mut() {
            obj.insert("app_secret".to_string(), json!(secret));
        }
    }
}
fn diff_feishu_config(
    before: &GatewayConfig,
    after: &GatewayConfig,
    restart: &mut Vec<String>,
    live: &mut Vec<String>,
) {
    let b = &before.platforms.feishu;
    let a = &after.platforms.feishu;
    if b.enabled != a.enabled {
        restart.push("feishu.enabled".to_string());
    }
    if b.app_id != a.app_id {
        restart.push("feishu.app_id".to_string());
    }
    if b.app_secret != a.app_secret {
        restart.push("feishu.app_secret".to_string());
    }
    if b.require_pairing != a.require_pairing {
        live.push("feishu.require_pairing".to_string());
    }
}
fn spawn_feishu(ctx: PlatformSpawnCtx<'_>) -> Result<Box<dyn Platform>> {
    Ok(Box::new(FeishuPlatform::new(
        ctx.config.platforms.feishu.clone(),
        ctx.default_dir,
        ctx.agent_profiles,
        ctx.show_thinking,
    )))
}

// --- Telegram config hooks ---

fn telegram_is_enabled(c: &GatewayConfig) -> bool {
    c.platforms.telegram.enabled
}
fn telegram_set_enabled(c: &mut GatewayConfig, v: bool) {
    c.platforms.telegram.enabled = v;
}
fn telegram_require_pairing(c: &GatewayConfig) -> bool {
    c.platforms.telegram.require_pairing
}
fn telegram_set_require_pairing(c: &mut GatewayConfig, v: bool) {
    c.platforms.telegram.require_pairing = v;
}
fn telegram_config_to_json(c: &GatewayConfig) -> Value {
    serde_json::to_value(&c.platforms.telegram).unwrap_or(Value::Null)
}
fn apply_telegram_settings(
    config: &mut GatewayConfig,
    section: &Value,
    _is_masked: &dyn Fn(&str, &str) -> bool,
) {
    let Ok(mut incoming) = serde_json::from_value::<TelegramConfig>(section.clone()) else {
        return;
    };
    if is_masked_secret(&incoming.bot_token, &config.platforms.telegram.bot_token) {
        incoming.bot_token = config.platforms.telegram.bot_token.clone();
    }
    config.platforms.telegram = incoming;
}
fn mask_telegram_secrets_json(value: &mut Value) {
    if let Some(token) = value
        .get("bot_token")
        .and_then(|v| v.as_str())
        .map(mask_secret)
    {
        if let Some(obj) = value.as_object_mut() {
            obj.insert("bot_token".to_string(), json!(token));
        }
    }
}
fn diff_telegram_config(
    before: &GatewayConfig,
    after: &GatewayConfig,
    restart: &mut Vec<String>,
    live: &mut Vec<String>,
) {
    let b = &before.platforms.telegram;
    let a = &after.platforms.telegram;
    if b.enabled != a.enabled {
        restart.push("telegram.enabled".to_string());
    }
    if b.bot_token != a.bot_token {
        restart.push("telegram.bot_token".to_string());
    }
    if b.proxy != a.proxy {
        restart.push("telegram.proxy".to_string());
    }
    if b.require_pairing != a.require_pairing {
        live.push("telegram.require_pairing".to_string());
    }
}
fn spawn_telegram(ctx: PlatformSpawnCtx<'_>) -> Result<Box<dyn Platform>> {
    Ok(Box::new(TelegramPlatform::new(
        ctx.config.platforms.telegram.clone(),
        ctx.default_dir,
        ctx.agent_profiles,
        ctx.show_thinking,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lists_all_integrated_platforms() {
        assert_eq!(PLATFORM_DEFS.len(), 2);
        assert!(def_by_id("feishu").is_some());
        assert!(def_by_id("telegram").is_some());
    }

    #[test]
    fn session_source_mapping() {
        assert_eq!(
            session_source_for_platform("telegram"),
            SessionSource::Telegram
        );
        assert_eq!(session_source_for_platform("unknown"), SessionSource::WebUI);
    }

    #[test]
    fn platforms_api_includes_disabled_platforms() {
        let config = GatewayConfig::default();
        let body = build_platforms_api_response(&config);
        let platforms = body.get("platforms").unwrap().as_array().unwrap();
        assert_eq!(platforms.len(), PLATFORM_DEFS.len());
    }

    #[test]
    fn pairing_only_change_is_live_not_restart() {
        let before = GatewayConfig::default();
        let mut after = before.clone();
        after.platforms.feishu.require_pairing = false;
        let mut restart = Vec::new();
        let mut live = Vec::new();
        assess_platform_config_changes(&before, &after, &mut restart, &mut live);
        assert!(restart.is_empty());
        assert!(live.contains(&"feishu.require_pairing".to_string()));
    }
}
