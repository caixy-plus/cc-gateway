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

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$WebUiDir = Join-Path (Split-Path -Parent $ScriptDir) "cc-gateway-webui"

if (Test-Path $WebUiDir -PathType Container) {
    try {
        npm --version | Out-Null
        Write-Msg "1. Building frontend..." "1. 构建前端..."
        Push-Location $WebUiDir
        npm ci
        npm run build
        Pop-Location
        $DistDir = Join-Path $ScriptDir "webui\dist"
        if (Test-Path $DistDir) { Remove-Item $DistDir -Recurse -Force }
        New-Item -ItemType Directory -Path $DistDir -Force | Out-Null
        Copy-Item -Path (Join-Path $WebUiDir "dist\*") -Destination $DistDir -Recurse -Force
        Write-Msg "   Frontend embedded" "   前端已嵌入"
    } catch {
        Write-Msg "   Frontend build skipped: $_" "   跳过前端构建: $_"
    }
} else {
    Write-Msg "   Frontend source not found, skipping..." "   未找到前端源码，跳过..."
}

Write-Msg "2. Building release version..." "2. 构建 release 版本..."
cargo build --release

Write-Msg "2. Stopping all cc-gateway processes..." "2. 停止所有 cc-gateway 进程..."
Stop-AllCCGateway

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
        Write-Msg "Default port $defaultPort is used by cc-gateway (PID: $cgPid), continuing..." "默认端口 $defaultPort 可以使用，继续..."
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
try {
    # Never block local install on daemon startup. Add a timeout guard.
    $p = Start-Process -FilePath "$InstallDir\$Binary.exe" -ArgumentList @("start") -PassThru -WindowStyle Hidden
    try {
        Wait-Process -Id $p.Id -Timeout 15 -ErrorAction SilentlyContinue | Out-Null
    } catch {}
} catch {
    Write-Msg "Failed to start daemon: $_" "启动守护进程失败: $_"
}

Write-Msg "" ""
Write-Msg "cc-gateway installed successfully to $InstallDir\$Binary.exe" "cc-gateway 已成功安装到 $InstallDir\$Binary.exe"
Write-Msg "Run '$InstallDir\$Binary.exe --help' to get started" "运行 '$InstallDir\$Binary.exe --help' 开始使用"

Write-Msg "" ""
Write-Msg "Open WebUI (starts daemon if needed)..." "打开 WebUI（如未启动会自动 start）..."
try {
    & "$InstallDir\$Binary.exe" webui | Out-Null
} catch {
    # Ignore browser open failures during local install
}
