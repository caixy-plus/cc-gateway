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

        // config/wizard.rs
        "wizard.title" => match lang {
            Language::En => "=== cc-gateway Configuration ===",
            Language::ZhCN => "=== cc-gateway 配置 ===",
        },
        "wizard.log_section" => match lang {
            Language::En => "log        - Logging settings",
            Language::ZhCN => "log        - 日志设置",
        },
        "wizard.claude_section" => match lang {
            Language::En => "claude     - Claude Code settings",
            Language::ZhCN => "claude     - Claude Code 设置",
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
        "wizard.claude_config" => match lang {
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
            Language::En => "      You can configure it later by running 'cc-gateway init' again.",
            Language::ZhCN => "      您可以稍后通过再次运行 'cc-gateway init' 来配置。",
        },
        "wizard.please_edit_credentials" => match lang {
            Language::En => "Please edit it to add your Feishu app credentials.",
            Language::ZhCN => "请编辑以添加您的飞书应用凭证。",
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
        "claude.thinking_placeholder" => match lang {
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
            Language::En => "Claude session stopped. Back to cc-gateway.",
            Language::ZhCN => "Claude 会话已停止。返回 cc-gateway。",
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
            Language::En => "cc-gateway commands:\n  /help                Show this help\n  /quit                Quit current agent session\n  /cd <path>           Change working directory\n  /cd_default          Change to default directory\n  /agent [args...]     Start a new agent session\n  /claude [args...]    Alias for /agent\n  /pwd                 Show current working directory\n  /ll                  List files in current directory\n  /mkdir <dirname>     Create a new directory\n  /show-thinking       Always show Thinking output when available\n  /hide-thinking       Hide Thinking output\n  /agent-history       Show recent agent sessions\nAny other text is sent directly to the active agent.",
            Language::ZhCN => "cc-gateway 命令:\n  /help                显示此帮助\n  /quit                退出当前智能体会话\n  /cd <path>           更改工作目录\n  /cd_default          将工作目录更改为默认目录\n  /agent [args...]     启动新的智能体会话\n  /claude [args...]    /agent 的别名\n  /pwd                 显示当前工作目录\n  /ll                  列出当前目录中的文件\n  /mkdir <目录名>       创建新目录\n  /show-thinking       始终显示可用的 Thinking 输出\n  /hide-thinking       隐藏 Thinking 输出\n  /agent-history       显示最近的智能体会话\n任何其他文本将直接发送给活动智能体。",
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
        "builtin.help_claude_history" => match lang {
            Language::En => "  /agent-history       Show recent agent sessions and resume by index",
            Language::ZhCN => "  /agent-history       显示最近的智能体会话并按索引恢复",
        },
        "builtin.help_any_text" => match lang {
            Language::En => "Any other text is sent directly to Claude Code.",
            Language::ZhCN => "任何其他文本将直接发送给 Claude Code。",
        },
        "builtin.session_stopped" => match lang {
            Language::En => "Claude session stopped.",
            Language::ZhCN => "Claude 会话已停止。",
        },
        "builtin.shutdown_notice" => match lang {
            Language::En => "cc-gateway is shutting down, session closed.",
            Language::ZhCN => "机器人正在关闭，会话已退出。",
        },
        "builtin.failed_stop_session" => match lang {
            Language::En => "Failed to stop session: {ERR}",
            Language::ZhCN => "停止会话失败: {ERR}",
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
            Language::En => "Claude session started in: {DIR}\n\n\x1b[2m\u{1F4A1} Type anything and press Enter to chat with Claude.\x1b[0m",
            Language::ZhCN => "Claude 会话已启动于: {DIR}\n\n\x1b[2m\u{1F4A1} 输入任意内容并按 Enter 与 Claude 对话。\x1b[0m",
        },
        "builtin.session_resumed" => match lang {
            Language::En => "Claude session resumed in: {DIR}\n\n\x1b[2m\u{1F4A1} Type anything and press Enter to chat with Claude.\x1b[0m",
            Language::ZhCN => "Claude 会话已恢复于: {DIR}\n\n\x1b[2m\u{1F4A1} 输入任意内容并按 Enter 与 Claude 对话。\x1b[0m",
        },
        "builtin.failed_start_claude" => match lang {
            Language::En => "Failed to start Claude: {ERR}",
            Language::ZhCN => "启动 Claude 失败: {ERR}",
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
        "builtin.recent_claude_sessions" => match lang {
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
            Language::En => "Unknown command. Available commands: /help, /cd, /agent, /agent-history, /claude, /claude-history, /ll, /mkdir, /quit, /pwd, /show-thinking, /hide-thinking",
            Language::ZhCN => "未知命令。可用命令: /help, /cd, /agent, /agent-history, /claude, /claude-history, /ll, /mkdir, /quit, /pwd, /show-thinking, /hide-thinking",
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
        "feishu.session_resumed" => match lang {
            Language::En => "Claude session resumed in: {DIR}\n\n💡 Type anything and press Enter to chat with Claude.",
            Language::ZhCN => "Claude 会话已恢复于: {DIR}\n\n💡 输入任意内容并按 Enter 与 Claude 对话。",
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
        "feishu.session_started" => match lang {
            Language::En => "Claude session started in: {DIR}\n\n💡 Type anything and press Enter to chat with Claude.",
            Language::ZhCN => "Claude 会话已启动于: {DIR}\n\n💡 输入任意内容并按 Enter 与 Claude 对话。",
        },

        // platform/telegram
        "telegram.private_chat_only" => match lang {
            Language::En => "Please use in private chat.",
            Language::ZhCN => "请在私聊中使用。",
        },
        "telegram.history_unavailable" => match lang {
            Language::En => "Claude history is not available in Telegram.",
            Language::ZhCN => "Telegram 中不可用 Claude 历史记录。",
        },
        "telegram.shutdown_notice" => match lang {
            Language::En => "Bot is shutting down, sessions exited.",
            Language::ZhCN => "机器人正在关闭，会话已退出。",
        },
        "telegram.session_started" => match lang {
            Language::En => "Claude session started in: {DIR}\n\n💡 Type anything and press Enter to chat with Claude.",
            Language::ZhCN => "Claude 会话已启动于: {DIR}\n\n💡 输入任意内容并按 Enter 与 Claude 对话。",
        },
        "telegram.session_resumed" => match lang {
            Language::En => "Claude session resumed in: {DIR}\n\n💡 Type anything and press Enter to chat with Claude.",
            Language::ZhCN => "Claude 会话已恢复于: {DIR}\n\n💡 输入任意内容并按 Enter 与 Claude 对话。",
        },
        "telegram.permission_request" => match lang {
            Language::En => "Permission request: `{NAME}`\nID: `{ID}`",
            Language::ZhCN => "权限请求: `{NAME}`\nID: `{ID}`",
        },
        "telegram.command_help" => match lang {
            Language::En => "Show help",
            Language::ZhCN => "查看帮助",
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
            Language::En => "Change directory",
            Language::ZhCN => "切换目录",
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
        "telegram.command_claude" => match lang {
            Language::En => "Start Claude session",
            Language::ZhCN => "启动 Claude 会话",
        },
        "telegram.command_claude_history" => match lang {
            Language::En => "Show Claude history",
            Language::ZhCN => "查看 Claude 历史",
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
            Language::En => "Confirm (id: {ID}): {PROMPT}\nOptions: {OPTIONS}",
            Language::ZhCN => "确认 (id: {ID}): {PROMPT}\n选项: {OPTIONS}",
        },
        "tui.select_request" => match lang {
            Language::En => "Select (id: {ID}): {PROMPT}\nOptions: {OPTIONS}",
            Language::ZhCN => "选择 (id: {ID}): {PROMPT}\n选项: {OPTIONS}",
        },
        "tui.questions_title" => match lang {
            Language::En => "Questions (id: {ID}):\n",
            Language::ZhCN => "问题 (id: {ID}):\n",
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
            Language::En => "Permission request: `{NAME}`\nID: `{ID}`",
            Language::ZhCN => "权限请求: `{NAME}`\nID: `{ID}`",
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
        let mut _args: ::std::vec::Vec<(&str, ::std::string::String)> = ::std::vec::Vec::new();
        $(
            _args.push((stringify!($name), ::std::string::ToString::to_string(&$value)));
        )*
        let _refs: ::std::vec::Vec<(&str, &str)> = _args.iter().map(|(k, v)| (*k, v.as_str())).collect();
        $crate::i18n::dict::tfmt($key, &_refs)
    }};
}
