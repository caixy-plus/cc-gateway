use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    pub log: LogConfig,
    pub ai: AiConfig,
    pub claude: ClaudeConfig,
    pub feishu: FeishuConfig,
    pub workspace: WorkspaceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    pub level: String,
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    pub enabled: bool,
    pub provider: String,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeConfig {
    pub cli_path: String,
    pub mode: String,
    pub model: String,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub system_prompt: String,
    pub reasoning_effort: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FeishuConfig {
    pub enabled: bool,
    pub app_id: String,
    pub app_secret: String,
    pub allow_from: String,
    pub encrypt_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceConfig {
    pub scan_dirs: Vec<String>,
    pub default_dir: String,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            log: LogConfig::default(),
            ai: AiConfig::default(),
            claude: ClaudeConfig::default(),
            feishu: FeishuConfig::default(),
            workspace: WorkspaceConfig::default(),
        }
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            file: "~/.cc-gateway/logs/gateway.log".to_string(),
        }
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "openai".to_string(),
            api_key: "${OPENAI_API_KEY}".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o-mini".to_string(),
        }
    }
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            cli_path: "claude".to_string(),
            mode: "default".to_string(),
            model: "".to_string(),
            allowed_tools: vec![
                "Read".to_string(),
                "Grep".to_string(),
                "Glob".to_string(),
                "Bash".to_string(),
                "Edit".to_string(),
                "Write".to_string(),
            ],
            disallowed_tools: vec![],
            system_prompt: "".to_string(),
            reasoning_effort: "".to_string(),
        }
    }
}

impl Default for FeishuConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            app_id: "${FEISHU_APP_ID}".to_string(),
            app_secret: "${FEISHU_APP_SECRET}".to_string(),
            allow_from: "*".to_string(),
            encrypt_key: "".to_string(),
        }
    }
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            scan_dirs: vec![
                "~/Workspace".to_string(),
                "~/Projects".to_string(),
            ],
            default_dir: "~/Workspace".to_string(),
        }
    }
}
