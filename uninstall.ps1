# cc-gateway uninstaller (Windows)
#
# Removes the binary, install directory, the User PATH entry, and (unless
# -KeepData / $env:CCG_KEEP_DATA=1) the data directory.
#
# Usage (download first to avoid antivirus false positives):
#   Invoke-WebRequest https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/uninstall.ps1 -OutFile cc-gateway-uninstall.ps1
#   .\cc-gateway-uninstall.ps1
#   $env:CCG_KEEP_DATA='1'; .\cc-gateway-uninstall.ps1
param([switch]$KeepData)

$ErrorActionPreference = 'Continue'
if ($env:CCG_KEEP_DATA -eq '1') { $KeepData = $true }

function Write-Msg($en, $zh) {
    if ($env:LANG -match 'zh' -or (Get-Culture).Name -match '^zh') {
        Write-Host $zh
    } else {
        Write-Host $en
    }
}

$InstallDir = "$env:LOCALAPPDATA\cc-gateway"
$ConfigDir  = "$env:USERPROFILE\.cc-gateway"

Write-Msg "Uninstalling cc-gateway..." "正在卸载 cc-gateway..."

# 1. Stop all cc-gateway processes (daemon + any others).
$procs = Get-Process -Name cc-gateway -ErrorAction SilentlyContinue
if ($procs) {
    $procs | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 800
}
Write-Msg "  - stopped processes" "  - 已停止进程"

# 2. Remove the binary and install directory.
if (Test-Path $InstallDir) {
    Remove-Item -Recurse -Force $InstallDir -ErrorAction SilentlyContinue
    Write-Msg "  - removed install dir: $InstallDir" "  - 已删除安装目录：$InstallDir"
}

# 3. Remove the install dir from the User PATH (leave everything else intact).
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath) {
    $installDirNorm = $InstallDir.TrimEnd('\')
    $parts = $userPath.Split(';') | Where-Object { $_ -and ($_.TrimEnd('\') -ine $installDirNorm) }
    [Environment]::SetEnvironmentVariable("Path", ($parts -join ';'), "User")
    Write-Msg "  - cleaned PATH entry" "  - 已清理 PATH 配置"
}

# 4. Data directory.
if ($KeepData) {
    Write-Msg "  - kept data: $ConfigDir" "  - 已保留数据：$ConfigDir"
} else {
    if (Test-Path $ConfigDir) {
        Remove-Item -Recurse -Force $ConfigDir -ErrorAction SilentlyContinue
    }
    Write-Msg "  - removed data: $ConfigDir" "  - 已删除数据：$ConfigDir"
}

# 5. Verify cleanup — check if cc-gateway is still on PATH.
$remaining = Get-Command cc-gateway -ErrorAction SilentlyContinue
if ($remaining) {
    Write-Host ""
    Write-Msg "WARNING: cc-gateway is still found at: $($remaining.Source)" "警告：cc-gateway 仍在以下位置存在：$($remaining.Source)"
    Write-Msg "It may have been installed to a non-standard location." "它可能被安装到了非标准位置。"
}

Write-Msg "cc-gateway has been uninstalled." "cc-gateway 已卸载完成。"
Write-Msg "Open a new terminal for PATH changes to take effect." "请重新打开终端以使 PATH 变更生效。"
