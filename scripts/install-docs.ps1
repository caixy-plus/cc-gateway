# Shared documentation links printed after install / install_local.
# Dot-sourced by install.ps1, install_local.ps1

function Print-InstallDocs {
    param([string]$LocalRepoRoot = "")

    $repo = "caixy-plus/cc-gateway"
    $gh = "https://github.com/$repo/blob/main/docs"

    if ($lang -eq 'zh') {
        $overview = "$gh/bots/README.zh-CN.md"
        $feishu = "$gh/bots/feishu.zh-CN.md"
        $telegram = "$gh/bots/telegram.zh-CN.md"
        $config = "$gh/config.zh-CN.md"
        $usage = "$gh/usage.zh-CN.md"
        $checklist = "$gh/platform-integration-checklist.zh-CN.md"
    } else {
        $overview = "$gh/bots/README.md"
        $feishu = "$gh/bots/feishu.md"
        $telegram = "$gh/bots/telegram.md"
        $config = "$gh/config.md"
        $usage = "$gh/usage.md"
        $checklist = "$gh/platform-integration-checklist.md"
    }

    Write-Msg "" ""
    Write-Msg "Agent providers (install separately on your machine):" "智能体 CLI（需在本机单独安装）："
    Write-Msg "  Claude Code, Cursor (agent), OpenCode, Kimi, Gemini, Pi — see config guide." "  Claude Code、Cursor (agent)、OpenCode、Kimi、Gemini、Pi — 见配置说明。"
    Write-Msg "  Codex: npm i -g @zed-industries/codex-acp" "  Codex：npm i -g @zed-industries/codex-acp"
    Write-Msg "    (Zed ACP adapter for the Codex CLI — not the raw 'codex' binary; auth: codex login or OPENAI_API_KEY)" "    （Codex CLI 的 Zed ACP 适配器，不是裸 codex 可执行文件；登录：codex login 或 OPENAI_API_KEY）"

    Write-Msg "" ""
    Write-Msg "Documentation:" "文档与配置指南："
    Write-Msg "  Overview (pairing, multi-platform): $overview" "  总览（配对、多平台）: $overview"
    Write-Msg "  Feishu / Lark bot setup:            $feishu" "  飞书 / Lark 机器人:                  $feishu"
    Write-Msg "  Telegram bot setup:                $telegram" "  Telegram 机器人:                     $telegram"
    Write-Msg "  Configuration reference:           $config" "  配置字段说明:                        $config"
    Write-Msg "  Usage guide (daemon & WebUI):      $usage" "  使用指南（守护进程 / WebUI）:        $usage"
    Write-Msg "  Platform integration checklist:    $checklist" "  平台接入检查清单:                    $checklist"

    if ($LocalRepoRoot -and (Test-Path "$LocalRepoRoot\docs\bots")) {
        Write-Msg "" ""
        if ($lang -eq 'zh') {
            Write-Msg "Local docs in this repository:" "本仓库本地文档："
            Write-Msg "  $LocalRepoRoot\docs\bots\README.zh-CN.md" "  $LocalRepoRoot\docs\bots\README.zh-CN.md"
            Write-Msg "  $LocalRepoRoot\docs\bots\feishu.zh-CN.md" "  $LocalRepoRoot\docs\bots\feishu.zh-CN.md"
            Write-Msg "  $LocalRepoRoot\docs\bots\telegram.zh-CN.md" "  $LocalRepoRoot\docs\bots\telegram.zh-CN.md"
            Write-Msg "  $LocalRepoRoot\docs\config.zh-CN.md" "  $LocalRepoRoot\docs\config.zh-CN.md"
            Write-Msg "  $LocalRepoRoot\docs\usage.zh-CN.md" "  $LocalRepoRoot\docs\usage.zh-CN.md"
            Write-Msg "  $LocalRepoRoot\docs\platform-integration-checklist.zh-CN.md" "  $LocalRepoRoot\docs\platform-integration-checklist.zh-CN.md"
        } else {
            Write-Msg "Local docs in this repository:" "Local docs in this repository:"
            Write-Msg "  $LocalRepoRoot\docs\bots\README.md" "  $LocalRepoRoot\docs\bots\README.md"
            Write-Msg "  $LocalRepoRoot\docs\bots\feishu.md" "  $LocalRepoRoot\docs\bots\feishu.md"
            Write-Msg "  $LocalRepoRoot\docs\bots\telegram.md" "  $LocalRepoRoot\docs\bots\telegram.md"
            Write-Msg "  $LocalRepoRoot\docs\config.md" "  $LocalRepoRoot\docs\config.md"
            Write-Msg "  $LocalRepoRoot\docs\usage.md" "  $LocalRepoRoot\docs\usage.md"
            Write-Msg "  $LocalRepoRoot\docs\platform-integration-checklist.md" "  $LocalRepoRoot\docs\platform-integration-checklist.md"
        }
    }
}
