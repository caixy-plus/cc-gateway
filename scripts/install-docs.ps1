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
        $qq = "$gh/bots/qq.zh-CN.md"
        $config = "$gh/config.zh-CN.md"
        $usage = "$gh/usage.zh-CN.md"
        $checklist = "$gh/platform-integration-checklist.zh-CN.md"
    } else {
        $overview = "$gh/bots/README.md"
        $feishu = "$gh/bots/feishu.md"
        $telegram = "$gh/bots/telegram.md"
        $qq = "$gh/bots/qq.md"
        $config = "$gh/config.md"
        $usage = "$gh/usage.md"
        $checklist = "$gh/platform-integration-checklist.md"
    }

    Write-Msg "" ""
    Write-Msg "Documentation:" "文档与配置指南："
    Write-Msg "  Overview (pairing, multi-platform): $overview" "  总览（配对、多平台）: $overview"
    Write-Msg "  Feishu / Lark bot setup:            $feishu" "  飞书 / Lark 机器人:                  $feishu"
    Write-Msg "  Telegram bot setup:                $telegram" "  Telegram 机器人:                     $telegram"
    Write-Msg "  QQ official bot setup:             $qq" "  QQ 官方机器人:                       $qq"
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
            Write-Msg "  $LocalRepoRoot\docs\bots\qq.zh-CN.md" "  $LocalRepoRoot\docs\bots\qq.zh-CN.md"
            Write-Msg "  $LocalRepoRoot\docs\config.zh-CN.md" "  $LocalRepoRoot\docs\config.zh-CN.md"
            Write-Msg "  $LocalRepoRoot\docs\usage.zh-CN.md" "  $LocalRepoRoot\docs\usage.zh-CN.md"
            Write-Msg "  $LocalRepoRoot\docs\platform-integration-checklist.zh-CN.md" "  $LocalRepoRoot\docs\platform-integration-checklist.zh-CN.md"
        } else {
            Write-Msg "Local docs in this repository:" "Local docs in this repository:"
            Write-Msg "  $LocalRepoRoot\docs\bots\README.md" "  $LocalRepoRoot\docs\bots\README.md"
            Write-Msg "  $LocalRepoRoot\docs\bots\feishu.md" "  $LocalRepoRoot\docs\bots\feishu.md"
            Write-Msg "  $LocalRepoRoot\docs\bots\telegram.md" "  $LocalRepoRoot\docs\bots\telegram.md"
            Write-Msg "  $LocalRepoRoot\docs\bots\qq.md" "  $LocalRepoRoot\docs\bots\qq.md"
            Write-Msg "  $LocalRepoRoot\docs\config.md" "  $LocalRepoRoot\docs\config.md"
            Write-Msg "  $LocalRepoRoot\docs\usage.md" "  $LocalRepoRoot\docs\usage.md"
            Write-Msg "  $LocalRepoRoot\docs\platform-integration-checklist.md" "  $LocalRepoRoot\docs\platform-integration-checklist.md"
        }
    }
}
