use super::current_language;
use super::lang::Language;

/// Lookup a translation by key. Returns the key itself if not found.
pub fn t(key: &str) -> &str {
    let lang = current_language();
    match key {
        // daemon.rs
        "daemon.already_running" => match lang {
            Language::En => "cc-gateway daemon is already running (PID: {PID})",
            Language::ZhCN => "cc-gateway 守护进程已在运行 (PID: {PID})",
        },
        "daemon.started" => match lang {
            Language::En => "cc-gateway daemon started (PID: {PID})",
            Language::ZhCN => "cc-gateway 守护进程已启动 (PID: {PID})",
        },
        "daemon.stop_signal" => match lang {
            Language::En => "Sent stop signal to daemon (PID: {PID})",
            Language::ZhCN => "已向守护进程发送停止信号 (PID: {PID})",
        },
        "daemon.stopped" => match lang {
            Language::En => "Daemon stopped.",
            Language::ZhCN => "守护进程已停止。",
        },
        "daemon.running" => match lang {
            Language::En => "cc-gateway daemon is running (PID: {PID})",
            Language::ZhCN => "cc-gateway 守护进程正在运行 (PID: {PID})",
        },
        "daemon.not_running" => match lang {
            Language::En => "cc-gateway daemon is not running.",
            Language::ZhCN => "cc-gateway 守护进程未在运行。",
        },
        "daemon.restarting" => match lang {
            Language::En => "Restarting cc-gateway daemon...",
            Language::ZhCN => "正在重启 cc-gateway 守护进程...",
        },
        "daemon.auto_start_enabled_macos" => match lang {
            Language::En => "Enabled auto-start at login (launchd).",
            Language::ZhCN => "已启用登录时自动启动 (launchd)。",
        },
        "daemon.plist_path" => match lang {
            Language::En => "Plist: {PATH}",
            Language::ZhCN => "Plist: {PATH}",
        },
        "daemon.auto_start_enabled_linux" => match lang {
            Language::En => "Enabled auto-start at login (systemd).",
            Language::ZhCN => "已启用登录时自动启动 (systemd)。",
        },
        "daemon.service_path" => match lang {
            Language::En => "Service: {PATH}",
            Language::ZhCN => "服务: {PATH}",
        },
        "daemon.auto_start_unsupported" => match lang {
            Language::En => "Auto-start is only supported on macOS and Linux.",
            Language::ZhCN => "自动启动仅支持 macOS 和 Linux。",
        },
        "daemon.auto_start_disabled_macos" => match lang {
            Language::En => "Disabled auto-start at login (launchd).",
            Language::ZhCN => "已禁用登录时自动启动 (launchd)。",
        },
        "daemon.auto_start_disabled_linux" => match lang {
            Language::En => "Disabled auto-start at login (systemd).",
            Language::ZhCN => "已禁用登录时自动启动 (systemd)。",
        },
        "daemon.log_not_found" => match lang {
            Language::En => "Log file not found: {PATH}",
            Language::ZhCN => "日志文件未找到: {PATH}",
        },
        "daemon.following_log" => match lang {
            Language::En => "\n-- Following log (Ctrl+C to exit) --",
            Language::ZhCN => "\n-- 正在追踪日志 (按 Ctrl+C 退出) --",
        },
        "daemon.launchctl_load_failed" => match lang {
            Language::En => "launchctl load failed",
            Language::ZhCN => "launchctl 加载失败",
        },
        "daemon.systemctl_enable_failed" => match lang {
            Language::En => "systemctl enable failed",
            Language::ZhCN => "systemctl 启用失败",
        },
        "daemon.webui_starting" => match lang {
            Language::En => "Daemon not running, starting...",
            Language::ZhCN => "守护进程未运行，正在启动...",
        },
        "daemon.webui_opening" => match lang {
            Language::En => "Opening WebUI at {URL}...",
            Language::ZhCN => "正在打开 WebUI: {URL}...",
        },
        "daemon.webui_token_header" => match lang {
            Language::En => "WebUI token: {TOKEN}",
            Language::ZhCN => "WebUI 令牌: {TOKEN}",
        },
        "daemon.webui_token_generated" => match lang {
            Language::En => "A new token has been generated and saved.",
            Language::ZhCN => "已生成新令牌并保存。",
        },
        "daemon.webui_token_refreshed" => match lang {
            Language::En => "Token refreshed (old token is now invalid).",
            Language::ZhCN => "令牌已刷新（旧令牌已失效）。",
        },
        "daemon.webui_token_url" => match lang {
            Language::En => "Open: {URL}",
            Language::ZhCN => "访问: {URL}",
        },
        "daemon.webui_token_hint" => match lang {
            Language::En => "The daemon must be running. Use cc-gateway webui-token --refresh to regenerate.",
            Language::ZhCN => "需要先启动守护进程。使用 cc-gateway webui-token --refresh 可刷新令牌。",
        },

        // config/wizard.rs
        "wizard.title" => match lang {
            Language::En => "=== cc-gateway Configuration ===",
            Language::ZhCN => "=== cc-gateway 配置 ===",
        },
        "wizard.log_section" => match lang {
            Language::En => "log        - Logging settings",
            Language::ZhCN => "log        - 日志设置",
        },
        "wizard.agent_section" => match lang {
            Language::En => "agent      - Agent provider settings",
            Language::ZhCN => "agent      - 智能体提供商设置",
        },
        "wizard.agent_config" => match lang {
            Language::En => "=== Agent Settings ===",
            Language::ZhCN => "=== 智能体设置 ===",
        },
        "wizard.agent_profile" => match lang {
            Language::En => "Profile: {NAME}",
            Language::ZhCN => "配置档: {NAME}",
        },
        "wizard.feishu_section" => match lang {
            Language::En => "feishu     - Feishu/Lark bot settings",
            Language::ZhCN => "feishu     - 飞书/ Lark 机器人设置",
        },
        "wizard.default_dir_section" => match lang {
            Language::En => "default_dir - Default working directory",
            Language::ZhCN => "default_dir - 默认工作目录",
        },
        "wizard.save_exit" => match lang {
            Language::En => "Save and exit",
            Language::ZhCN => "保存并退出",
        },
        "wizard.exit_no_save" => match lang {
            Language::En => "Exit without saving",
            Language::ZhCN => "不保存退出",
        },
        "wizard.select_section" => match lang {
            Language::En => "Select section [1-6]:",
            Language::ZhCN => "选择配置项 [1-6]:",
        },
        "wizard.invalid_choice" => match lang {
            Language::En => "Invalid choice, try again.",
            Language::ZhCN => "无效的选择，请重试。",
        },
        "wizard.log_config" => match lang {
            Language::En => "--- Log Configuration ---",
            Language::ZhCN => "--- 日志配置 ---",
        },
        "wizard.agent_settings" => match lang {
            Language::En => "--- Claude Configuration ---",
            Language::ZhCN => "--- Claude 配置 ---",
        },
        "wizard.feishu_config" => match lang {
            Language::En => "--- Feishu Configuration ---",
            Language::ZhCN => "--- 飞书配置 ---",
        },
        "wizard.default_dir_config" => match lang {
            Language::En => "--- Default Directory Configuration ---",
            Language::ZhCN => "--- 默认目录配置 ---",
        },
        "wizard.config_saved" => match lang {
            Language::En => "Config saved.",
            Language::ZhCN => "配置已保存。",
        },
        "wizard.exiting_no_save" => match lang {
            Language::En => "Exiting without saving.",
            Language::ZhCN => "不保存退出。",
        },
        "wizard.init_title" => match lang {
            Language::En => "=== cc-gateway Initial Setup ===",
            Language::ZhCN => "=== cc-gateway 初始设置 ===",
        },
        "wizard.welcome" => match lang {
            Language::En => "Welcome! This will create your configuration file.",
            Language::ZhCN => "欢迎！这将创建您的配置文件。",
        },
        "wizard.location" => match lang {
            Language::En => "Location: {PATH}",
            Language::ZhCN => "位置: {PATH}",
        },
        "wizard.rerun" => match lang {
            Language::En => "You can re-run this anytime with: cc-gateway init",
            Language::ZhCN => "您可以随时通过以下命令重新运行: cc-gateway init",
        },
        "wizard.found_existing" => match lang {
            Language::En => "Found existing config. Press Enter to keep current values.",
            Language::ZhCN => "发现已有配置。按 Enter 保留当前值。",
        },
        "wizard.init_skipped_existing" => match lang {
            Language::En => "Config already exists at {PATH}, skipping initialization. Edit it in the WebUI settings or directly.",
            Language::ZhCN => "配置文件已存在于 {PATH}，跳过初始化。可在 WebUI 设置中修改或直接编辑文件。",
        },
        "wizard.no_config_defaults" => match lang {
            Language::En => "No existing config found. Using defaults.",
            Language::ZhCN => "未找到现有配置。使用默认值。",
        },
        "wizard.feishu_section_title" => match lang {
            Language::En => "--- Feishu/Lark Bot Configuration ---",
            Language::ZhCN => "--- 飞书/ Lark 机器人配置 ---",
        },
        "wizard.press_enter_keep" => match lang {
            Language::En => "(Press Enter to keep the current/default value)",
            Language::ZhCN => "(按 Enter 保留当前/默认值)",
        },
        "wizard.enter_skip" => match lang {
            Language::En => "(Enter 'skip' to skip Feishu configuration entirely)",
            Language::ZhCN => "(输入 'skip' 完全跳过飞书配置)",
        },
        "wizard.other_settings" => match lang {
            Language::En => "--- Other Settings ---",
            Language::ZhCN => "--- 其他设置 ---",
        },
        "wizard.setup_complete" => match lang {
            Language::En => "=== Setup Complete ===",
            Language::ZhCN => "=== 设置完成 ===",
        },
        "wizard.config_saved_to" => match lang {
            Language::En => "Config saved to: {PATH}",
            Language::ZhCN => "配置已保存至: {PATH}",
        },
        "wizard.modify_later" => match lang {
            Language::En => "To modify later:",
            Language::ZhCN => "稍后修改方式:",
        },
        "wizard.run_init" => match lang {
            Language::En => "  - Run: cc-gateway init",
            Language::ZhCN => "  - 运行: cc-gateway init",
        },
        "wizard.open_webui" => match lang {
            Language::En => "  - Open the WebUI and edit settings there (cc-gateway webui)",
            Language::ZhCN => "  - 打开 WebUI 在设置中修改 (cc-gateway webui)",
        },
        "wizard.or_edit" => match lang {
            Language::En => "  - Or edit: {PATH}",
            Language::ZhCN => "  - 或编辑: {PATH}",
        },
        "wizard.feishu_not_configured" => match lang {
            Language::En => "Note: Feishu bot is not configured.",
            Language::ZhCN => "注意: 飞书机器人未配置。",
        },
        "wizard.without_credentials" => match lang {
            Language::En => "      Without app_id and app_secret, the Feishu bot will not work.",
            Language::ZhCN => "      没有 app_id 和 app_secret，飞书机器人将无法工作。",
        },
        "wizard.configure_later" => match lang {
            Language::En => "      You can configure it later in the WebUI settings.",
            Language::ZhCN => "      您可以稍后在 WebUI 设置中配置。",
        },
        "wizard.please_edit_credentials" => match lang {
            Language::En => "Please edit it to add your Feishu app credentials.",
            Language::ZhCN => "请编辑以添加您的飞书应用凭证。",
        },

        // init guided setup (cc-gateway init)
        "wizard.agent_section_title" => match lang {
            Language::En => "--- Step 1/2: Agent ---",
            Language::ZhCN => "--- 第 1/2 步：智能体 ---",
        },
        "wizard.agent_section_hint" => match lang {
            Language::En => "Pick one agent to start with. You can enable and configure both later in the WebUI.",
            Language::ZhCN => "先选择一个智能体。两者都可稍后在 WebUI 中启用并配置。",
        },
        "wizard.bot_section_title" => match lang {
            Language::En => "--- Step 2/2: Bot Platform ---",
            Language::ZhCN => "--- 第 2/2 步：机器人平台 ---",
        },
        "wizard.bot_section_hint" => match lang {
            Language::En => "Pick one bot platform to start with. You can enable and configure both later in the WebUI.",
            Language::ZhCN => "先选择一个机器人平台。两者都可稍后在 WebUI 中启用并配置。",
        },
        "wizard.opt_skip" => match lang {
            Language::En => "  (Press Enter to skip)",
            Language::ZhCN => "  (直接回车跳过)",
        },
        "wizard.choose_prompt" => match lang {
            Language::En => "Your choice:",
            Language::ZhCN => "请选择:",
        },
        "wizard.label_installed" => match lang {
            Language::En => "[installed]",
            Language::ZhCN => "[已安装]",
        },
        "wizard.label_not_found" => match lang {
            Language::En => "[not found]",
            Language::ZhCN => "[未检测到]",
        },
        "wizard.keep_or_clear" => match lang {
            Language::En => "(Enter=keep, '-'=clear)",
            Language::ZhCN => "(回车=保留, '-'=清空)",
        },
        "wizard.agent_unavailable_warn" => match lang {
            Language::En => "  Note: '{NAME}' was not detected on PATH; this agent will be unavailable until it is installed.",
            Language::ZhCN => "  注意: 未在 PATH 中检测到 '{NAME}'，安装前该智能体不可用。",
        },
        "wizard.install_hint_codex" => match lang {
            Language::En => "Requires Zed's ACP adapter: npm i -g @zed-industries/codex-acp (not the raw codex CLI). Auth: codex login or OPENAI_API_KEY.",
            Language::ZhCN => "需安装 Zed 的 ACP 适配器：npm i -g @zed-industries/codex-acp（不是裸 codex CLI）。登录：codex login 或设置 OPENAI_API_KEY。",
        },
        "wizard.agent_configured" => match lang {
            Language::En => "  Agent set to: {NAME}",
            Language::ZhCN => "  智能体已设为: {NAME}",
        },
        "wizard.bot_configured" => match lang {
            Language::En => "  Bot platform set to: {NAME}",
            Language::ZhCN => "  机器人平台已设为: {NAME}",
        },
        "wizard.skipped_agent" => match lang {
            Language::En => "  Skipped agent configuration.",
            Language::ZhCN => "  已跳过智能体配置。",
        },
        "wizard.skipped_bot" => match lang {
            Language::En => "  Skipped bot configuration.",
            Language::ZhCN => "  已跳过机器人配置。",
        },
        "wizard.review_title" => match lang {
            Language::En => "--- Review ---",
            Language::ZhCN => "--- 检查 ---",
        },
        "wizard.review_ok" => match lang {
            Language::En => "Everything looks good.",
            Language::ZhCN => "配置看起来没问题。",
        },
        "wizard.review_has_issues" => match lang {
            Language::En => "Some settings are incomplete. You can finish them later in the WebUI:",
            Language::ZhCN => "部分设置尚不完整，可稍后在 WebUI 中补全:",
        },
        "wizard.warn_agent_missing" => match lang {
            Language::En => "  - Agent '{NAME}' is not installed; it won't work until installed.",
            Language::ZhCN => "  - 智能体 '{NAME}' 未安装，安装前无法使用。",
        },
        "wizard.warn_feishu_incomplete" => match lang {
            Language::En => "  - Feishu is enabled but app_id/app_secret are empty.",
            Language::ZhCN => "  - 飞书已启用，但 app_id/app_secret 为空。",
        },
        "wizard.warn_qq_incomplete" => match lang {
            Language::En => "QQ bot credentials are incomplete; enable after filling app_id and app_secret.",
            Language::ZhCN => "QQ 机器人凭证不完整，请补全 app_id 与 app_secret 后再启用。",
        },
        "wizard.warn_telegram_incomplete" => match lang {
            Language::En => "  - Telegram is enabled but bot_token is empty.",
            Language::ZhCN => "  - Telegram 已启用，但 bot_token 为空。",
        },
        "wizard.webui_from_now" => match lang {
            Language::En => "Setup done. From now on, manage everything in the WebUI (run: cc-gateway webui).",
            Language::ZhCN => "初始化完成。之后所有配置都在 WebUI 中管理 (运行: cc-gateway webui)。",
        },
        "wizard.webui_token_generated" => match lang {
            Language::En => "WebUI access token: {TOKEN}",
            Language::ZhCN => "WebUI 访问令牌: {TOKEN}",
        },

        // uninstall (cc-gateway uninstall)
        "uninstall.plan_title" => match lang {
            Language::En => "This will completely uninstall cc-gateway:",
            Language::ZhCN => "即将彻底卸载 cc-gateway：",
        },
        "uninstall.plan_stop" => match lang {
            Language::En => "  - stop the running daemon",
            Language::ZhCN => "  - 停止守护进程",
        },
        "uninstall.plan_autostart" => match lang {
            Language::En => "  - remove auto-start (launchd/systemd)",
            Language::ZhCN => "  - 移除开机自启 (launchd/systemd)",
        },
        "uninstall.plan_binary" => match lang {
            Language::En => "  - delete the binary: {PATH}",
            Language::ZhCN => "  - 删除二进制：{PATH}",
        },
        "uninstall.plan_data_delete" => match lang {
            Language::En => "  - delete all data: ~/.cc-gateway (config, logs, history, media, skills)",
            Language::ZhCN => "  - 删除全部数据：~/.cc-gateway（配置、日志、会话历史、媒体、skills）",
        },
        "uninstall.plan_data_keep" => match lang {
            Language::En => "  - keep data: ~/.cc-gateway (use without --keep-data to delete it)",
            Language::ZhCN => "  - 保留数据：~/.cc-gateway（不加 --keep-data 则会删除）",
        },
        "uninstall.confirm_prompt" => match lang {
            Language::En => "This cannot be undone. Continue? [y/N]:",
            Language::ZhCN => "此操作不可恢复。确认卸载？[y/N]：",
        },
        "uninstall.cancelled" => match lang {
            Language::En => "Uninstall cancelled.",
            Language::ZhCN => "已取消卸载。",
        },
        "uninstall.running" => match lang {
            Language::En => "Running cleanup...",
            Language::ZhCN => "正在执行清理...",
        },

        "builtin.thinking_placeholder" => match lang {
            Language::En => "💭 Thinking...",
            Language::ZhCN => "💭 Thinking...",
        },

        // command/builtin.rs
        "builtin.help" => match lang {
            Language::En => concat!(
                "cc-gateway commands (no active session)\n",
                "  /help                     Show this help\n",
                "  /agent [args...]          Start an agent session\n",
                "  /agents [provider]        Set this channel's default agent\n",
                "  /agent_history [n]        List or resume recent sessions\n",
                "  /cd <path>                Change working directory\n",
                "  /cd_default               Reset to default directory\n",
                "  /pwd                      Show working directory\n",
                "  /ll                       List subdirectories\n",
                "  /mkdir <name>             Create a directory\n",
                "\nTip: use /agent to start; during a session, /help lists session-only commands.",
            ),
            Language::ZhCN => concat!(
                "cc-gateway 命令（无活跃会话）\n",
                "  /help                     显示此帮助\n",
                "  /agent [参数]             启动智能体会话\n",
                "  /agents [provider]        设置本频道默认智能体\n",
                "  /agent_history [n]        查看 / 恢复最近会话\n",
                "  /cd <路径>                更改工作目录\n",
                "  /cd_default               切换到默认目录\n",
                "  /pwd                      显示工作目录\n",
                "  /ll                       列出子目录\n",
                "  /mkdir <目录名>           创建目录\n",
                "\n提示：使用 /agent 启动会话；会话进行中输入 /help 可查看会话模式专用命令。",
            ),
        },
        "builtin.session_help" => match lang {
            Language::En => concat!(
                "cc-gateway commands (active session)\n",
                "  /help                     Show this help\n",
                "  /quit                     Stop the active session\n",
                "  /esc [msg]                Flush queued messages (best-effort)\n",
                "  /stop                     Stop current generation (best-effort)\n",
                "  /clear                    Clear session context\n",
                "  /compact [hint]           Compact conversation context (Claude, Pi)\n",
                "  /init [hint]              Initialize project memory (CLAUDE.md; Claude only)\n",
                "  /models|/model [id|n]     List or switch models\n",
                "  /status                   Show agent status (ready / busy)\n",
                "  /show_thinking            Always show Thinking output\n",
                "  /hide_thinking            Hide Thinking output\n",
                "\nPermission prompts use in-chat buttons (not /commands).\n",
                "Other /commands are not available in session mode (shown here).\n",
                "Plain text (no leading /) is sent to the agent.",
            ),
            Language::ZhCN => concat!(
                "cc-gateway 命令（会话进行中）\n",
                "  /help                     显示此帮助\n",
                "  /quit                     停止当前会话\n",
                "  /esc [消息]               强推排队消息（best-effort）\n",
                "  /stop                     停止当前输出（best-effort）\n",
                "  /clear                    清理会话上下文\n",
                "  /compact [提示]           压缩会话上下文（Claude、Pi）\n",
                "  /init [提示]              初始化项目记忆文件（CLAUDE.md；仅 Claude）\n",
                "  /models|/model [id|序号]  列出 / 切换模型\n",
                "  /status                   显示智能体状态（就绪 / 输出中）\n",
                "  /show_thinking            始终显示 Thinking 输出\n",
                "  /hide_thinking            隐藏 Thinking 输出\n",
                "\n权限请求请使用消息内按钮（非 / 命令）。\n",
                "会话模式下其他 / 命令不可用（将显示本帮助）。\n",
                "不以 / 开头的文字将发送给智能体。",
            ),
        },
        "models.title" => match lang {
            Language::En => "Models for current agent: {NAME}",
            Language::ZhCN => "当前智能体可用模型：{NAME}",
        },
        "models.current_active" => match lang {
            Language::En => "Current model: {MODEL}",
            Language::ZhCN => "当前模型：{MODEL}",
        },
        "models.current_default" => match lang {
            Language::En => "Current model: (provider default — not set via cc-gateway)",
            Language::ZhCN => "当前模型：（智能体默认，未通过 cc-gateway 指定）",
        },
        "models.no_known_models" => match lang {
            Language::En => "(No curated model list for this agent in cc-gateway.)",
            Language::ZhCN => "（cc-gateway 未内置该智能体的模型列表）",
        },
        "models.switch_hint_index" => match lang {
            Language::En => "Switch: /models or /model <number> (restarts the session with --model).",
            Language::ZhCN => "切换：/models 或 /model <序号>（会重启会话并追加 --model 参数）",
        },
        "models.switch_hint_raw" => match lang {
            Language::En => "Switch: /models or /model <model_id> (applied in the current session).",
            Language::ZhCN => "切换：/models 或 /model <model_id>（在当前会话内生效）",
        },
        "models.not_supported" => match lang {
            Language::En => "Model switching is not supported for {NAME} in cc-gateway yet.",
            Language::ZhCN => "cc-gateway 暂不支持为 {NAME} 切换模型。",
        },
        "models.not_supported_platform_agent" => match lang {
            Language::En => "{NAME} is a platform-bound agent — model selection is managed by the vendor CLI, not cc-gateway. Use /agent with provider-specific flags if you need a different setup, or switch to OpenCode/Pi for in-session /models.",
            Language::ZhCN => "{NAME} 是平台定制型智能体，模型由官方 CLI 管理，cc-gateway 不提供 /models 切换。如需更换配置请用 /agent 并带上对应参数，或改用 OpenCode/Pi 以使用会话内 /models。",
        },
        "models.invalid_index" => match lang {
            Language::En => "Invalid model index.",
            Language::ZhCN => "模型序号无效。",
        },
        "models.switched" => match lang {
            Language::En => "Switched {NAME} model to: {MODEL}.",
            Language::ZhCN => "已将 {NAME} 模型切换为：{MODEL}。",
        },
        "models.switch_failed" => match lang {
            Language::En => "Failed to switch model: {ERR}",
            Language::ZhCN => "切换模型失败：{ERR}",
        },
        "models.pi_requires_provider_model" => match lang {
            Language::En => "Pi model id must be in the form: provider/model (e.g. anthropic/claude-sonnet-4-20250514).",
            Language::ZhCN => "Pi 模型需使用 provider/model 格式（例如 anthropic/claude-sonnet-4-20250514）。",
        },
        "builtin.select_agent_prompt" => match lang {
            Language::En => "Default agents for this channel:",
            Language::ZhCN => "本频道可选默认智能体：",
        },
        "builtin.agent_option" => match lang {
            Language::En => "{NAME}",
            Language::ZhCN => "{NAME}",
        },
        "builtin.agent_option_default" => match lang {
            Language::En => "{NAME} *",
            Language::ZhCN => "{NAME} *",
        },
        "builtin.agent_fallback_name" => match lang {
            Language::En => "Agent",
            Language::ZhCN => "智能体",
        },
        "builtin.channel_agent_set" => match lang {
            Language::En => "Default agent for this channel set to {NAME}.",
            Language::ZhCN => "本频道默认智能体已设为 {NAME}。",
        },
        "builtin.failed_set_channel_agent" => match lang {
            Language::En => "Failed to set channel default agent: {ERR}",
            Language::ZhCN => "设置频道默认智能体失败: {ERR}",
        },
        "builtin.agents_requires_channel" => match lang {
            Language::En => "/agents must be used from a channel context (WebUI or chat).",
            Language::ZhCN => "/agents 需要在频道上下文中使用（WebUI 或聊天）。",
        },
        "builtin.invalid_agent_index" => match lang {
            Language::En => "Invalid agent selection.",
            Language::ZhCN => "无效的智能体选项。",
        },
        "feishu.choose_agent" => match lang {
            Language::En => "Choose the default agent for this chat:",
            Language::ZhCN => "选择本聊天的默认智能体：",
        },
        "feishu.model_picker_title" => match lang {
            Language::En => "Choose model",
            Language::ZhCN => "选择模型",
        },
        "feishu.choose_model" => match lang {
            Language::En => "Choose model for {NAME}:",
            Language::ZhCN => "为 {NAME} 选择模型：",
        },
        "feishu.select_agent_title" => match lang {
            Language::En => "Default Agent",
            Language::ZhCN => "默认智能体",
        },
        "feishu.agent_option_default" => match lang {
            Language::En => "{NAME} (current)",
            Language::ZhCN => "{NAME}（当前）",
        },
        "telegram.choose_agent" => match lang {
            Language::En => "Choose the default agent for this chat:",
            Language::ZhCN => "选择本聊天的默认智能体：",
        },
        "telegram.choose_model" => match lang {
            Language::En => "Choose model for {NAME}:",
            Language::ZhCN => "为 {NAME} 选择模型：",
        },
        "builtin.session_stopped" => match lang {
            Language::En => "{NAME} session stopped.",
            Language::ZhCN => "{NAME} 会话已停止。",
        },
        "builtin.shutdown_notice" => match lang {
            Language::En => "cc-gateway is shutting down, session closed.",
            Language::ZhCN => "机器人正在关闭，会话已退出。",
        },
        "builtin.failed_stop_session" => match lang {
            Language::En => "Failed to stop session: {ERR}",
            Language::ZhCN => "停止会话失败: {ERR}",
        },
        "builtin.esc_sent" => match lang {
            Language::En => "ESC sent — queued messages flushed.",
            Language::ZhCN => "ESC 已发送 — 排队消息已强推。",
        },
        "builtin.esc_sent_claude" => match lang {
            Language::En => "Flush signal sent to Claude (best-effort). cc-gateway cannot guarantee queued messages are processed immediately — especially while a tool is running.",
            Language::ZhCN => "已向 Claude 发送 flush 信号（best-effort）。gateway 无法保证排队消息立刻生效，工具运行期间尤其如此。",
        },
        "builtin.esc_sent_cursor" => match lang {
            Language::En => "ESC sent — pending gateway messages flushed (if any). Use /esc <msg> to forward a message while busy.",
            Language::ZhCN => "ESC 已发送 — 已刷新 gateway 侧待发消息（如有）。busy 时可用 /esc <消息> 立即转发。",
        },
        "builtin.esc_sent_pi" => match lang {
            Language::En => "ESC sent — pending gateway messages flushed (if any). Use /esc <msg> to forward a message while busy.",
            Language::ZhCN => "ESC 已发送 — 已刷新 gateway 侧待发消息（如有）。busy 时可用 /esc <消息> 立即转发。",
        },
        "builtin.esc_sent_codex" => match lang {
            Language::En => "ESC sent — current generation cancelled via ACP.",
            Language::ZhCN => "ESC 已发送 — 已通过 ACP 取消当前输出。",
        },
        "builtin.esc_sent_opencode" => match lang {
            Language::En => "ESC sent — current generation cancelled via ACP.",
            Language::ZhCN => "ESC 已发送 — 已通过 ACP 取消当前输出。",
        },
        "builtin.esc_sent_kimi" => match lang {
            Language::En => "ESC sent — current generation cancelled via ACP.",
            Language::ZhCN => "ESC 已发送 — 已通过 ACP 取消当前输出。",
        },
        "builtin.esc_sent_gemini" => match lang {
            Language::En => "ESC sent — current generation cancelled via ACP.",
            Language::ZhCN => "ESC 已发送 — 已通过 ACP 取消当前输出。",
        },
        "builtin.esc_sent_qoder" => match lang {
            Language::En => "ESC sent — current generation cancelled via ACP.",
            Language::ZhCN => "ESC 已发送 — 已通过 ACP 取消当前输出。",
        },
        "builtin.esc_already_idle" => match lang {
            Language::En => "Agent is idle with no queued messages.",
            Language::ZhCN => "智能体已就绪，无排队消息。",
        },
        "builtin.esc_already_idle_claude" => match lang {
            Language::En => "Claude is idle on the gateway side — nothing to flush here. Messages sent while busy are queued inside Claude and usually processed when the current turn ends; stream-json mode does not support a reliable force-flush.",
            Language::ZhCN => "Claude 当前空闲（gateway 侧无待发消息）。busy 时发送的内容在 Claude 内部排队，通常需等当前 turn 结束；stream-json 模式暂无可靠的强推机制。",
        },
        "builtin.esc_with_prompt_sent" => match lang {
            Language::En => "ESC sent — flushed queue and forwarded: {MSG}",
            Language::ZhCN => "ESC 已发送 — 已强推排队消息并转发: {MSG}",
        },
        "builtin.esc_with_prompt_sent_claude" => match lang {
            Language::En => "Sent to Claude: {MSG}. If the agent is busy, stream-json may defer processing until the current turn ends.",
            Language::ZhCN => "已发送给 Claude：{MSG}。若智能体正 busy，stream-json 下可能需等当前 turn 结束才处理。",
        },
        "builtin.esc_with_prompt_sent_cursor" => match lang {
            Language::En => "Message forwarded: {MSG}",
            Language::ZhCN => "消息已转发：{MSG}",
        },
        "builtin.esc_with_prompt_sent_pi" => match lang {
            Language::En => "Message forwarded: {MSG}",
            Language::ZhCN => "消息已转发：{MSG}",
        },
        "builtin.esc_with_prompt_sent_codex" => match lang {
            Language::En => "Message forwarded: {MSG}",
            Language::ZhCN => "消息已转发：{MSG}",
        },
        "builtin.esc_with_prompt_sent_opencode" => match lang {
            Language::En => "Message forwarded: {MSG}",
            Language::ZhCN => "消息已转发：{MSG}",
        },
        "builtin.esc_with_prompt_sent_kimi" => match lang {
            Language::En => "Message forwarded: {MSG}",
            Language::ZhCN => "消息已转发：{MSG}",
        },
        "builtin.esc_with_prompt_sent_gemini" => match lang {
            Language::En => "Message forwarded: {MSG}",
            Language::ZhCN => "消息已转发：{MSG}",
        },
        "builtin.esc_with_prompt_sent_qoder" => match lang {
            Language::En => "Message forwarded: {MSG}",
            Language::ZhCN => "消息已转发：{MSG}",
        },
        "builtin.failed_esc" => match lang {
            Language::En => "Failed to send ESC: {ERR}",
            Language::ZhCN => "发送 ESC 失败: {ERR}",
        },
        "builtin.stop_sent" => match lang {
            Language::En => "Stop sent — generation interrupted.",
            Language::ZhCN => "Stop 已发送 — 已中断当前输出。",
        },
        "builtin.stop_sent_claude" => match lang {
            Language::En => "Stop signal sent to Claude (best-effort). stream-json mode may not abort a running tool immediately; wait for the turn to end if nothing changes.",
            Language::ZhCN => "已向 Claude 发送 stop 信号（best-effort）。stream-json 模式下正在运行的工具可能不会立刻中止；若无变化请等待当前 turn 结束。",
        },
        "builtin.stop_sent_cursor" => match lang {
            Language::En => "Stop sent — current generation cancelled via ACP.",
            Language::ZhCN => "Stop 已发送 — 已通过 ACP 取消当前输出。",
        },
        "builtin.stop_sent_pi" => match lang {
            Language::En => "Stop sent — current operation aborted.",
            Language::ZhCN => "Stop 已发送 — 已中止当前操作。",
        },
        "builtin.stop_sent_codex" => match lang {
            Language::En => "Stop sent — current generation cancelled via ACP.",
            Language::ZhCN => "Stop 已发送 — 已通过 ACP 取消当前输出。",
        },
        "builtin.stop_sent_opencode" => match lang {
            Language::En => "Stop sent — current generation cancelled via ACP.",
            Language::ZhCN => "Stop 已发送 — 已通过 ACP 取消当前输出。",
        },
        "builtin.stop_sent_kimi" => match lang {
            Language::En => "Stop sent — current generation cancelled via ACP.",
            Language::ZhCN => "Stop 已发送 — 已通过 ACP 取消当前输出。",
        },
        "builtin.stop_sent_gemini" => match lang {
            Language::En => "Stop sent — current generation cancelled via ACP.",
            Language::ZhCN => "Stop 已发送 — 已通过 ACP 取消当前输出。",
        },
        "builtin.stop_sent_qoder" => match lang {
            Language::En => "Stop sent — current generation cancelled via ACP.",
            Language::ZhCN => "Stop 已发送 — 已通过 ACP 取消当前输出。",
        },
        "builtin.stop_already_idle" => match lang {
            Language::En => "Agent is already idle — nothing to stop.",
            Language::ZhCN => "智能体已就绪，无需停止。",
        },
        "builtin.stop_already_idle_claude" => match lang {
            Language::En => "Claude is idle — there is no output to stop.",
            Language::ZhCN => "Claude 当前空闲，没有可中断的输出。",
        },
        "builtin.failed_stop_generation" => match lang {
            Language::En => "Failed to stop generation: {ERR}",
            Language::ZhCN => "停止输出失败: {ERR}",
        },
        "builtin.compact_ok" => match lang {
            Language::En => "Context compaction started.",
            Language::ZhCN => "已开始压缩会话上下文。",
        },
        "builtin.compact_ok_with_summary" => match lang {
            Language::En => "Context compacted.\n\n{SUMMARY}",
            Language::ZhCN => "会话上下文已压缩。\n\n{SUMMARY}",
        },
        "builtin.compact_not_supported" => match lang {
            Language::En => "{NAME} does not support /compact in cc-gateway. Use Claude or Pi, or start a new session with /clear.",
            Language::ZhCN => "{NAME} 暂不支持 /compact。请使用 Claude 或 Pi，或用 /clear 开启新会话。",
        },
        "builtin.failed_compact" => match lang {
            Language::En => "Failed to compact context: {ERR}",
            Language::ZhCN => "压缩上下文失败: {ERR}",
        },
        "builtin.init_not_supported" => match lang {
            Language::En => "{NAME} does not support /init in cc-gateway. Use Claude to generate or update CLAUDE.md in the project.",
            Language::ZhCN => "{NAME} 暂不支持 /init。请使用 Claude 在当前项目生成或更新 CLAUDE.md 记忆文件。",
        },
        "builtin.failed_init" => match lang {
            Language::En => "Failed to run /init: {ERR}",
            Language::ZhCN => "执行 /init 失败: {ERR}",
        },
        "builtin.context_cleared" => match lang {
            Language::En => "Context cleared.",
            Language::ZhCN => "上下文已清理。",
        },
        "builtin.failed_clear" => match lang {
            Language::En => "Failed to clear context: {ERR}",
            Language::ZhCN => "清理上下文失败: {ERR}",
        },
        "builtin.status_no_session" => match lang {
            Language::En => "No active agent session.",
            Language::ZhCN => "无活跃智能体会话。",
        },
        "builtin.status_starting" => match lang {
            Language::En => "Agent is starting up...",
            Language::ZhCN => "智能体正在启动中...",
        },
        "builtin.status_ready" => match lang {
            Language::En => "Agent is ready (idle).",
            Language::ZhCN => "智能体就绪（空闲）。",
        },
        "builtin.status_busy" => match lang {
            Language::En => "Agent is generating output...",
            Language::ZhCN => "智能体正在输出中...",
        },
        "builtin.cd_usage" => match lang {
            Language::En => "Usage: /cd <path>",
            Language::ZhCN => "用法: /cd <路径>",
        },
        "builtin.invalid_path" => match lang {
            Language::En => "Invalid path: {PATH}",
            Language::ZhCN => "无效路径: {PATH}",
        },
        "builtin.dir_changed" => match lang {
            Language::En => "Working directory changed to: {PATH}",
            Language::ZhCN => "工作目录已更改为: {PATH}",
        },
        "builtin.session_started" => match lang {
            Language::En => "{NAME} session started in: {DIR}\n\n\x1b[2m\u{1F4A1} Type anything and press Enter to chat.\x1b[0m",
            Language::ZhCN => "{NAME} 会话已启动于: {DIR}\n\n\x1b[2m\u{1F4A1} 输入任意内容并按 Enter 开始对话。\x1b[0m",
        },
        "builtin.session_resumed" => match lang {
            Language::En => "{NAME} session resumed in: {DIR}\n\n\x1b[2m\u{1F4A1} Type anything and press Enter to chat.\x1b[0m",
            Language::ZhCN => "{NAME} 会话已恢复于: {DIR}\n\n\x1b[2m\u{1F4A1} 输入任意内容并按 Enter 开始对话。\x1b[0m",
        },
        "builtin.session_restarted_pi_hint" => match lang {
            Language::En => "Note: Pi cannot restore previous conversations yet. A new Pi session was started (earlier messages in this chat are from gateway history only).",
            Language::ZhCN => "提示：Pi 暂不支持恢复历史对话，已为你启动全新 Pi 会话（此前记录仅保存在网关侧，无法由 Pi 继续）。",
        },
        "builtin.pi_resume_not_supported" => match lang {
            Language::En => "Pi cannot restore previous conversations yet. Use /agent pi to start a new session.",
            Language::ZhCN => "Pi 暂不支持恢复历史对话，请使用 /agent pi 启动新会话。",
        },
        "builtin.failed_resume_session" => match lang {
            Language::En => "Failed to resume session: {ERR}",
            Language::ZhCN => "恢复会话失败: {ERR}",
        },
        "builtin.failed_start_agent" => match lang {
            Language::En => "Failed to start {NAME}: {ERR}",
            Language::ZhCN => "启动 {NAME} 失败: {ERR}",
        },
        "agent.acp_no_session_id" => match lang {
            Language::En => "The agent did not return a session id after connect. Try starting a new session, or update the agent CLI.",
            Language::ZhCN => "智能体连接后未返回会话 ID。请尝试新建会话，或更新智能体 CLI。",
        },
        "agent.acp_request_timeout" => match lang {
            Language::En => "Agent connection timed out (often during session load with long history). Try again or start a new session.",
            Language::ZhCN => "连接智能体超时（恢复长会话历史时较常见）。请重试或新建会话。",
        },
        "agent.acp_no_response" => match lang {
            Language::En => "{NAME} returned no output for this turn. Retry, or run the provider CLI in a terminal for details.",
            Language::ZhCN => "{NAME} 本轮未返回任何内容。请重试，或在终端运行该 CLI 查看详细错误。",
        },
        "agent.acp_prompt_idle_timeout" => match lang {
            Language::En => "The agent sent nothing for 10 minutes; this turn was ended. Retry, or use /stop to terminate the session.",
            Language::ZhCN => "智能体连续 10 分钟无任何输出，本轮已结束。请重试，或用 /stop 终止会话。",
        },
        "agent.turn_stalled" => match lang {
            Language::En => "No agent output for 15 minutes — stopped forwarding this turn. Any remaining output will be delivered with your next message; use /stop to terminate.",
            Language::ZhCN => "智能体长时间（15 分钟）无输出，本轮转发已停止；剩余输出将随下一条消息补发。可用 /stop 终止会话。",
        },
        "agent.kimi_auth_required" => match lang {
            Language::En => "Kimi Code is not signed in. Run `kimi login` in a terminal, or check your Kimi account.",
            Language::ZhCN => "Kimi Code 未登录。请在终端运行 `kimi login`，或检查 Kimi 账号状态。",
        },
        "agent.kimi_subscription_required" => match lang {
            Language::En => "Kimi Code requires an active subscription or plan. Check your Kimi account billing.",
            Language::ZhCN => "Kimi Code 需要有效套餐或订阅。请检查 Kimi 账号的套餐/订阅状态。",
        },
        "agent.kimi_no_response" => match lang {
            Language::En => "Kimi returned no output for this turn. If your membership is inactive, check your Kimi account subscription; otherwise retry or run `kimi -p \"test\"` in a terminal for details.",
            Language::ZhCN => "Kimi 本轮未返回任何内容。若套餐未生效，请检查 Kimi 账号订阅状态；否则请重试，或在终端运行 `kimi -p \"test\"` 查看详细错误。",
        },
        "agent.gemini_auth_required" => match lang {
            Language::En => "Gemini CLI is not signed in. Run `gemini` in a terminal to sign in (or set GEMINI_API_KEY), then retry.",
            Language::ZhCN => "Gemini CLI 未登录。请在终端运行 `gemini` 完成登录（或设置 GEMINI_API_KEY）后重试。",
        },
        "agent.auth_required" => match lang {
            Language::En => "The agent CLI is not signed in. Sign in with the provider's CLI in a terminal, then retry.",
            Language::ZhCN => "智能体 CLI 未登录。请在终端使用对应 CLI 完成登录后重试。",
        },
        "agent.subscription_required" => match lang {
            Language::En => "The provider account has a plan, billing, or quota issue. Check your account status, then retry.",
            Language::ZhCN => "提供方账号存在套餐/订阅或配额问题。请检查账号状态后重试。",
        },
        "agent.process_exited_no_stderr" => match lang {
            Language::En => "The agent process exited immediately after start; no error output was captured. Check that the CLI is installed and on PATH.",
            Language::ZhCN => "智能体进程启动后立刻退出，且未捕获错误输出。请确认 CLI 已安装并在 PATH 中。",
        },
        "agent.process_exited" => match lang {
            Language::En => "The agent process exited immediately after start: {DETAIL}",
            Language::ZhCN => "智能体进程启动后立刻退出: {DETAIL}",
        },
        "agent.spawn_failed" => match lang {
            Language::En => "Failed to start the agent process: {DETAIL}",
            Language::ZhCN => "无法启动智能体进程: {DETAIL}",
        },
        "session.agent_not_found" => match lang {
            Language::En => "Session not found: {ID}",
            Language::ZhCN => "未找到会话: {ID}",
        },
        "session.provider_disabled" => match lang {
            Language::En => "Provider \"{NAME}\" is disabled in settings. Enable it to resume this session.",
            Language::ZhCN => "配置中已禁用智能体「{NAME}」。请在设置中启用后再恢复此会话。",
        },
        "builtin.current_dir" => match lang {
            Language::En => "Current directory: {DIR}",
            Language::ZhCN => "当前目录: {DIR}",
        },
        "builtin.access_denied" => match lang {
            Language::En => "Access denied: {ERR}",
            Language::ZhCN => "访问被拒绝: {ERR}",
        },
        "builtin.failed_list_dir" => match lang {
            Language::En => "Failed to list directory: {ERR}",
            Language::ZhCN => "列出目录失败: {ERR}",
        },
        "builtin.no_subdirs" => match lang {
            Language::En => "No subdirectories found.",
            Language::ZhCN => "未找到子目录。",
        },
        "builtin.dir_list_header" => match lang {
            Language::En => "Directories under {DIR}:",
            Language::ZhCN => "{DIR} 下的目录：",
        },
        "builtin.dir_list_hint" => match lang {
            Language::En => "Use /cd <path> to change directory.",
            Language::ZhCN => "使用 /cd <路径> 切换目录。",
        },
        "builtin.agents_pick_hint" => match lang {
            Language::En => "Use /agents <provider> (e.g. /agents claude).",
            Language::ZhCN => "使用 /agents <智能体>（例如 /agents claude）。",
        },
        "builtin.agent_history_hint" => match lang {
            Language::En => "Use /agent-history <n> to resume, or /agent-history <n> new to start a new session in that record's work dir.",
            Language::ZhCN => "使用 /agent-history <n> 恢复；或用 /agent-history <n> new 在该记录的工作目录新起会话。",
        },
        "builtin.changed_dir" => match lang {
            Language::En => "Changed directory to: {PATH}",
            Language::ZhCN => "目录已更改为: {PATH}",
        },
        "builtin.cannot_delete_active" => match lang {
            Language::En => "Cannot delete an active session. Use /quit to stop it first.",
            Language::ZhCN => "无法删除活跃中的会话，请先使用 /quit 退出。",
        },
        "builtin.session_deleted" => match lang {
            Language::En => "Session deleted.",
            Language::ZhCN => "会话已删除。",
        },
        "builtin.no_active_session_to_quit" => match lang {
            Language::En => "No active session to quit. Use /quit in an active session or type /help for available commands.",
            Language::ZhCN => "没有可退出的活动会话。请在活动会话中使用 /quit，或输入 /help 查看可用命令。",
        },
        "builtin.mkdir_usage" => match lang {
            Language::En => "Usage: /mkdir <dirname>",
            Language::ZhCN => "用法: /mkdir <目录名>",
        },
        "builtin.dir_created" => match lang {
            Language::En => "Directory created: {PATH}",
            Language::ZhCN => "目录已创建: {PATH}",
        },
        "builtin.failed_create_dir" => match lang {
            Language::En => "Failed to create directory: {ERR}",
            Language::ZhCN => "创建目录失败: {ERR}",
        },
        "builtin.thinking_enabled" => match lang {
            Language::En => "Thinking display enabled.",
            Language::ZhCN => "已启用 Thinking 显示。",
        },
        "builtin.thinking_disabled" => match lang {
            Language::En => "Thinking display disabled.",
            Language::ZhCN => "已禁用 Thinking 显示。",
        },
        "builtin.no_sessions" => match lang {
            Language::En => "No sessions found.",
            Language::ZhCN => "未找到会话。",
        },
        "builtin.status_active" => match lang {
            Language::En => "Active",
            Language::ZhCN => "活跃",
        },
        "builtin.status_inactive" => match lang {
            Language::En => "Inactive",
            Language::ZhCN => "非活跃",
        },
        "builtin.start_new_session_hint" => match lang {
            Language::En => "Use /agent to start a new session.",
            Language::ZhCN => "使用 /agent 开始新会话。",
        },
        "builtin.recent_agent_sessions" => match lang {
            Language::En => "Recent agent sessions:",
            Language::ZhCN => "最近的智能体会话:",
        },
        "builtin.resume_hint" => match lang {
            Language::En => "Use /agent-history <n> to select a session to resume.",
            Language::ZhCN => "使用 /agent-history <n> 选择要恢复的会话。",
        },
        "builtin.resume_session_set" => match lang {
            Language::En => "Resume session set: {SID}",
            Language::ZhCN => "恢复会话已设置: {SID}",
        },
        "builtin.resume_session_missing_id" => match lang {
            Language::En => "Cannot resume session {SID}: provider session id is missing. Use /agent to start a new session.",
            Language::ZhCN => "无法恢复会话 {SID}：缺少 provider 会话 ID。请使用 /agent 新建会话。",
        },
        "builtin.invalid_history_index" => match lang {
            Language::En => "Invalid history index.",
            Language::ZhCN => "无效的历史索引。",
        },
        "builtin.no_history_file" => match lang {
            Language::En => "No Claude history file found.",
            Language::ZhCN => "未找到 Claude 历史文件。",
        },
        "builtin.failed_read_history" => match lang {
            Language::En => "Failed to read history: {ERR}",
            Language::ZhCN => "读取历史失败: {ERR}",
        },
        // command/forward.rs
        "forward.no_session" => match lang {
            Language::En => "No active agent session. Use /agent to start one, or type a builtin command like /help.\n\nYou said: {MSG}",
            Language::ZhCN => "没有活动的智能体会话。使用 /agent 启动一个，或输入内置命令如 /help。\n\n您说: {MSG}",
        },
        "forward.failed_send" => match lang {
            Language::En => "Failed to send message: {ERR}",
            Language::ZhCN => "发送消息失败: {ERR}",
        },

        // claude/controller.rs
        "controller.access_denied" => match lang {
            Language::En => "Access denied: '{PATH}' is outside home directory '{HOME}'",
            Language::ZhCN => "访问被拒绝: '{PATH}' 不在主目录 '{HOME}' 下",
        },
        "controller.no_active_session" => match lang {
            Language::En => "No active agent session. Use /agent to start one.",
            Language::ZhCN => "没有活动的智能体会话。使用 /agent 启动一个。",
        },
        "controller.no_pending_request" => match lang {
            Language::En => "No pending permission request. The request may have already timed out or been handled.",
            Language::ZhCN => "没有待处理的权限请求。请求可能已超时或被处理。",
        },
        "controller.permission_allowed" => match lang {
            Language::En => "Permission allowed (request: {ID}).",
            Language::ZhCN => "已允许权限 (请求: {ID})。",
        },
        "controller.permission_denied" => match lang {
            Language::En => "Permission denied (request: {ID}).",
            Language::ZhCN => "已拒绝权限 (请求: {ID})。",
        },
        "controller.failed_permission" => match lang {
            Language::En => "Failed to respond to permission request: {ERR}",
            Language::ZhCN => "响应权限请求失败: {ERR}",
        },

        // platform/feishu.rs
        "feishu.permission_title" => match lang {
            Language::En => "Agent Tool Permission Request",
            Language::ZhCN => "智能体工具权限请求",
        },
        "feishu.permission_subtitle" => match lang {
            Language::En => "Tool: {NAME}",
            Language::ZhCN => "工具: {NAME}",
        },
        "feishu.request_id_label" => match lang {
            Language::En => "**Request ID:** `{ID}`",
            Language::ZhCN => "**请求 ID:** `{ID}`",
        },
        "feishu.tool_input_label" => match lang {
            Language::En => "**Tool Input:**\n```json\n{INPUT}\n```",
            Language::ZhCN => "**工具输入:**\n```json\n{INPUT}\n```",
        },
        "feishu.approve_once" => match lang {
            Language::En => "Approve Once",
            Language::ZhCN => "批准一次",
        },
        "feishu.approve_session" => match lang {
            Language::En => "Approve Session",
            Language::ZhCN => "批准本次会话",
        },
        "feishu.approve_always" => match lang {
            Language::En => "Approve Always",
            Language::ZhCN => "始终批准",
        },
        "feishu.deny" => match lang {
            Language::En => "Deny",
            Language::ZhCN => "拒绝",
        },
        "feishu.choose_dir" => match lang {
            Language::En => "Choose a working directory:",
            Language::ZhCN => "选择工作目录:",
        },
        "feishu.select_dir_title" => match lang {
            Language::En => "Select Directory",
            Language::ZhCN => "选择目录",
        },
        "feishu.no_directories" => match lang {
            Language::En => "No directories found.",
            Language::ZhCN => "未找到目录。",
        },
        "feishu.prev_page" => match lang {
            Language::En => "Previous Page",
            Language::ZhCN => "上一页",
        },
        "feishu.next_page" => match lang {
            Language::En => "Next Page",
            Language::ZhCN => "下一页",
        },
        "feishu.page_info" => match lang {
            Language::En => "Page {PAGE} / {TOTAL}",
            Language::ZhCN => "第 {PAGE} 页 / 共 {TOTAL} 页",
        },
        "feishu.dir_changed" => match lang {
            Language::En => "Changed directory to: {PATH}",
            Language::ZhCN => "目录已更改为: {PATH}",
        },

        "feishu.file_from_user" => match lang {
            Language::En => "User sent a file",
            Language::ZhCN => "用户发送了一个文件",
        },
        "feishu.file_received" => match lang {
            Language::En => "File received, Claude is processing...",
            Language::ZhCN => "已收到文件，Claude 正在处理...",
        },
        "feishu.session_history_title" => match lang {
            Language::En => "Claude Session History",
            Language::ZhCN => "Claude 会话历史",
        },
        "feishu.session_history_subtitle" => match lang {
            Language::En => "Sessions for this chat",
            Language::ZhCN => "此聊天的会话",
        },
        "feishu.no_sessions" => match lang {
            Language::En => "No sessions found for this chat.",
            Language::ZhCN => "未找到此聊天的会话。",
        },
        "feishu.resume" => match lang {
            Language::En => "Resume",
            Language::ZhCN => "恢复",
        },
        "feishu.start_new_session" => match lang {
            Language::En => "Start New Session",
            Language::ZhCN => "开始新会话",
        },
        "feishu.delete_session" => match lang {
            Language::En => "Delete",
            Language::ZhCN => "删除",
        },
        "feishu.session_deleted" => match lang {
            Language::En => "Session deleted.",
            Language::ZhCN => "会话已删除。",
        },
        "feishu.cannot_delete_active" => match lang {
            Language::En => "Cannot delete an active session. Use /quit to stop it first.",
            Language::ZhCN => "无法删除活跃中的会话，请先使用 /quit 退出。",
        },
        "feishu.status_active" => match lang {
            Language::En => "Active",
            Language::ZhCN => "活跃",
        },
        "feishu.status_inactive" => match lang {
            Language::En => "Inactive",
            Language::ZhCN => "非活跃",
        },
        "feishu.error_generic" => match lang {
            Language::En => "Error: {ERR}",
            Language::ZhCN => "错误: {ERR}",
        },
        "feishu.failed_send" => match lang {
            Language::En => "Failed to send: {ERR}",
            Language::ZhCN => "发送失败: {ERR}",
        },
        "feishu.select_button" => match lang {
            Language::En => "Select",
            Language::ZhCN => "选择",
        },
        "feishu.allow_button" => match lang {
            Language::En => "Allow",
            Language::ZhCN => "允许",
        },
        "feishu.deny_button" => match lang {
            Language::En => "Deny",
            Language::ZhCN => "拒绝",
        },
        "feishu.select_title" => match lang {
            Language::En => "Please Select",
            Language::ZhCN => "请选择",
        },
        "feishu.permission_request_text" => match lang {
            Language::En => "Permission request: {NAME} ({ID})",
            Language::ZhCN => "权限请求: {NAME} ({ID})",
        },
        "feishu.card_dir_changed_title" => match lang {
            Language::En => "Directory Changed",
            Language::ZhCN => "目录已切换",
        },
        "feishu.card_agent_set_title" => match lang {
            Language::En => "Agent Set",
            Language::ZhCN => "智能体已设置",
        },
        "feishu.card_session_deleted_title" => match lang {
            Language::En => "Session Deleted",
            Language::ZhCN => "会话已删除",
        },
        "feishu.card_starting_title" => match lang {
            Language::En => "Starting Session",
            Language::ZhCN => "正在启动会话",
        },
        "feishu.card_resumed_title" => match lang {
            Language::En => "Session Resumed",
            Language::ZhCN => "会话已恢复",
        },
        "feishu.card_started_title" => match lang {
            Language::En => "Session Started",
            Language::ZhCN => "会话已启动",
        },
        "feishu.card_allowed" => match lang {
            Language::En => "Permission allowed.",
            Language::ZhCN => "已允许该操作。",
        },
        "feishu.card_denied" => match lang {
            Language::En => "Permission denied.",
            Language::ZhCN => "已拒绝该操作。",
        },
        "feishu.card_selected_title" => match lang {
            Language::En => "Selected",
            Language::ZhCN => "已选择",
        },
        "feishu.card_selected" => match lang {
            Language::En => "Option selected.",
            Language::ZhCN => "已选择该选项。",
        },
        "feishu.card_processing" => match lang {
            Language::En => "Processing...",
            Language::ZhCN => "处理中...",
        },
        "feishu.card_starting" => match lang {
            Language::En => "Starting a new session, please wait...",
            Language::ZhCN => "正在启动新会话，请稍候...",
        },
        // platform/telegram
        "telegram.error_generic" => match lang {
            Language::En => "Error: {ERR}",
            Language::ZhCN => "错误: {ERR}",
        },
        "telegram.poll_network_hint" => match lang {
            Language::En => "Hint: cannot reach api.telegram.org (network/proxy). Set `telegram.proxy` in config (e.g. http://127.0.0.1:7890) and restart the daemon.",
            Language::ZhCN => "提示：无法连接 api.telegram.org（网络/代理问题）。可在 config.json 配置 `telegram.proxy`（如 http://127.0.0.1:7890）并重启 daemon。",
        },
        "telegram.private_chat_only" => match lang {
            Language::En => "Please use in private chat.",
            Language::ZhCN => "请在私聊中使用。",
        },
        "telegram.history_unavailable" => match lang {
            Language::En => "Agent history is not available in Telegram.",
            Language::ZhCN => "Telegram 中不可用智能体历史记录。",
        },
        "telegram.shutdown_notice" => match lang {
            Language::En => "Bot is shutting down, sessions exited.",
            Language::ZhCN => "机器人正在关闭，会话已退出。",
        },
        "qq.choose_agent" => match lang {
            Language::En => "Choose default agent (reply with number or name):",
            Language::ZhCN => "选择默认智能体（回复序号或名称）：",
        },
        "qq.use_agents_hint" => match lang {
            Language::En => "Tip: /agents <name> sets the default for this chat.",
            Language::ZhCN => "提示：/agents <名称> 可设置本聊天默认智能体。",
        },
        "qq.choose_directory" => match lang {
            Language::En => "Choose working directory: {DIR}",
            Language::ZhCN => "选择工作目录：{DIR}",
        },
        "qq.shutdown_notice" => match lang {
            Language::En => "QQ bot is shutting down, sessions exited.",
            Language::ZhCN => "QQ 机器人正在关闭，会话已退出。",
        },
        "qq.permission_request" => match lang {
            Language::En => "Permission request: `{NAME}` (ID: `{ID}`). QQ does not support in-chat approval — use WebUI, Feishu, or Telegram.",
            Language::ZhCN => "权限请求: `{NAME}` (ID: `{ID}`)。QQ 暂不支持在聊天内批准，请使用 WebUI、飞书或 Telegram。",
        },
        "qq.sent_file_caption" => match lang {
            Language::En => "File: {NAME}",
            Language::ZhCN => "文件：{NAME}",
        },
        "qq.send_file_group_unsupported" => match lang {
            Language::En => "This file type cannot be sent in QQ groups (images/videos/voice only). Use private chat (C2C) or send a PNG/JPG.",
            Language::ZhCN => "该文件类型无法发到 QQ 群（群聊仅支持图片/视频/语音富媒体）。请私聊发送，或改用 PNG/JPG 图片。",
        },
        "qq.send_image_format_unsupported" => match lang {
            Language::En => "QQ inline images must be PNG or JPG (per QQ Bot API). Convert the file or send it as a document in private chat.",
            Language::ZhCN => "QQ 内联图片仅支持 PNG/JPG（官方富媒体规范）。请转换格式，或在私聊中以文件形式发送。",
        },
        "qq.send_image_group_unsupported" => match lang {
            Language::En => "This image format cannot be shown inline in QQ groups (PNG/JPG only). Use private chat or convert to PNG/JPG.",
            Language::ZhCN => "该图片格式无法在 QQ 群内联展示（群聊仅支持 PNG/JPG）。请私聊发送或转换为 PNG/JPG。",
        },
        "feishu.image_too_large" => match lang {
            Language::En => "Image exceeds Feishu upload limit ({MB} MB). Use a smaller file or send as a document via the file API.",
            Language::ZhCN => "图片超过飞书上传限制（{MB} MB）。请缩小文件，或通过文件接口发送。",
        },
        "qq.group_chat_unsupported" => match lang {
            Language::En => "QQ group chat is not supported. Please DM (C2C) the bot instead.",
            Language::ZhCN => "暂不支持 QQ 群聊通道，请改为私聊（C2C）机器人。",
        },
        "telegram.permission_request" => match lang {
            Language::En => "Permission request: `{NAME}`\nID: `{ID}`",
            Language::ZhCN => "权限请求: `{NAME}`\nID: `{ID}`",
        },
        "telegram.allow_button" => match lang {
            Language::En => "Allow",
            Language::ZhCN => "允许",
        },
        "telegram.deny_button" => match lang {
            Language::En => "Deny",
            Language::ZhCN => "拒绝",
        },
        "telegram.permission_responded" => match lang {
            Language::En => "Request {ID} {ACTION}.",
            Language::ZhCN => "请求 {ID} 已{ACTION}。",
        },
        "cursor.resume_may_ignore_flags" => match lang {
            Language::En => "Note: Cursor session resume may not honor CLI flags like --yolo/--print. If the resumed session behaves unexpectedly, create a new session.",
            Language::ZhCN => "提示：Cursor 会话恢复可能不会完全生效 CLI 启动参数（如 --yolo/--print）。如果恢复后行为异常，请新建会话。",
        },
        "cursor.session_resume_failed" => match lang {
            Language::En => "Failed to restore Cursor session ({ID}): {ERR}. Use /agent to start a new session if needed.",
            Language::ZhCN => "无法恢复 Cursor 会话（{ID}）：{ERR}。如需新会话请使用 /agent。",
        },
        "codex.session_resume_failed" => match lang {
            Language::En => "Failed to restore Codex session ({ID}): {ERR}. Use /agent to start a new session if needed.",
            Language::ZhCN => "无法恢复 Codex 会话（{ID}）：{ERR}。如需新会话请使用 /agent。",
        },
        "opencode.session_resume_failed" => match lang {
            Language::En => "Failed to restore OpenCode session ({ID}): {ERR}. Use /agent to start a new session if needed.",
            Language::ZhCN => "无法恢复 OpenCode 会话（{ID}）：{ERR}。如需新会话请使用 /agent。",
        },
        "kimi.session_resume_failed" => match lang {
            Language::En => "Failed to restore Kimi session ({ID}): {ERR}. Use /agent to start a new session if needed.",
            Language::ZhCN => "无法恢复 Kimi 会话（{ID}）：{ERR}。如需新会话请使用 /agent。",
        },
        "gemini.session_resume_failed" => match lang {
            Language::En => "Failed to restore Gemini session ({ID}): {ERR}. Use /agent to start a new session if needed.",
            Language::ZhCN => "无法恢复 Gemini 会话（{ID}）：{ERR}。如需新会话请使用 /agent。",
        },
        "qoder.session_resume_failed" => match lang {
            Language::En => "Failed to restore Qoder session ({ID}): {ERR}. Use /agent to start a new session if needed.",
            Language::ZhCN => "无法恢复 Qoder 会话（{ID}）：{ERR}。如需新会话请使用 /agent。",
        },
        "pi.session_resume_failed" => match lang {
            Language::En => "Failed to restore Pi session ({PATH}): {ERR}. Use /agent to start a new session if needed.",
            Language::ZhCN => "无法恢复 Pi 会话（{PATH}）：{ERR}。如需新会话请使用 /agent。",
        },
        "pi.session_resume_timeout" => match lang {
            Language::En => "Pi did not respond in time while loading the session (large history can take up to 2 minutes). Try again or use /agent to start a new session.",
            Language::ZhCN => "Pi 加载会话超时（历史较长时可能需要更久）。请重试，或使用 /agent 新建会话。",
        },
        "telegram.command_help" => match lang {
            Language::En => "Show help",
            Language::ZhCN => "查看帮助",
        },
        "telegram.help_title" => match lang {
            Language::En => "cc-gateway commands (Telegram):",
            Language::ZhCN => "cc-gateway 命令（Telegram）：",
        },
        "telegram.help_footer" => match lang {
            Language::En => "Any other text will be forwarded to the active agent.",
            Language::ZhCN => "其他文本将直接发送给活动智能体。",
        },
        "telegram.help_text" => match lang {
            Language::En => "cc-gateway commands (Telegram):\n/help  Show help\n/pwd   Show current directory\n/ll    List directories\n/cd    Pick directory\n/mkdir Create directory\n/agent Start agent session\n/agents Set default agent\n/agent_history Show recent sessions\n/esc   Flush queued messages\n/stop  Stop current generation\n/clear Clear context\n/models List/switch models\n/status Show status\n/show_thinking Show thinking\n/hide_thinking Hide thinking\n/quit  Stop active session\n\nTip: type /agent <provider> or /agents <provider> to pick a specific agent (e.g. claude, cursor, pi, opencode, kimi, gemini).\nAny other text will be forwarded to the active agent.",
            Language::ZhCN => "cc-gateway 命令（Telegram）：\n/help  显示帮助\n/pwd   显示当前目录\n/ll    列出目录\n/cd    选择目录\n/mkdir 创建目录\n/agent 启动智能体会话\n/agents 设置本聊天默认智能体\n/agent_history 显示最近会话\n/esc   强推排队消息\n/stop  停止当前输出\n/clear 清理上下文\n/models 列出/切换模型\n/status 显示状态\n/show_thinking 显示 Thinking\n/hide_thinking 隐藏 Thinking\n/quit  停止当前会话\n\n提示：可直接输入 /agent <智能体> 或 /agents <智能体> 指定智能体（如 claude、cursor、pi、opencode、kimi、gemini）。\n其他文本将直接发送给活动智能体。",
        },
        "telegram.command_pwd" => match lang {
            Language::En => "Show current directory",
            Language::ZhCN => "查看当前目录",
        },
        "telegram.command_ll" => match lang {
            Language::En => "List folders",
            Language::ZhCN => "列出文件夹",
        },
        "telegram.command_cd" => match lang {
            Language::En => "Pick directory (folder list)",
            Language::ZhCN => "选择目录（文件夹列表）",
        },
        "telegram.command_cd_up" => match lang {
            Language::En => "Go to parent directory (/cd ..)",
            Language::ZhCN => "返回上级目录 (/cd ..)",
        },
        "telegram.command_cd_default" => match lang {
            Language::En => "Return to default directory",
            Language::ZhCN => "返回默认目录",
        },
        "telegram.command_mkdir" => match lang {
            Language::En => "Create directory",
            Language::ZhCN => "创建目录",
        },
        "telegram.command_agent" => match lang {
            Language::En => "Start agent session (or /agent <provider>)",
            Language::ZhCN => "启动智能体会话（也可 /agent <智能体>）",
        },
        "telegram.command_agents" => match lang {
            Language::En => "Set default agent (or /agents <provider>)",
            Language::ZhCN => "设置默认智能体（也可 /agents <智能体>）",
        },
        "telegram.command_agent_history" => match lang {
            Language::En => "Show agent history",
            Language::ZhCN => "查看智能体历史",
        },
        "telegram.command_show_thinking" => match lang {
            Language::En => "Show thinking",
            Language::ZhCN => "显示 Thinking",
        },
        "telegram.command_hide_thinking" => match lang {
            Language::En => "Hide thinking",
            Language::ZhCN => "隐藏 Thinking",
        },
        "telegram.command_quit" => match lang {
            Language::En => "Stop active session",
            Language::ZhCN => "停止当前会话",
        },
        "telegram.command_esc" => match lang {
            Language::En => "Flush queued messages",
            Language::ZhCN => "强推排队消息",
        },
        "telegram.command_stop" => match lang {
            Language::En => "Stop current generation",
            Language::ZhCN => "停止当前输出",
        },
        "telegram.command_clear" => match lang {
            Language::En => "Clear context",
            Language::ZhCN => "清理上下文",
        },
        "telegram.command_status" => match lang {
            Language::En => "Show status",
            Language::ZhCN => "显示状态",
        },
        "telegram.command_models" => match lang {
            Language::En => "List or switch models",
            Language::ZhCN => "列出 / 切换模型",
        },
        "telegram.choose_directory" => match lang {
            Language::En => "Choose a directory in {DIR}:",
            Language::ZhCN => "请选择 {DIR} 下的目录:",
        },
        "telegram.choose_history" => match lang {
            Language::En => "Choose a Claude session to resume:",
            Language::ZhCN => "请选择要恢复的 Claude 会话:",
        },
        "telegram.session_history_subtitle" => match lang {
            Language::En => "Sessions for this chat",
            Language::ZhCN => "此聊天的会话",
        },
        "telegram.resume" => match lang {
            Language::En => "Resume",
            Language::ZhCN => "恢复",
        },
        "telegram.start_new_session" => match lang {
            Language::En => "New",
            Language::ZhCN => "新开",
        },
        "telegram.delete_session" => match lang {
            Language::En => "Delete",
            Language::ZhCN => "删除",
        },
        "telegram.session_deleted" => match lang {
            Language::En => "Session deleted.",
            Language::ZhCN => "会话已删除。",
        },
        "telegram.cannot_delete_active" => match lang {
            Language::En => "Cannot delete an active session. Use /quit to stop it first.",
            Language::ZhCN => "无法删除活跃中的会话，请先使用 /quit 退出。",
        },
        "telegram.card_allowed" => match lang {
            Language::En => "Allowed.",
            Language::ZhCN => "已允许。",
        },
        "telegram.card_denied" => match lang {
            Language::En => "Denied.",
            Language::ZhCN => "已拒绝。",
        },
        "telegram.callback_expired" => match lang {
            Language::En => "This action has expired. Please run the command again.",
            Language::ZhCN => "这个操作已过期，请重新执行命令。",
        },

        // web/handlers/session.rs
        "webui.permission_request" => match lang {
            Language::En => "Permission request: {NAME}\nID: {ID}",
            Language::ZhCN => "权限请求: {NAME}\nID: {ID}",
        },
        "webui.permission_request_input" => match lang {
            Language::En => "Input:",
            Language::ZhCN => "输入:",
        },
        "webui.confirm_request" => match lang {
            Language::En => "Confirm: {PROMPT} (id: {ID})\nOptions: {OPTIONS}\n",
            Language::ZhCN => "确认: {PROMPT} (id: {ID})\n选项: {OPTIONS}\n",
        },
        "webui.select_request" => match lang {
            Language::En => "Select: {PROMPT} (id: {ID})\nOptions: {OPTIONS}\n",
            Language::ZhCN => "选择: {PROMPT} (id: {ID})\n选项: {OPTIONS}\n",
        },
        "webui.questions_title" => match lang {
            Language::En => "Question (id: {ID})\n",
            Language::ZhCN => "问题 (id: {ID})\n",
        },
        "webui.question_item" => match lang {
            Language::En => "  {HEADER}: {QUESTION}\n",
            Language::ZhCN => "  {HEADER}: {QUESTION}\n",
        },
        "webui.question_option" => match lang {
            Language::En => "    - {LABEL}: {DESCRIPTION}\n",
            Language::ZhCN => "    - {LABEL}: {DESCRIPTION}\n",
        },
        "webui.empty_message" => match lang {
            Language::En => "Empty message",
            Language::ZhCN => "空消息",
        },
        "webui.upload_missing_file" => match lang {
            Language::En => "No file in upload",
            Language::ZhCN => "上传缺少文件",
        },
        "webui.runtime_not_found" => match lang {
            Language::En => "WebUI runtime not found",
            Language::ZhCN => "未找到 WebUI 运行时",
        },
        "webui.no_active_session" => match lang {
            Language::En => "No active session",
            Language::ZhCN => "没有活动会话",
        },
        "webui.session_not_found" => match lang {
            Language::En => "Session not found",
            Language::ZhCN => "未找到会话",
        },
        "webui.cannot_delete_active" => match lang {
            Language::En => "Cannot delete an active session. Stop it first.",
            Language::ZhCN => "无法删除活跃中的会话，请先停止它。",
        },
        "webui.home_dir_error" => match lang {
            Language::En => "Could not determine home directory",
            Language::ZhCN => "无法确定主目录",
        },
        "webui.failed_stop_session" => match lang {
            Language::En => "Failed to stop session: {ERR}",
            Language::ZhCN => "停止会话失败: {ERR}",
        },
        "webui.failed_restart_session" => match lang {
            Language::En => "Failed to restart session: {ERR}",
            Language::ZhCN => "重启会话失败: {ERR}",
        },

        // pairing
        "pairing.wait_message" => match lang {
            Language::En => "Waiting for admin approval. Your pairing code is: {CODE}",
            Language::ZhCN => "等待管理员放行，你的配对码是：{CODE}",
        },
        "pairing.already_pending" => match lang {
            Language::En => "Your pairing request is still pending. Pairing code: {CODE}",
            Language::ZhCN => "你的配对请求仍在等待放行，配对码：{CODE}",
        },

        _ => key,
    }
}

/// Format a translation by replacing `{NAME}` placeholders.
pub fn tfmt(key: &str, replacements: &[(&str, &str)]) -> String {
    let mut result = t(key).to_string();
    for (placeholder, value) in replacements {
        result = result.replace(&format!("{{{}}}", placeholder), value);
    }
    result
}

/// Convenience macro for static translations.
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::i18n::dict::t($key)
    };
}

/// Convenience macro for formatted translations.
/// Usage: `t_fmt!("key", PID = pid, NAME = name)`
#[macro_export]
macro_rules! t_fmt {
    ($key:literal $(, $name:ident = $value:expr)* $(,)?) => {{
        let _args: ::std::vec::Vec<(&str, ::std::string::String)> = ::std::vec![
            $(
                (stringify!($name), ::std::string::ToString::to_string(&$value))
            ),*
        ];
        let _refs: ::std::vec::Vec<(&str, &str)> = _args.iter().map(|(k, v)| (*k, v.as_str())).collect();
        $crate::i18n::dict::tfmt($key, &_refs)
    }};
}
