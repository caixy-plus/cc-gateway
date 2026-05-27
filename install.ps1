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

# Config
$ConfigDir = "$env:USERPROFILE\.cc-gateway"
New-Item -ItemType Directory -Path "$ConfigDir\logs" -Force | Out-Null

if (-not (Test-Path "$ConfigDir\config.json")) {
    $Config = @"
{
  "log": {
    "level": "info",
    "file": "~/.cc-gateway/logs/gateway.log"
  },
  "claude": {
    "cli_path": "claude",
    "default_args": "--dangerously-skip-permissions"
  },
  "feishu": {
    "enabled": true,
    "app_id": "${FEISHU_APP_ID}",
    "app_secret": "${FEISHU_APP_SECRET}",
    "allow_from": "*",
    "encrypt_key": "",
    "mode": "websocket",
    "webhook_bind": "0.0.0.0:3000"
  },
  "default_dir": "~",
  "port": 17534,
  "telegram": {
    "enabled": false,
    "bot_token": "${TELEGRAM_BOT_TOKEN}",
    "allow_from": "*",
    "webhook_url": ""
  }
}
"@
    Set-Content -Path "$ConfigDir\config.json" -Value $Config
    Write-Msg "Created default config at $ConfigDir\config.json" "已创建默认配置: $ConfigDir\config.json"
}

# Check default port conflict
function Test-PortInUse($port) {
    try {
        $client = New-Object System.Net.Sockets.TcpClient
        $client.Connect("127.0.0.1", $port)
        $client.Close()
        return $true
    } catch {
        return $false
    }
}

$defaultPort = 17534
$configFile = "$ConfigDir\config.json"
$pidFile = "$ConfigDir\daemon.pid"

if (Test-PortInUse $defaultPort) {
    $cgPid = $null
    if (Test-Path $pidFile) {
        $cgPid = (Get-Content $pidFile).Trim()
    }
    $isCgRunning = $false
    if ($cgPid) {
        try {
            Get-Process -Id $cgPid -ErrorAction Stop | Out-Null
            $isCgRunning = $true
        } catch {}
    }
    if ($isCgRunning) {
        Write-Msg "Default port $defaultPort is used by cc-gateway (PID: $cgPid), continuing..." "默认端口 $defaultPort 已被 cc-gateway 占用 (PID: $cgPid)，继续..."
    } else {
        Write-Msg "Default port $defaultPort is occupied by another process" "默认端口 $defaultPort 被其他进程占用"
        $newPort = $defaultPort
        while (Test-PortInUse $newPort) {
            $newPort++
        }
        Write-Msg "Auto-assigned new port: $newPort" "自动分配新端口: $newPort"
        if (Test-Path $configFile) {
            $config = Get-Content $configFile | ConvertFrom-Json
            $config | Add-Member -NotePropertyName "port" -NotePropertyValue $newPort -Force
            $config | ConvertTo-Json -Depth 10 | Set-Content $configFile
            Write-Msg "Updated config: $configFile (port = $newPort)" "已更新配置文件: $configFile (port = $newPort)"
        }
    }
}

# Add to PATH
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
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
Write-Msg "cc-gateway installed successfully to $InstallDir\$Binary.exe" "cc-gateway 已成功安装到 $InstallDir\$Binary.exe"
Write-Msg "Run '$Binary --help' to get started" "运行 '$Binary --help' 开始使用"
Write-Msg "" ""
Write-Msg "For Feishu bot setup instructions, see:" "飞书机器人配置说明请参阅:"
Write-Msg "  https://github.com/caixy-plus/cc-gateway/blob/main/docs/config.md#feishu-setup" "  https://github.com/caixy-plus/cc-gateway/blob/main/docs/config.md#feishu-setup"
