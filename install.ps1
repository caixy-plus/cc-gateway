#Requires -Version 5.1
$ErrorActionPreference = "Stop"

$Repo = "caixy-plus/cc-gateway"
$Binary = "cc-gateway"
$InstallDir = "$env:LOCALAPPDATA\cc-gateway"

# Language detection
function Get-LangCode {
    if ($env:CC_GATEWAY_LANG -match '^zh') { return 'zh' }
    if ($env:LANG -match '^zh') { return 'zh' }
    try {
        $culture = [System.Globalization.CultureInfo]::CurrentUICulture
        if ($culture.Name -match '^zh') { return 'zh' }
    } catch {}
    return 'en'
}

$lang = Get-LangCode

# PowerShell 5.1 executes .ps1 without a UTF-8 BOM using the system ANSI code page
# (e.g. GBK on Chinese Windows), which breaks Chinese strings after Invoke-WebRequest
# saves UTF-8 from GitHub. Re-write downloaded scripts with a BOM before dot-sourcing.
function Set-ScriptUtf8Bom([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return }
    $utf8NoBom = New-Object System.Text.UTF8Encoding $false
    $utf8Bom = New-Object System.Text.UTF8Encoding $true
    $text = [System.IO.File]::ReadAllText($Path, $utf8NoBom)
    [System.IO.File]::WriteAllText($Path, $text, $utf8Bom)
}

function Write-Msg($en, $zh) {
    if ($lang -eq 'zh') {
        Write-Host $zh
    } else {
        Write-Host $en
    }
}

function Stop-AllCCGateway {
    # Step 1: Force kill all cc-gateway processes
    $procs = Get-Process -Name $Binary -ErrorAction SilentlyContinue
    if ($procs) {
        Write-Msg "Force stopping cc-gateway processes..." "正在强制停止 cc-gateway 进程..."
        $procs | Stop-Process -Force -ErrorAction SilentlyContinue
    }

    # Step 2: Verify all processes are actually closed
    $maxRetries = 10
    for ($i = 1; $i -le $maxRetries; $i++) {
        Start-Sleep -Seconds 1
        $procs = Get-Process -Name $Binary -ErrorAction SilentlyContinue
        if (-not $procs) {
            Write-Msg "All cc-gateway processes have stopped." "所有 cc-gateway 进程已停止。"
            return
        }
        Write-Msg "Waiting for $($procs.Count) process(es) to exit... ($i/$maxRetries)" "等待 $($procs.Count) 个进程退出... ($i/$maxRetries)"
        $procs | Stop-Process -Force -ErrorAction SilentlyContinue
    }

    # Step 3: Final check - fail if still running
    $procs = Get-Process -Name $Binary -ErrorAction SilentlyContinue
    if ($procs) {
        Write-Msg "Failed to stop all cc-gateway processes after $maxRetries attempts. PIDs: $($procs.Id -join ', '). Aborting." "无法停止所有 cc-gateway 进程（尝试 $maxRetries 次后）。PID: $($procs.Id -join ', ')。中止安装。"
        exit 1
    }
}

# Detect architecture
$Arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64"   { "x86_64" }
    "ARM64"   { "aarch64" }
    default   { throw (Write-Msg "Unsupported architecture: $($env:PROCESSOR_ARCHITECTURE)" "不支持的架构: $($env:PROCESSOR_ARCHITECTURE)") }
}

$Target = "$Arch-pc-windows-msvc"
Write-Msg "Installing cc-gateway for $Target..." "正在安装 cc-gateway ($Target)..."

# Download
$LatestUrl = "https://github.com/$Repo/releases/latest/download/$Binary-$Target.zip"
$TempFile = "$env:TEMP\$Binary.zip"
$TempDir = "$env:TEMP\$Binary-install"

Write-Msg "Downloading from $LatestUrl..." "正在下载: $LatestUrl..."
try {
    Invoke-WebRequest -Uri $LatestUrl -OutFile $TempFile -UseBasicParsing
} catch {
    Write-Msg "Failed to download: $_" "下载失败: $_"
    exit 1
}

# Extract
if (Test-Path $TempDir) { Remove-Item -Recurse -Force $TempDir }
New-Item -ItemType Directory -Path $TempDir | Out-Null
Expand-Archive -Path $TempFile -DestinationPath $TempDir -Force

# Install
New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

Write-Msg "Stopping all cc-gateway processes before replacing binary..." "正在停止所有 cc-gateway 进程以替换可执行文件..."
Stop-AllCCGateway

Copy-Item -Path "$TempDir\$Binary.exe" -Destination "$InstallDir\$Binary.exe" -Force

if ($env:CC_GATEWAY_SKIP_SETUP) {
    Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
    Remove-Item -Force $TempFile -ErrorAction SilentlyContinue
    Write-Msg "" ""
    Write-Msg "cc-gateway installed successfully to $InstallDir\$Binary.exe" "cc-gateway 已成功安装到 $InstallDir\$Binary.exe"
    return
}

# Config directory (actual config initialization is handled by `cc-gateway init`)
$ConfigDir = "$env:USERPROFILE\.cc-gateway"
New-Item -ItemType Directory -Path "$ConfigDir\logs" -Force | Out-Null

# Add to PATH
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    $sep = if ($UserPath) { ";" } else { "" }
    [Environment]::SetEnvironmentVariable("Path", "$UserPath$sep$InstallDir", "User")
    Write-Msg "Added $InstallDir to PATH" "已将 $InstallDir 添加到 PATH"
}

# Cleanup
Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
Remove-Item -Force $TempFile -ErrorAction SilentlyContinue

# Run init
Write-Msg "" ""
Write-Msg "Running initial setup..." "正在运行初始设置..."
& "$InstallDir\$Binary.exe" init

Write-Msg "" ""
Write-Msg "Restarting cc-gateway daemon..." "正在重启 cc-gateway 守护进程..."
try {
    # Run restart in a detached process and never block the installer.
    # Some Windows environments may hang when starting background processes
    # with shared console. We add a timeout guard to keep install responsive.
    $p = Start-Process -FilePath "$InstallDir\$Binary.exe" -ArgumentList @("restart") -PassThru -WindowStyle Hidden
    try {
        Wait-Process -Id $p.Id -Timeout 15 -ErrorAction SilentlyContinue | Out-Null
    } catch {}
} catch {
    Write-Msg "Failed to restart daemon: $_" "重启守护进程失败: $_"
}

Write-Msg "" ""
Write-Msg "cc-gateway installed successfully to $InstallDir\$Binary.exe" "cc-gateway 已成功安装到 $InstallDir\$Binary.exe"
Write-Msg "Run '$Binary --help' to get started" "运行 '$Binary --help' 开始使用"
Write-Msg "Open WebUI: '$Binary webui' (starts daemon if needed)" "打开 WebUI：'$Binary webui'（如未启动会自动 start）"
$stuckHintZh = "若守护进程启动疑似卡住，请运行：{0} status 和 {1} log -n 200" -f $Binary, $Binary
Write-Msg "If daemon start looks stuck, run: '$Binary status' and '$Binary log -n 200'" $stuckHintZh
$docsScript = $null
$docsTmp = $null
if ($PSScriptRoot) {
    $candidate = Join-Path $PSScriptRoot "scripts\install-docs.ps1"
    if (Test-Path $candidate) { $docsScript = $candidate }
}
if (-not $docsScript) {
    $docsTmp = Join-Path $env:TEMP "cc-gateway-install-docs.ps1"
    try {
        Invoke-WebRequest -Uri "https://raw.githubusercontent.com/$Repo/main/scripts/install-docs.ps1" -OutFile $docsTmp -UseBasicParsing
        Set-ScriptUtf8Bom $docsTmp
        $docsScript = $docsTmp
    } catch {
        $docsScript = $null
        if ($docsTmp -and (Test-Path $docsTmp)) {
            Remove-Item -Force $docsTmp -ErrorAction SilentlyContinue
        }
        $docsTmp = $null
    }
}
if ($docsScript -and (Test-Path $docsScript)) {
    . $docsScript
    Print-InstallDocs
    if ($docsTmp -and (Test-Path $docsTmp)) {
        Remove-Item -Force $docsTmp -ErrorAction SilentlyContinue
    }
} else {
    Write-Msg "Documentation: https://github.com/$Repo/tree/main/docs/bots" "文档: https://github.com/$Repo/tree/main/docs/bots"
}
