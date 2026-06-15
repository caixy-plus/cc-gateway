use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::config::model::{AgentConfig, AgentProvider};

/// Platform-bound agents (Cursor) use vendor-tied models; cc-gateway does not expose `/models`.
pub fn is_platform_bound_agent(provider: &AgentProvider) -> bool {
    crate::config::agent_registry::capabilities_for(provider).platform_bound
}

/// User-visible hint for how `/models <arg>` applies to the active provider.
pub fn switch_hint_for_provider(provider: &AgentProvider) -> String {
    let caps = crate::config::agent_registry::capabilities_for(provider);
    if caps.model_switch_via_user_message {
        return crate::t!("models.switch_hint_claude").to_string();
    }
    if caps.in_session_model_switch {
        return crate::t!("models.switch_hint_raw").to_string();
    }
    crate::t!("models.switch_hint_index").to_string()
}

/// Stable Claude Code aliases used only when CLI/settings discovery returns nothing.
/// Version-pinned ids (`claude-opus-4-8`, …) are intentionally omitted — users can
/// `/models <id>` directly; Claude Code validates the `/model` command.
pub fn claude_model_alias_fallback() -> &'static [&'static str] {
    &[
        "default",
        "best",
        "opus",
        "sonnet",
        "haiku",
        "fable",
        "opusplan",
        "sonnet[1m]",
        "opus[1m]",
    ]
}

/// @deprecated alias for tests — prefer [`claude_model_alias_fallback`].
pub fn curated_claude_models() -> &'static [&'static str] {
    claude_model_alias_fallback()
}

/// Human-readable line for the active model in `/models` output.
pub fn current_model_line(current: Option<&str>) -> String {
    match current {
        Some(m) => crate::t_fmt!("models.current_active", MODEL = m),
        None => crate::t!("models.current_default").to_string(),
    }
}

pub fn format_model_list_entry(index: usize, model_id: &str, is_current: bool) -> String {
    if is_current {
        format!("{}. {} ✓", index + 1, model_id)
    } else {
        format!("{}. {}", index + 1, model_id)
    }
}

/// Read `--model` / `-m` from provider spawn args (best-effort).
pub fn extract_model_from_args(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--model" || arg == "-m" {
            if let Some(model) = iter.next() {
                let model = model.trim();
                if !model.is_empty() {
                    return Some(model.to_string());
                }
            }
        } else if let Some(rest) = arg.strip_prefix("--model=") {
            let rest = rest.trim();
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
    }
    None
}

/// List models using provider's official CLI or registry-curated catalog.
pub async fn list_models_via_cli(
    provider: &AgentProvider,
    config: &AgentConfig,
) -> Result<Vec<String>> {
    match provider {
        AgentProvider::Cursor => list_cursor_models(config).await,
        AgentProvider::OpenCode => list_opencode_models(config).await,
        AgentProvider::Claude | AgentProvider::Pi | AgentProvider::Codex | AgentProvider::Kimi
        | AgentProvider::Gemini | AgentProvider::Qoder => Ok(vec![]),
    }
}

/// Gateway-maintained / discovered model catalog for providers without a stable list CLI.
pub async fn list_discovered_models(
    provider: &AgentProvider,
    config: &AgentConfig,
    work_dir: &str,
) -> Vec<String> {
    match provider {
        AgentProvider::Claude => list_claude_models(config, work_dir).await,
        _ => vec![],
    }
}

/// Discover Claude models: CLI probe → `availableModels` in settings → stable alias fallback.
pub async fn list_claude_models(config: &AgentConfig, work_dir: &str) -> Vec<String> {
    let mut set = BTreeSet::new();

    #[cfg(not(test))]
    if let Ok(cli_models) = try_list_claude_models_via_cli(config).await {
        set.extend(cli_models);
    }
    set.extend(models_from_claude_settings(work_dir));

    if set.is_empty() {
        set.extend(
            claude_model_alias_fallback()
                .iter()
                .map(|s| s.to_string()),
        );
    }

    set.into_iter().collect()
}

fn claude_settings_candidates(work_dir: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if !work_dir.trim().is_empty() {
        let project = PathBuf::from(work_dir);
        paths.push(project.join(".claude").join("settings.json"));
        paths.push(project.join(".claude").join("settings.local.json"));
    }
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".claude").join("settings.json"));
        paths.push(home.join(".claude").join("settings.local.json"));
    }
    paths
}

fn models_from_claude_settings(work_dir: &str) -> Vec<String> {
    let mut set = BTreeSet::new();
    for path in claude_settings_candidates(work_dir) {
        merge_available_models_from_settings_file(&path, &mut set);
    }
    set.into_iter().collect()
}

fn merge_available_models_from_settings_file(path: &Path, out: &mut BTreeSet<String>) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<Value>(&content) else {
        return;
    };
    let Some(arr) = value.get("availableModels").and_then(|v| v.as_array()) else {
        return;
    };
    for item in arr {
        if let Some(s) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) {
            out.insert(s.to_string());
        }
    }
}

async fn try_list_claude_models_via_cli(config: &AgentConfig) -> Result<Vec<String>> {
    let cli_path = crate::runtime::session::resolve_cli_path(&config.cli_path);
    const PROBES: &[&[&str]] = &[
        &["models"],
        &["model", "list"],
        &["--list-models"],
    ];
    const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
    for args in PROBES {
        let Some(stdout) = run_cli_probe_collect(&cli_path, args, PROBE_TIMEOUT).await else {
            continue;
        };
        let models = extract_claude_models_from_stdout(&stdout);
        if !models.is_empty() {
            return Ok(models);
        }
    }
    Ok(vec![])
}

/// Short-lived CLI probe: kills the child if it does not finish within `timeout`.
async fn run_cli_probe_collect(
    cli_path: &str,
    args: &[&str],
    timeout: Duration,
) -> Option<String> {
    let mut cmd = crate::core::agent::agent_command(cli_path);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().ok()?;
    let pid = child.id();
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) if out.status.success() => {
            Some(String::from_utf8_lossy(&out.stdout).to_string())
        }
        _ => {
            kill_probe_by_pid(pid);
            None
        }
    }
}

fn kill_probe_by_pid(pid: Option<u32>) {
    let Some(pid) = pid else {
        return;
    };
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status();
    }
}

fn looks_like_claude_model_alias(token: &str) -> bool {
    let t = token.trim();
    matches!(
        t,
        "default" | "best" | "opus" | "sonnet" | "haiku" | "fable" | "opusplan"
    ) || ((t.starts_with("sonnet") || t.starts_with("opus")) && t.ends_with("[1m]"))
}

fn extract_claude_models_from_stdout(stdout: &str) -> Vec<String> {
    let mut set = BTreeSet::new();
    for line in stdout.lines() {
        for raw in line.split_whitespace() {
            let m = normalize_model_token(raw);
            if looks_like_model_token(&m) || looks_like_claude_model_alias(&m) {
                set.insert(m);
            }
        }
    }
    set.into_iter().collect()
}

/// Resolve `/models <arg>` for providers that forward `/model` to Claude Code.
///
/// The picker list is advisory; any non-empty alias/id is accepted and validated by Claude CLI.
pub fn resolve_claude_model_arg(arg: &str, options: &[String]) -> Result<String> {
    let arg = arg.trim();
    if arg.is_empty() {
        anyhow::bail!("{}", crate::t!("models.invalid_index"));
    }
    if arg.chars().all(|c| c.is_ascii_digit()) {
        return resolve_model_arg(arg, options);
    }
    if options.iter().any(|o| o == arg) {
        return Ok(arg.to_string());
    }
    Ok(arg.to_string())
}

/// Provider-aware `/models <arg>` resolution.
pub fn resolve_model_switch_arg(
    provider: &AgentProvider,
    arg: &str,
    options: &[String],
) -> Result<String> {
    if crate::config::agent_registry::capabilities_for(provider).model_switch_via_user_message {
        resolve_claude_model_arg(arg, options)
    } else {
        resolve_model_arg(arg, options)
    }
}

/// @deprecated — use [`list_discovered_models`].
pub fn list_curated_models(provider: &AgentProvider) -> Vec<String> {
    match provider {
        AgentProvider::Claude => claude_model_alias_fallback()
            .iter()
            .map(|s| s.to_string())
            .collect(),
        _ => vec![],
    }
}

fn looks_like_model_token(token: &str) -> bool {
    let t = token.trim();
    if t.is_empty() {
        return false;
    }
    // Model ids typically contain `/` (opencode) or start with well-known prefixes.
    t.contains('/')
        || t.starts_with("gpt-")
        || t.starts_with("claude-")
        || t.starts_with("gemini-")
        || t.starts_with("kimi-")
        || t.starts_with("grok-")
}

fn normalize_model_token(token: &str) -> String {
    token
        .trim()
        .trim_matches(|c: char| c == ',' || c == '"' || c == '\'' || c == '`')
        .to_string()
}

fn extract_models_from_stdout(stdout: &str) -> Vec<String> {
    let mut set = BTreeSet::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Skip common headings.
        let lower = line.to_ascii_lowercase();
        if lower.contains("available models")
            || lower.starts_with("models")
            || lower.starts_with("provider")
            || lower.starts_with("name")
        {
            continue;
        }
        for raw in line.split_whitespace() {
            if looks_like_model_token(raw) {
                let m = normalize_model_token(raw);
                if looks_like_model_token(&m) {
                    set.insert(m);
                }
            }
        }
    }
    set.into_iter().collect()
}

async fn run_cli_and_collect(cli_path: &str, args: &[&str]) -> Result<String> {
    let out = crate::core::agent::agent_command(cli_path)
        .args(args)
        .output()
        .await
        .with_context(|| format!("Failed to run {} {:?}", cli_path, args))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        anyhow::bail!("{} {:?} failed: {}", cli_path, args, stderr.trim());
    }
    Ok(stdout)
}

async fn list_cursor_models(config: &AgentConfig) -> Result<Vec<String>> {
    let cli_path = crate::runtime::session::resolve_cli_path(&config.cli_path);
    // Prefer official flag; fall back to subcommand.
    let stdout = match run_cli_and_collect(&cli_path, &["--list-models"]).await {
        Ok(s) => s,
        Err(_) => run_cli_and_collect(&cli_path, &["models"]).await?,
    };
    let models = extract_models_from_stdout(&stdout);
    Ok(models)
}

async fn list_opencode_models(config: &AgentConfig) -> Result<Vec<String>> {
    let cli_path = crate::runtime::session::resolve_cli_path(&config.cli_path);
    let stdout = run_cli_and_collect(&cli_path, &["models"]).await?;
    Ok(extract_models_from_stdout(&stdout))
}

pub fn parse_provider_model_id(model_id: &str) -> Option<(String, String)> {
    let trimmed = model_id.trim();
    let (provider, rest) = trimmed.split_once('/')?;
    let provider = provider.trim();
    let rest = rest.trim();
    if provider.is_empty() || rest.is_empty() {
        return None;
    }
    Some((provider.to_string(), rest.to_string()))
}

/// Canonical catalog id (same shape as OpenCode CLI `provider/model` lines).
pub fn canonical_model_id(provider: &str, model_id: &str) -> String {
    format!("{}/{}", provider.trim(), model_id.trim())
}

/// Parse a Pi RPC `Model` JSON value into `(provider, modelId)`.
pub fn pi_model_from_json(value: &serde_json::Value) -> Option<(String, String)> {
    if let Some(s) = value.as_str() {
        return parse_provider_model_id(s);
    }
    let provider = value
        .get("provider")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let model_id = value
        .get("id")
        .or_else(|| value.get("modelId"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    Some((provider.to_string(), model_id.to_string()))
}

pub fn canonical_from_pi_model_json(value: &serde_json::Value) -> Option<String> {
    pi_model_from_json(value).map(|(p, id)| canonical_model_id(&p, &id))
}

/// Resolve `/models <arg>` against the latest model list (1-based index or `provider/model` id).
pub fn resolve_model_arg(arg: &str, options: &[String]) -> Result<String> {
    let arg = arg.trim();
    if arg.is_empty() {
        anyhow::bail!("{}", crate::t!("models.invalid_index"));
    }
    if options.iter().any(|o| o == arg) {
        return Ok(arg.to_string());
    }
    if let Ok(index) = arg.parse::<usize>() {
        if index == 0 || index > options.len() {
            anyhow::bail!("{}", crate::t!("models.invalid_index"));
        }
        return Ok(options[index - 1].clone());
    }
    if parse_provider_model_id(arg).is_some() {
        let suffix_matches: Vec<_> = options
            .iter()
            .filter(|o| o.as_str() == arg || o.ends_with(&format!("/{arg}")))
            .collect();
        if suffix_matches.len() == 1 {
            return Ok(suffix_matches[0].clone());
        }
        if suffix_matches.is_empty() {
            return Ok(arg.to_string());
        }
    }
    anyhow::bail!("{}", crate::t_fmt!("models.switch_failed", ERR = arg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_models_from_cli_stdout() {
        let stdout = r#"
Available models:
anthropic/claude-sonnet-4-20250514
openai/gpt-5.1
name  description
gemini-3-flash
"#;
        let models = extract_models_from_stdout(stdout);
        assert!(models.contains(&"anthropic/claude-sonnet-4-20250514".to_string()));
        assert!(models.contains(&"openai/gpt-5.1".to_string()));
        assert!(models.contains(&"gemini-3-flash".to_string()));
    }

    #[test]
    fn parses_provider_model_id() {
        let parsed = parse_provider_model_id("anthropic/claude-sonnet-4-20250514").unwrap();
        assert_eq!(parsed.0, "anthropic");
        assert_eq!(parsed.1, "claude-sonnet-4-20250514");
    }

    #[test]
    fn claude_alias_fallback_includes_core_aliases() {
        let models = claude_model_alias_fallback();
        assert!(models.contains(&"opus"));
        assert!(models.contains(&"sonnet"));
        assert!(!models.iter().any(|m| m.starts_with("claude-opus-4")));
    }

    #[test]
    fn reads_available_models_from_claude_settings_json() {
        let dir = tempfile::tempdir().unwrap();
        let settings_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&settings_dir).unwrap();
        std::fs::write(
            settings_dir.join("settings.json"),
            r#"{"availableModels":["sonnet","claude-sonnet-4-6","haiku"]}"#,
        )
        .unwrap();
        let models = models_from_claude_settings(dir.path().to_str().unwrap());
        assert!(models.contains(&"sonnet".to_string()));
        assert!(models.contains(&"claude-sonnet-4-6".to_string()));
    }

    #[test]
    fn resolve_claude_model_arg_accepts_unknown_alias() {
        let options = vec!["opus".to_string(), "sonnet".to_string()];
        assert_eq!(
            resolve_claude_model_arg("brand-new-alias", &options).unwrap(),
            "brand-new-alias"
        );
        assert_eq!(
            resolve_claude_model_arg("claude-opus-4-9", &options).unwrap(),
            "claude-opus-4-9"
        );
    }

    #[test]
    fn resolve_claude_model_arg_still_resolves_index() {
        let options = vec!["opus".to_string(), "sonnet".to_string()];
        assert_eq!(resolve_claude_model_arg("2", &options).unwrap(), "sonnet");
    }

    #[test]
    fn list_curated_models_for_claude() {
        let models = list_curated_models(&AgentProvider::Claude);
        assert!(models.contains(&"opus".to_string()));
        assert!(list_curated_models(&AgentProvider::Pi).is_empty());
    }

    #[test]
    fn switch_hint_for_claude_uses_in_session_copy() {
        let hint = switch_hint_for_provider(&AgentProvider::Claude);
        assert!(hint.contains("/model"));
        let pi_hint = switch_hint_for_provider(&AgentProvider::Pi);
        assert!(pi_hint.contains("model_id") || pi_hint.contains("模型"));
    }

    #[test]
    fn platform_bound_agents_are_cursor_only() {
        assert!(!is_platform_bound_agent(&AgentProvider::Claude));
        assert!(is_platform_bound_agent(&AgentProvider::Cursor));
        assert!(!is_platform_bound_agent(&AgentProvider::OpenCode));
        assert!(!is_platform_bound_agent(&AgentProvider::Pi));
    }

    #[test]
    fn canonical_model_id_formats_provider_slash_model() {
        assert_eq!(
            canonical_model_id("anthropic", "claude-sonnet-4"),
            "anthropic/claude-sonnet-4"
        );
    }

    #[test]
    fn resolve_model_arg_accepts_index_and_canonical_id() {
        let options = vec![
            "openai/gpt-4".to_string(),
            "anthropic/claude-sonnet-4".to_string(),
        ];
        assert_eq!(
            resolve_model_arg("2", &options).unwrap(),
            "anthropic/claude-sonnet-4"
        );
        assert_eq!(
            resolve_model_arg("anthropic/claude-sonnet-4", &options).unwrap(),
            "anthropic/claude-sonnet-4"
        );
    }

    #[test]
    fn pi_model_from_json_reads_provider_and_id() {
        let m = serde_json::json!({"provider": "anthropic", "id": "claude-sonnet-4"});
        assert_eq!(
            canonical_from_pi_model_json(&m).as_deref(),
            Some("anthropic/claude-sonnet-4")
        );
    }

    #[test]
    fn extracts_model_from_spawn_args() {
        let args = vec![
            "--model".to_string(),
            "anthropic/claude-sonnet-4-20250514".to_string(),
        ];
        assert_eq!(
            extract_model_from_args(&args).as_deref(),
            Some("anthropic/claude-sonnet-4-20250514")
        );
        assert_eq!(
            extract_model_from_args(&["--model=openai/gpt-5.1".to_string()]).as_deref(),
            Some("openai/gpt-5.1")
        );
        assert!(extract_model_from_args(&["acp".to_string()]).is_none());
    }
}
