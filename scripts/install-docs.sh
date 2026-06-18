# Shared documentation links printed after install / install_local.
# Sourced by install.sh, install_local.sh (and embedded fallback in install.sh for curl|sh).
#
# Expects optional: LANG_CODE, msg() from the parent install script.
# Usage: print_install_docs [local_repo_root]

print_install_docs() {
    local root="${1:-}"
    local repo="caixy-plus/cc-gateway"
    local gh="https://github.com/${repo}/blob/main/docs"

    _doc_msg() {
        if type msg >/dev/null 2>&1; then
            msg "$1" "$2"
        elif [ "${LANG_CODE:-en}" = "zh" ]; then
            echo "$2"
        else
            echo "$1"
        fi
    }

    if [ "${LANG_CODE:-en}" = "zh" ]; then
        local overview="${gh}/bots/README.zh-CN.md"
        local feishu="${gh}/bots/feishu.zh-CN.md"
        local telegram="${gh}/bots/telegram.zh-CN.md"
        local config="${gh}/config.zh-CN.md"
        local usage="${gh}/usage.zh-CN.md"
        local checklist="${gh}/platform-integration-checklist.zh-CN.md"
    else
        local overview="${gh}/bots/README.md"
        local feishu="${gh}/bots/feishu.md"
        local telegram="${gh}/bots/telegram.md"
        local config="${gh}/config.md"
        local usage="${gh}/usage.md"
        local checklist="${gh}/platform-integration-checklist.md"
    fi

    _doc_msg "" ""
    _doc_msg "Agent providers (install separately on your machine):" "智能体 CLI（需在本机单独安装）："
    _doc_msg "  Claude Code, Cursor (agent), OpenCode, Kimi, Gemini, Pi — see config guide." "  Claude Code、Cursor (agent)、OpenCode、Kimi、Gemini、Pi — 见配置说明。"
    _doc_msg "  Codex: npm i -g @zed-industries/codex-acp" "  Codex：npm i -g @zed-industries/codex-acp"
    _doc_msg "    (Zed ACP adapter for the Codex CLI — not the raw 'codex' binary; auth: codex login or OPENAI_API_KEY)" "    （Codex CLI 的 Zed ACP 适配器，不是裸 codex 可执行文件；登录：codex login 或 OPENAI_API_KEY）"

    _doc_msg "" ""
    _doc_msg "Documentation:" "文档与配置指南："
    _doc_msg "  Overview (pairing, multi-platform): ${overview}" "  总览（配对、多平台）: ${overview}"
    _doc_msg "  Feishu / Lark bot setup:            ${feishu}" "  飞书 / Lark 机器人:                  ${feishu}"
    _doc_msg "  Telegram bot setup:                ${telegram}" "  Telegram 机器人:                     ${telegram}"
    _doc_msg "  Configuration reference:           ${config}" "  配置字段说明:                        ${config}"
    _doc_msg "  Usage guide (daemon & WebUI):      ${usage}" "  使用指南（守护进程 / WebUI）:        ${usage}"
    _doc_msg "  Platform integration checklist:    ${checklist}" "  平台接入检查清单:                    ${checklist}"

    if [ -n "$root" ] && [ -d "$root/docs/bots" ]; then
        _doc_msg "" ""
        if [ "${LANG_CODE:-en}" = "zh" ]; then
            _doc_msg "Local docs in this repository:" "本仓库本地文档："
            _doc_msg "  ${root}/docs/bots/README.zh-CN.md" "  ${root}/docs/bots/README.zh-CN.md"
            _doc_msg "  ${root}/docs/bots/feishu.zh-CN.md" "  ${root}/docs/bots/feishu.zh-CN.md"
            _doc_msg "  ${root}/docs/bots/telegram.zh-CN.md" "  ${root}/docs/bots/telegram.zh-CN.md"
            _doc_msg "  ${root}/docs/config.zh-CN.md" "  ${root}/docs/config.zh-CN.md"
            _doc_msg "  ${root}/docs/usage.zh-CN.md" "  ${root}/docs/usage.zh-CN.md"
            _doc_msg "  ${root}/docs/platform-integration-checklist.zh-CN.md" "  ${root}/docs/platform-integration-checklist.zh-CN.md"
        else
            _doc_msg "Local docs in this repository:" "Local docs in this repository:"
            _doc_msg "  ${root}/docs/bots/README.md" "  ${root}/docs/bots/README.md"
            _doc_msg "  ${root}/docs/bots/feishu.md" "  ${root}/docs/bots/feishu.md"
            _doc_msg "  ${root}/docs/bots/telegram.md" "  ${root}/docs/bots/telegram.md"
            _doc_msg "  ${root}/docs/config.md" "  ${root}/docs/config.md"
            _doc_msg "  ${root}/docs/usage.md" "  ${root}/docs/usage.md"
            _doc_msg "  ${root}/docs/platform-integration-checklist.md" "  ${root}/docs/platform-integration-checklist.md"
        fi
    fi
}
