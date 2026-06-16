use std::collections::BTreeSet;

use anyhow::{Context, Result};

use crate::config::model::{AgentConfig, AgentProvider};

/// Platform-bound agents (Cursor) use vendor-tied models; cc-gateway does not expose `/models`.
pub fn is_platform_bound_agent(provider: &AgentProvider) -> bool {
    crate::config::agent_registry::capabilities_for(provider).platform_bound
}

/// User-visible hint for how `/models <arg>` applies to the active provider.
pub fn switch_hint_for_provider(provider: &AgentProvider) -> String {
    let caps = crate::config::agent_registry::capabilities_for(provider);
    if caps.model_arg_passthrough {
        return crate::t!("models.switch_hint_claude").to_string();
    }
    if caps.in_session_model_switch {
        return crate::t!("models.switch_hint_raw").to_string();
    }
    crate::t!("models.switch_hint_index").to_string()
}

/// Selectable Claude model names shown by `/models` (aliases Claude Code accepts via `--model`).
///
/// Version-pinned ids (`claude-opus-4-8`, …) and the `sonnet[1m]` / `opus[1m]` 1M-context variants
/// are intentionally omitted — the 1M variants are overage/entitlement gated and make Claude exit at
/// startup on accounts that lack them. Anything not listed can still be passed directly
/// (`/models <id>`); Claude Code validates the `--model` argument on respawn.
pub fn claude_models() -> &'static [&'static str] {
    &["default", "opus", "sonnet", "haiku", "fable"]
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

/// Gateway-maintained model catalog for providers without a stable list CLI.
///
/// Claude Code has no model-list subcommand (`claude models` is parsed as a *prompt* and runs a
/// billable inference turn), so the list is the static [`claude_models`] catalog — never shelling
/// out to the CLI or scanning settings files.
pub fn list_discovered_models(provider: &AgentProvider) -> Vec<String> {
    match provider {
        AgentProvider::Claude => claude_models().iter().map(|s| s.to_string()).collect(),
        _ => vec![],
    }
}

/// Resolve `/models <arg>` for providers that pass the model straight through (Claude `--model`).
///
/// The picker list is advisory; a numeric arg selects from it, otherwise any non-empty alias/id is
/// accepted as-is and validated by the provider CLI on respawn.
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
    if crate::config::agent_registry::capabilities_for(provider).model_arg_passthrough {
        resolve_claude_model_arg(arg, options)
    } else {
        resolve_model_arg(arg, options)
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
    fn claude_models_includes_core_aliases_without_1m_variants() {
        let models = claude_models();
        assert!(models.contains(&"opus"));
        assert!(models.contains(&"sonnet"));
        assert!(!models.iter().any(|m| m.ends_with("[1m]")));
        assert!(!models.iter().any(|m| m.starts_with("claude-opus-4")));
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
    fn list_discovered_models_for_claude() {
        let models = list_discovered_models(&AgentProvider::Claude);
        assert!(models.contains(&"opus".to_string()));
        assert!(list_discovered_models(&AgentProvider::Pi).is_empty());
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
