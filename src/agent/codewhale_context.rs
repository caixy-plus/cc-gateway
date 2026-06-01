//! CodeWhale ACP context budget, transcript loading, and deterministic compression.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use tracing::{debug, warn};

/// Parsed `codewhale doctor --json` capability section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeWhaleCapability {
    pub resolved_model: String,
    pub context_window: u32,
    pub max_output: u32,
}

/// Tunable policy for history budgeting and truncation.
#[derive(Debug, Clone)]
pub struct ContextPolicy {
    /// Fraction of the model window usable for input (history + template + current).
    pub usage_ratio: f64,
    /// Full recent turns kept before dropping older middle turns.
    pub keep_recent_turns: usize,
    /// Minimum recent turns when aggressively shrinking.
    pub min_recent_turns: usize,
    pub pin_first_user: bool,
    pub context_window_fallback: u32,
    pub chars_per_token: f64,
    /// Per-message soft cap before head/tail compression.
    pub max_message_chars: usize,
    /// CodeWhale ACP hard-caps generation regardless of model max_output.
    pub acp_max_output: u32,
    /// Reserved tokens for prompt template, system, and safety margin.
    pub template_overhead_tokens: usize,
}

impl Default for ContextPolicy {
    fn default() -> Self {
        Self {
            usage_ratio: 0.72,
            keep_recent_turns: 8,
            min_recent_turns: 3,
            pin_first_user: true,
            context_window_fallback: 128_000,
            chars_per_token: 3.5,
            max_message_chars: 8_000,
            acp_max_output: 4_096,
            template_overhead_tokens: 512,
        }
    }
}

/// One user message optionally followed by an assistant reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub user: String,
    pub assistant: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DoctorReport {
    capability: Option<DoctorCapability>,
}

#[derive(Debug, Deserialize)]
struct DoctorCapability {
    resolved_model: Option<String>,
    context_window: Option<u32>,
    max_output: Option<u32>,
}

/// Run `codewhale doctor --json` and parse the capability block.
pub async fn fetch_capability(cli_path: &str) -> CodeWhaleCapability {
    let resolved = crate::runtime::session::resolve_cli_path(cli_path);
    let output = tokio::process::Command::new(&resolved)
        .args(["doctor", "--json"])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(cap) = parse_capability_json(&text) {
                debug!(
                    "CodeWhale capability: model={} window={} max_output={}",
                    cap.resolved_model, cap.context_window, cap.max_output
                );
                return cap;
            }
            warn!("CodeWhale doctor --json succeeded but capability block missing");
        }
        Ok(out) => {
            warn!(
                "CodeWhale doctor --json failed (status={}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Err(err) => warn!("Failed to run CodeWhale doctor --json: {err}"),
    }

    fallback_capability()
}

pub fn parse_capability_json(json: &str) -> Option<CodeWhaleCapability> {
    let report: DoctorReport = serde_json::from_str(json).ok()?;
    let cap = report.capability?;
    Some(CodeWhaleCapability {
        resolved_model: cap.resolved_model.unwrap_or_else(|| "unknown".to_string()),
        context_window: cap.context_window?,
        max_output: cap.max_output.unwrap_or(4_096),
    })
}

pub fn fallback_capability() -> CodeWhaleCapability {
    CodeWhaleCapability {
        resolved_model: "unknown".to_string(),
        context_window: ContextPolicy::default().context_window_fallback,
        max_output: 4_096,
    }
}

pub fn history_budget_tokens(cap: &CodeWhaleCapability, policy: &ContextPolicy) -> usize {
    let window = cap.context_window.max(1_024) as f64;
    let effective_output = cap.max_output.min(policy.acp_max_output) as f64;
    let usable =
        window * policy.usage_ratio - effective_output - policy.template_overhead_tokens as f64;
    usable.max(1_024.0) as usize
}

pub fn estimate_tokens(text: &str, chars_per_token: f64) -> usize {
    let chars = text.chars().count();
    if chars == 0 {
        return 0;
    }
    ((chars as f64) / chars_per_token).ceil() as usize
}

pub fn history_jsonl_path(session_id: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(
        home.join(".cc-gateway")
            .join("history")
            .join(format!("{session_id}.jsonl")),
    )
}

/// Load JSONL history into structured turns (user/assistant pairs).
pub fn load_turns(session_id: &str) -> Vec<Turn> {
    let Some(path) = history_jsonl_path(session_id) else {
        return Vec::new();
    };
    load_turns_from_path(&path)
}

pub fn load_turns_from_path(path: &Path) -> Vec<Turn> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    parse_turns_from_jsonl(&raw)
}

fn parse_turns_from_jsonl(raw: &str) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    let mut pending_user: Option<String> = None;

    for line in raw.lines() {
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let role = value.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let content = value.get("content").and_then(|v| v.as_str()).unwrap_or("");
        if !is_replayable_message(role, content) {
            continue;
        }
        let cleaned = clean_message_content(content);
        if cleaned.is_empty() {
            continue;
        }

        match role {
            "user" => {
                if let Some(user) = pending_user.take() {
                    turns.push(Turn {
                        user,
                        assistant: None,
                    });
                }
                pending_user = Some(cleaned);
            }
            "assistant" => {
                if let Some(user) = pending_user.take() {
                    turns.push(Turn {
                        user,
                        assistant: Some(cleaned),
                    });
                }
            }
            _ => {}
        }
    }

    if let Some(user) = pending_user.take() {
        turns.push(Turn {
            user,
            assistant: None,
        });
    }

    turns
}

fn is_replayable_message(role: &str, content: &str) -> bool {
    if content.is_empty() {
        return false;
    }
    if role != "user" && role != "assistant" {
        return false;
    }
    if content.contains("[Tool:") || content.contains("\"sessionUpdate\":\"tool_call") {
        return false;
    }
    true
}

fn clean_message_content(content: &str) -> String {
    let cleaned: String = content
        .lines()
        .filter(|l| !l.trim_start().starts_with('{') && !l.trim_start().starts_with('['))
        .collect::<Vec<_>>()
        .join("\n");
    collapse_blank_lines(cleaned.trim())
}

fn collapse_blank_lines(text: &str) -> String {
    let mut out = String::new();
    let mut prev_blank = false;
    for line in text.lines() {
        let blank = line.trim().is_empty();
        if blank {
            if !prev_blank {
                out.push('\n');
            }
            prev_blank = true;
        } else {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(line);
            out.push('\n');
            prev_blank = false;
        }
    }
    out.trim().to_string()
}

fn shrink_message(text: &str, max_chars: usize, protect_full: bool) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    if protect_full || text.chars().count() <= max_chars {
        return shrink_code_blocks(text, max_chars);
    }
    let shrunk = head_tail_truncate(text, max_chars);
    shrink_code_blocks(&shrunk, max_chars)
}

fn shrink_code_blocks(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = String::new();
    let mut in_fence = false;
    let mut fence_lines: Vec<&str> = Vec::new();

    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            if in_fence {
                out.push_str(&format_compact_code_block(&fence_lines));
                fence_lines.clear();
                in_fence = false;
                out.push('\n');
            } else {
                in_fence = true;
                fence_lines.push(line);
            }
            continue;
        }
        if in_fence {
            fence_lines.push(line);
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if in_fence {
        out.push_str(&format_compact_code_block(&fence_lines));
    }
    let result = out.trim().to_string();
    if result.chars().count() <= max_chars {
        result
    } else {
        head_tail_truncate(&result, max_chars)
    }
}

fn format_compact_code_block(lines: &[&str]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let open = lines[0];
    let body: Vec<&str> = lines.iter().skip(1).copied().collect();
    if body.len() <= 40 {
        return lines.join("\n");
    }
    let head = body.iter().take(30).copied().collect::<Vec<_>>().join("\n");
    let tail = body
        .iter()
        .skip(body.len().saturating_sub(10))
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{open}\n{head}\n[... {} lines omitted ...]\n{tail}\n```",
        body.len().saturating_sub(40)
    )
}

fn head_tail_truncate(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let head_len = (max_chars * 60) / 100;
    let tail_len = (max_chars * 20) / 100;
    let omitted = count.saturating_sub(head_len + tail_len);
    let head: String = text.chars().take(head_len).collect();
    let tail: String = text.chars().skip(count.saturating_sub(tail_len)).collect();
    format!("{head}\n[... omitted {omitted} chars ...]\n{tail}")
}

fn format_turn(turn: &Turn) -> String {
    match &turn.assistant {
        Some(a) => format!("User: {}\n\nAssistant: {}", turn.user, a),
        None => format!("User: {}", turn.user),
    }
}

fn format_turns(turns: &[Turn]) -> String {
    turns
        .iter()
        .map(format_turn)
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn compress_turns_in_place(turns: &mut [Turn], max_message_chars: usize, pin_first: bool) {
    for (idx, turn) in turns.iter_mut().enumerate() {
        let protect = pin_first && idx == 0;
        turn.user = shrink_message(&turn.user, max_message_chars, protect);
        if let Some(ref mut assistant) = turn.assistant {
            *assistant = shrink_message(assistant, max_message_chars, false);
        }
    }
}

fn split_turns_for_truncation(
    turns: Vec<Turn>,
    keep_recent: usize,
    pin_first: bool,
) -> (Option<Turn>, Vec<Turn>, Vec<Turn>) {
    if turns.is_empty() {
        return (None, Vec::new(), Vec::new());
    }

    let pin = if pin_first {
        Some(turns[0].clone())
    } else {
        None
    };

    let start = if pin_first { 1 } else { 0 };
    if start >= turns.len() {
        return (pin, Vec::new(), Vec::new());
    }

    let body = turns[start..].to_vec();
    if body.len() <= keep_recent {
        return (pin, Vec::new(), body);
    }

    let split = body.len().saturating_sub(keep_recent);
    let middle = body[..split].to_vec();
    let tail = body[split..].to_vec();
    (pin, middle, tail)
}

fn fits_budget(text: &str, budget_tokens: usize, policy: &ContextPolicy) -> bool {
    estimate_tokens(text, policy.chars_per_token) <= budget_tokens
}

/// Deterministic compression + truncation to fit `budget_tokens`.
pub fn compress_turns_to_budget(
    turns: Vec<Turn>,
    budget_tokens: usize,
    policy: &ContextPolicy,
) -> String {
    if turns.is_empty() || budget_tokens == 0 {
        return String::new();
    }

    let mut working = turns;
    let mut per_message_cap = policy.max_message_chars;

    for _pass in 0..4 {
        compress_turns_in_place(&mut working, per_message_cap, policy.pin_first_user);
        let formatted = format_turns(&working);
        if fits_budget(&formatted, budget_tokens, policy) {
            return formatted;
        }
        per_message_cap = per_message_cap.saturating_mul(2) / 3;
        if per_message_cap < 512 {
            break;
        }
    }

    let mut keep_recent = policy.keep_recent_turns;
    loop {
        let (pin, middle, tail) =
            split_turns_for_truncation(working.clone(), keep_recent, policy.pin_first_user);
        let mut selected: Vec<Turn> = Vec::new();
        if let Some(p) = pin {
            selected.push(p);
        }
        selected.extend(tail);

        compress_turns_in_place(&mut selected, per_message_cap, policy.pin_first_user);
        let mut formatted = format_turns(&selected);
        if middle.is_empty() {
            if fits_budget(&formatted, budget_tokens, policy) {
                return formatted;
            }
        } else if !formatted.is_empty() {
            formatted = format!(
                "[Note: {} earlier turn(s) omitted due to context limit]\n\n{formatted}",
                middle.len()
            );
        } else if !middle.is_empty() {
            formatted = format!(
                "[Note: {} earlier turn(s) omitted due to context limit]",
                middle.len()
            );
        }

        if fits_budget(&formatted, budget_tokens, policy) {
            return formatted;
        }

        if keep_recent <= policy.min_recent_turns {
            break;
        }
        keep_recent = keep_recent.saturating_sub(1);
    }

    let (pin, _middle, tail) =
        split_turns_for_truncation(working, policy.min_recent_turns, policy.pin_first_user);
    let mut selected: Vec<Turn> = Vec::new();
    if let Some(mut p) = pin {
        p.user = shrink_message(&p.user, 512, true);
        if let Some(ref mut a) = p.assistant {
            *a = shrink_message(a, 512, false);
        }
        selected.push(p);
    }
    for mut turn in tail {
        turn.user = shrink_message(&turn.user, 512, false);
        if let Some(ref mut a) = turn.assistant {
            *a = shrink_message(a, 512, false);
        }
        selected.push(turn);
    }
    let mut formatted = format_turns(&selected);
    if !fits_budget(&formatted, budget_tokens, policy) {
        formatted = head_tail_truncate(
            &formatted,
            (budget_tokens as f64 * policy.chars_per_token) as usize,
        );
    }
    formatted
}

/// Build transcript for replay, excluding the current user message (not yet in JSONL).
pub fn build_history_transcript(
    session_id: &str,
    cap: &CodeWhaleCapability,
    policy: &ContextPolicy,
    work_dir: &str,
    current_message: &str,
) -> Option<String> {
    let turns = load_turns(session_id);
    if turns.is_empty() {
        return None;
    }

    let template_without_history = build_prompt(work_dir, None, current_message);
    let template_tokens = estimate_tokens(&template_without_history, policy.chars_per_token);
    let total_budget = history_budget_tokens(cap, policy);
    let history_budget = total_budget.saturating_sub(template_tokens);
    if history_budget == 0 {
        return None;
    }

    let transcript = compress_turns_to_budget(turns, history_budget, policy);
    if transcript.trim().is_empty() {
        None
    } else {
        Some(transcript)
    }
}

pub fn build_prompt(work_dir: &str, history: Option<&str>, current_message: &str) -> String {
    match history.filter(|h| !h.trim().is_empty()) {
        Some(h) => format!(
            "[Conversation history]\n{h}\n\n---\nWorking directory: {work_dir}\n\nUser: {current_message}"
        ),
        None => format!("Working directory: {work_dir}\n\n{current_message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_doctor_capability_json() {
        let json = r#"{"capability":{"resolved_model":"deepseek-v4-pro","context_window":1000000,"max_output":384000}}"#;
        let cap = parse_capability_json(json).expect("capability");
        assert_eq!(cap.resolved_model, "deepseek-v4-pro");
        assert_eq!(cap.context_window, 1_000_000);
        assert_eq!(cap.max_output, 384_000);
    }

    #[test]
    fn history_budget_reserves_output_and_overhead() {
        let cap = CodeWhaleCapability {
            resolved_model: "m".into(),
            context_window: 10_000,
            max_output: 384_000,
        };
        let policy = ContextPolicy::default();
        let budget = history_budget_tokens(&cap, &policy);
        assert!(budget < 10_000);
        assert!(budget >= 1_024);
    }

    #[test]
    fn parse_turns_groups_user_assistant_pairs() {
        let raw = r#"
{"role":"user","content":"hello"}
{"role":"assistant","content":"hi there"}
{"role":"user","content":"second"}
{"role":"assistant","content":"reply two"}
"#;
        let turns = parse_turns_from_jsonl(raw);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].user, "hello");
        assert_eq!(turns[0].assistant.as_deref(), Some("hi there"));
        assert_eq!(turns[1].user, "second");
    }

    #[test]
    fn parse_turns_skips_tool_noise() {
        let raw = r#"
{"role":"user","content":"run tool"}
{"role":"assistant","content":"[Tool: bash]"}
{"role":"user","content":"ok"}
"#;
        let turns = parse_turns_from_jsonl(raw);
        // Tool-containing assistant line is filtered, leaving two unpaired user turns.
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].user, "run tool");
        assert!(turns[0].assistant.is_none());
        assert_eq!(turns[1].user, "ok");
        assert!(turns[1].assistant.is_none());
    }

    #[test]
    fn compress_keeps_recent_and_drops_middle() {
        let mut turns = Vec::new();
        for i in 0..12 {
            turns.push(Turn {
                user: format!("question {i}"),
                assistant: Some(format!("answer {i}")),
            });
        }
        let policy = ContextPolicy {
            keep_recent_turns: 3,
            min_recent_turns: 2,
            pin_first_user: false,
            max_message_chars: 200,
            ..Default::default()
        };
        let out = compress_turns_to_budget(turns, 120, &policy);
        assert!(out.contains("omitted"));
        assert!(out.contains("question 11"));
        assert!(!out.contains("question 0"));
    }

    #[test]
    fn pin_first_user_survives_truncation() {
        let turns = vec![
            Turn {
                user: "define the task".into(),
                assistant: Some("ack".into()),
            },
            Turn {
                user: "q2".into(),
                assistant: Some("a2".into()),
            },
            Turn {
                user: "q3".into(),
                assistant: Some("a3".into()),
            },
        ];
        let policy = ContextPolicy {
            keep_recent_turns: 1,
            min_recent_turns: 1,
            pin_first_user: true,
            max_message_chars: 100,
            ..Default::default()
        };
        let out = compress_turns_to_budget(turns, 40, &policy);
        assert!(out.contains("define the task"));
    }

    #[test]
    fn build_prompt_includes_history_block() {
        let prompt = build_prompt("/tmp/proj", Some("User: hi\n\nAssistant: hello"), "next");
        assert!(prompt.contains("[Conversation history]"));
        assert!(prompt.contains("Working directory: /tmp/proj"));
        assert!(prompt.ends_with("User: next"));
    }

    #[test]
    fn build_history_transcript_respects_budget() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sess.jsonl");
        let mut file = std::fs::File::create(&path).expect("create");
        for i in 0..20 {
            writeln!(file, r#"{{"role":"user","content":"u{i}"}}"#).expect("write");
            writeln!(file, r#"{{"role":"assistant","content":"a{i}"}}"#).expect("write");
        }

        let turns = load_turns_from_path(&path);
        assert_eq!(turns.len(), 20);

        let cap = CodeWhaleCapability {
            resolved_model: "test".into(),
            context_window: 2_000,
            max_output: 4_096,
        };
        let policy = ContextPolicy {
            usage_ratio: 0.5,
            keep_recent_turns: 4,
            ..Default::default()
        };
        let template_tokens =
            estimate_tokens(&build_prompt("/w", None, "current"), policy.chars_per_token);
        let budget = history_budget_tokens(&cap, &policy).saturating_sub(template_tokens);
        let transcript = compress_turns_to_budget(turns, budget, &policy);
        assert!(estimate_tokens(&transcript, policy.chars_per_token) <= budget);
    }
}
