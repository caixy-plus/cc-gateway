#Requires -Version 5.1
$ErrorActionPreference = "Stop"

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

Write-Msg "1. Building release version..." "1. 构建 release 版本..."
cargo build --release

Write-Msg "2. Stopping running daemon (if any)..." "2. 停止运行中的 daemon（如有）..."
& "$InstallDir\$Binary.exe" stop 2>$null | Out-Null

Write-Msg "3. Installing to $InstallDir..." "3. 安装到 $InstallDir..."
New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
Copy-Item -Path "target\release\$Binary.exe" -Destination "$InstallDir\$Binary.exe" -Force

# Add to PATH
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    $sep = if ($UserPath) { ";" } else { "" }
    [Environment]::SetEnvironmentVariable("Path", "$UserPath$sep$InstallDir", "User")
    Write-Msg "Added $InstallDir to PATH" "已将 $InstallDir 添加到 PATH"
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
$configFile = "$env:USERPROFILE\.cc-gateway\config.json"
$pidFile = "$env:USERPROFILE\.cc-gateway\daemon.pid"

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

Write-Msg "5. Starting cc-gateway..." "5. 启动 cc-gateway..."
& "$InstallDir\$Binary.exe" start

Write-Msg "" ""
Write-Msg "cc-gateway installed successfully to $InstallDir\$Binary.exe" "cc-gateway 已成功安装到 $InstallDir\$Binary.exe"
Write-Msg "Run '$InstallDir\$Binary.exe --help' to get started" "运行 '$InstallDir\$Binary.exe --help' 开始使用"
