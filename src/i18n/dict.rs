use super::current_language;
use super::lang::Language;

/// Lookup a translation by key. Returns the key itself if not found.
pub fn t(key: &str) -> &str {
    let lang = current_language();
    match key {
        // daemon/mod.rs
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

        // cli/interactive.rs
        "cli.banner" => match lang {
            Language::En => "cc-gateway interactive mode  Type '/help' for commands, '/quit' to exit.\n",
            Language::ZhCN => "cc-gateway 交互模式  输入 '/help' 查看命令，'/quit' 退出。\n",
        },
        "cli.thinking" => match lang {
            Language::En => "Thinking...",
            Language::ZhCN => "思考中...",
        },
        "builtin.thinking_placeholder" => match lang {
            Language::En => "💭 Thinking...",
            Language::ZhCN => "💭 Thinking...",
        },
        "cli.press_expand" => match lang {
            Language::En => "[press t to expand]",
            Language::ZhCN => "[按 t 展开]",
        },
        "cli.tool_label" => match lang {
            Language::En => "Tool:",
            Language::ZhCN => "工具:",
        },
        "cli.permission_required" => match lang {
            Language::En => "Permission Required",
            Language::ZhCN => "需要权限",
        },
        "cli.request_id" => match lang {
            Language::En => "Request ID:",
            Language::ZhCN => "请求 ID:",
        },
        "cli.allow_deny_hint" => match lang {
            Language::En => "Type /allow or /deny [reason] to respond.",
            Language::ZhCN => "输入 /allow 或 /deny [原因] 来响应。",
        },
        "cli.goodbye" => match lang {
            Language::En => "Goodbye!",
            Language::ZhCN => "再见!",
        },
        "cli.session_stopped" => match lang {
            Language::En => "Agent session stopped. Back to cc-gateway.",
            Language::ZhCN => "智能体会话已停止。返回 cc-gateway。",
        },
        "cli.help_desc" => match lang {
            Language::En => "Show this help",
            Language::ZhCN => "显示此帮助",
        },
        "cli.quit_desc" => match lang {
            Language::En => "Quit session or exit",
            Language::ZhCN => "退出会话或退出程序",
        },
        "cli.cd_desc" => match lang {
            Language::En => "Change working directory",
            Language::ZhCN => "更改工作目录",
        },
        "cli.claude_desc" => match lang {
            Language::En => "Start or restart Claude session",
            Language::ZhCN => "启动或重启 Claude 会话",
        },
        "cli.pwd_desc" => match lang {
            Language::En => "Show current working directory",
            Language::ZhCN => "显示当前工作目录",
        },
        "cli.ll_desc" => match lang {
            Language::En => "List files in current directory",
            Language::ZhCN => "列出当前目录中的文件",
        },

        // command/builtin.rs
        "builtin.help" => match lang {
            Language::En => "cc-gateway commands:\n  /help                Show this help\n  /quit                Quit current agent session\n  /esc [msg]           Flush queued messages (Claude: best-effort)\n  /stop                Stop current generation (Claude: best-effort)\n  /clear               Clear session context\n  /status              Show agent status (ready / busy)\n  /cd <path>           Change working directory\n  /cd_default          Change to default directory\n  /agent [args...]     Start a new agent session\n  /agents              Pick this channel's default agent\n  /pwd                 Show current working directory\n  /ll                  List files in current directory\n  /mkdir <dirname>     Create a new directory\n  /show-thinking       Always show Thinking output when available\n  /hide-thinking       Hide Thinking output\n  /agent-history       Show recent agent sessions\nAny other text is sent directly to the active agent.",
            Language::ZhCN => "cc-gateway 命令:\n  /help                显示此帮助\n  /quit                退出当前智能体会话\n  /esc [消息]           强推排队消息（Claude：best-effort）\n  /stop                停止当前输出（Claude：best-effort）\n  /clear               清理会话上下文\n  /status              显示智能体状态（就绪 / 输出中）\n  /cd <path>           更改工作目录\n  /cd_default          将工作目录更改为默认目录\n  /agent [args...]     启动新的智能体会话\n  /agents              选择本频道默认智能体\n  /pwd                 显示当前工作目录\n  /ll                  列出当前目录中的文件\n  /mkdir <目录名>       创建新目录\n  /show-thinking       始终显示可用的 Thinking 输出\n  /hide-thinking       隐藏 Thinking 输出\n  /agent-history       显示最近的智能体会话\n任何其他文本将直接发送给活动智能体。",
        },
        "builtin.help_title" => match lang {
            Language::En => "cc-gateway commands:",
            Language::ZhCN => "cc-gateway 命令:",
        },
        "builtin.help_help" => match lang {
            Language::En => "  /help                Show this help",
            Language::ZhCN => "  /help                显示此帮助",
        },
        "builtin.help_quit" => match lang {
            Language::En => "  /quit                Quit current claude session or exit cc-gateway (no in feishu)",
            Language::ZhCN => "  /quit                退出当前 Claude会话或退出 cc-gateway (飞书中无效)",
        },
        "builtin.help_cd" => match lang {
            Language::En => "  /cd <path>           Change working directory",
            Language::ZhCN => "  /cd <path>           更改工作目录",
        },
        "builtin.help_cd_default" => match lang {
            Language::En => "  /cd_default          Change working directory to the default directory",
            Language::ZhCN => "  /cd_default          将工作目录更改为默认目录",
        },
        "builtin.help_claude" => match lang {
            Language::En => "  /agent [args...]     Start a new agent session (pass args to the configured CLI)",
            Language::ZhCN => "  /agent [args...]     启动新的智能体会话 (传递参数给配置的 CLI)",
        },
        "builtin.help_pwd" => match lang {
            Language::En => "  /pwd                 Show current working directory",
            Language::ZhCN => "  /pwd                 显示当前工作目录",
        },
        "builtin.help_ll" => match lang {
            Language::En => "  /ll                  List files in current directory (ls -l)",
            Language::ZhCN => "  /ll                  列出当前目录中的文件 (ls -l)",
        },
        "builtin.help_mkdir" => match lang {
            Language::En => "  /mkdir <dirname>     Create a new directory",
            Language::ZhCN => "  /mkdir <目录名>       创建新目录",
        },
        "builtin.help_show_thinking" => match lang {
            Language::En => "  /show-thinking         Always show Claude Thinking content",
            Language::ZhCN => "  /show-thinking         始终显示 Claude Thinking 内容",
        },
        "builtin.help_hide_thinking" => match lang {
            Language::En => "  /hide-thinking         Hide Claude Thinking content (show placeholder only)",
            Language::ZhCN => "  /hide-thinking         隐藏 Claude Thinking 内容（仅显示占位符）",
        },
        "builtin.help_agent_history" => match lang {
            Language::En => "  /agent-history       Show recent agent sessions and resume by index",
            Language::ZhCN => "  /agent-history       显示最近的智能体会话并按索引恢复",
        },
        "builtin.help_agents" => match lang {
            Language::En => "  /agents              Set this channel's default agent (interactive picker)",
            Language::ZhCN => "  /agents              设置本频道默认智能体（交互选择）",
        },
        "builtin.help_esc" => match lang {
            Language::En => "  /esc [msg]           Flush queued messages (Claude: best-effort)",
            Language::ZhCN => "  /esc [消息]           强推排队消息（Claude：best-effort）",
        },
        "builtin.help_clear" => match lang {
            Language::En => "  /clear               Clear context",
            Language::ZhCN => "  /clear               清理上下文",
        },
        "builtin.help_status" => match lang {
            Language::En => "  /status              Show agent status (ready / busy)",
            Language::ZhCN => "  /status              显示智能体状态（就绪 / 输出中）",
        },
        "builtin.select_agent_prompt" => match lang {
            Language::En => "Select default agent for this channel (↑↓ Enter, Esc cancel):",
            Language::ZhCN => "选择本频道默认智能体（↑↓ 回车确认，Esc 取消）：",
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
            Language::En => "/agents must be used from a channel context (TUI or chat).",
            Language::ZhCN => "/agents 需要在频道上下文中使用（TUI 或聊天）。",
        },
        "builtin.invalid_agent_index" => match lang {
            Language::En => "Invalid agent selection.",
            Language::ZhCN => "无效的智能体选项。",
        },
        "feishu.choose_agent" => match lang {
            Language::En => "Choose the default agent for this chat:",
            Language::ZhCN => "选择本聊天的默认智能体：",
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
        "builtin.help_any_text" => match lang {
            Language::En => "Any other text is sent directly to the active agent.",
            Language::ZhCN => "任何其他文本将直接发送给当前智能体。",
        },
        "builtin.session_stopped" => match lang {
            Language::En => "{NAME} session stopped.",
            Language::ZhCN => "{NAME} 会话已停止。",
        },
        "builtin.shutdown_notice" => match lang {
            Language::En => "cc-gateway is shutting down, session closed.",
            Language::ZhCN => "机器人正在关闭，会话已退出。",
        },
        "builtin.unknown_command" => match lang {
            Language::En => "Unknown command. Available commands: /help, /cd, /agent, /agents, /agent-history, /clear, /esc, /stop, /ll, /mkdir, /quit, /pwd, /show-thinking, /hide-thinking, /status",
            Language::ZhCN => "未知命令。可用命令: /help, /cd, /agent, /agents, /agent-history, /clear, /esc, /stop, /ll, /mkdir, /quit, /pwd, /show-thinking, /hide-thinking, /status",
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
        "builtin.esc_sent_opencode" => match lang {
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
        "builtin.esc_with_prompt_sent_opencode" => match lang {
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
        "builtin.stop_sent_opencode" => match lang {
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
            Language::En => "Note: Pi does not support session resume. A new session has been started.",
            Language::ZhCN => "注意：Pi 不支持会话恢复，已为你启动全新会话。",
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
        "builtin.changed_dir" => match lang {
            Language::En => "Changed directory to: {PATH}",
            Language::ZhCN => "目录已更改为: {PATH}",
        },
        "builtin.selection_cancelled" => match lang {
            Language::En => "Selection cancelled.",
            Language::ZhCN => "选择已取消。",
        },
        "builtin.cannot_delete_active" => match lang {
            Language::En => "Cannot delete an active session. Use /quit to stop it first.",
            Language::ZhCN => "无法删除活跃中的会话，请先使用 /quit 退出。",
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
        "builtin.select_dir_prompt" => match lang {
            Language::En => "Select a directory (↑↓ move, Enter to cd, q quit):",
            Language::ZhCN => "选择目录 (↑↓ 移动, Enter 确认, q 退出):",
        },
        "builtin.select_history_prompt" => match lang {
            Language::En => "Select session (↑↓ move, Enter resume, x delete, q cancel):",
            Language::ZhCN => "选择会话 (↑↓ 移动, Enter 恢复, x 删除, q 取消):",
        },
        "builtin.no_sessions" => match lang {
            Language::En => "No sessions found.",
            Language::ZhCN => "未找到会话。",
        },
        "builtin.session_history_title" => match lang {
            Language::En => "Claude Session History:",
            Language::ZhCN => "Claude 会话历史:",
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
            Language::En => "Claude resume id is missing for {SID}; starting a new session in its project directory.",
            Language::ZhCN => "会话 {SID} 缺少 Claude 恢复 ID，将在原项目目录启动新会话。",
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
        "feishu.unknown_command" => match lang {
            Language::En => "Unknown command. Available commands: /help, /cd, /agent, /agents, /agent-history, /clear, /esc, /stop, /ll, /mkdir, /quit, /pwd, /show-thinking, /hide-thinking, /status",
            Language::ZhCN => "未知命令。可用命令: /help, /cd, /agent, /agents, /agent-history, /clear, /esc, /stop, /ll, /mkdir, /quit, /pwd, /show-thinking, /hide-thinking, /status",
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
            Language::En => "Permission request: `{NAME}`\nReply with allow/deny and ID: `{ID}`",
            Language::ZhCN => "权限请求: `{NAME}`\n请回复允许/拒绝并带上 ID: `{ID}`",
        },
        "qq.unknown_command" => match lang {
            Language::En => "Unknown command. Available commands: /help, /cd, /agent, /agents, /agent-history, /clear, /esc, /stop, /ll, /mkdir, /quit, /pwd, /show-thinking, /hide-thinking, /status",
            Language::ZhCN => "未知命令。可用命令: /help, /cd, /agent, /agents, /agent-history, /clear, /esc, /stop, /ll, /mkdir, /quit, /pwd, /show-thinking, /hide-thinking, /status",
        },
        "qq.sent_file_caption" => match lang {
            Language::En => "File: {NAME}",
            Language::ZhCN => "文件：{NAME}",
        },
        "qq.send_file_group_unsupported" => match lang {
            Language::En => "This file type cannot be sent in QQ groups (images/videos/voice only). Use private chat (C2C) or send a PNG/JPG.",
            Language::ZhCN => "该文件类型无法发到 QQ 群（群聊仅支持图片/视频/语音富媒体）。请私聊发送，或改用 PNG/JPG 图片。",
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
        "cursor.session_not_found_new_session" => match lang {
            Language::En => "Cursor session not found ({ID}); a new session was started. If you rely on flags like --yolo/--print, consider creating a new session explicitly.",
            Language::ZhCN => "未找到 Cursor 会话（{ID}），已自动新建会话。如果你依赖 --yolo/--print 等启动参数，建议显式新建会话。",
        },
        "opencode.session_not_found_new_session" => match lang {
            Language::En => "OpenCode session not found ({ID}); a new session was started. Run `opencode auth login` if authentication fails.",
            Language::ZhCN => "未找到 OpenCode 会话（{ID}），已自动新建会话。若认证失败，请在终端运行 `opencode auth login`。",
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
            Language::En => "cc-gateway commands (Telegram):\n/help  Show help\n/pwd   Show current directory\n/ll    List directories\n/cd    Pick directory\n/mkdir Create directory\n/agent Start agent session\n/agents Set default agent\n/agent_history Show recent sessions\n/esc   Flush queued messages\n/stop  Stop current generation\n/clear Clear context\n/status Show status\n/show_thinking Show thinking\n/hide_thinking Hide thinking\n/quit  Stop active session\n\nTip: type /agent <provider> or /agents <provider> to pick a specific agent (e.g. claude, cursor, pi, opencode).\nAny other text will be forwarded to the active agent.",
            Language::ZhCN => "cc-gateway 命令（Telegram）：\n/help  显示帮助\n/pwd   显示当前目录\n/ll    列出目录\n/cd    选择目录\n/mkdir 创建目录\n/agent 启动智能体会话\n/agents 设置本聊天默认智能体\n/agent_history 显示最近会话\n/esc   强推排队消息\n/stop  停止当前输出\n/clear 清理上下文\n/status 显示状态\n/show_thinking 显示 Thinking\n/hide_thinking 隐藏 Thinking\n/quit  停止当前会话\n\n提示：可直接输入 /agent <智能体> 或 /agents <智能体> 指定智能体（如 claude、cursor、pi、opencode）。\n其他文本将直接发送给活动智能体。",
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

        // cli/tui.rs
        "tui.permission_required" => match lang {
            Language::En => "Permission Required: {NAME} (id: {ID})\n  /allow or /deny",
            Language::ZhCN => "需要权限: {NAME} (id: {ID})\n  输入 /allow 或 /deny",
        },
        "tui.confirm_request" => match lang {
            Language::En => "Confirm (id: {ID}): {PROMPT}\nOptions: {OPTIONS}\n  /allow {ID} or /deny {ID}",
            Language::ZhCN => "确认 (id: {ID}): {PROMPT}\n选项: {OPTIONS}\n  输入 /allow {ID} 或 /deny {ID}",
        },
        "tui.select_request" => match lang {
            Language::En => "Select (id: {ID}): {PROMPT}\nOptions: {OPTIONS}\n  /allow {ID} or /deny {ID}",
            Language::ZhCN => "选择 (id: {ID}): {PROMPT}\n选项: {OPTIONS}\n  输入 /allow {ID} 或 /deny {ID}",
        },
        "tui.questions_title" => match lang {
            Language::En => "Questions (id: {ID}):\n  /allow {ID} or /deny {ID}",
            Language::ZhCN => "问题 (id: {ID}):\n  输入 /allow {ID} 或 /deny {ID}",
        },
        "tui.question_item" => match lang {
            Language::En => "  {HEADER}: {QUESTION}\n",
            Language::ZhCN => "  {HEADER}: {QUESTION}\n",
        },
        "tui.question_option" => match lang {
            Language::En => "    - {LABEL}: {DESCRIPTION}\n",
            Language::ZhCN => "    - {LABEL}: {DESCRIPTION}\n",
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
