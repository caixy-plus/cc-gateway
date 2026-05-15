#Requires -Version 5.1
$ErrorActionPreference = "Stop"

$Repo = "caixinyun/cc-gateway"
$Binary = "cc-gateway"
$InstallDir = "$env:LOCALAPPDATA\cc-gateway"

# Detect architecture
$Arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64"   { "x86_64" }
    "ARM64"   { "aarch64" }
    default   { throw "Unsupported architecture: $($env:PROCESSOR_ARCHITECTURE)" }
}

$Target = "$Arch-pc-windows-msvc"
Write-Host "Installing cc-gateway for $Target..."

# Download
$LatestUrl = "https://github.com/$Repo/releases/latest/download/$Binary-$Target.zip"
$TempFile = "$env:TEMP\$Binary.zip"
$TempDir = "$env:TEMP\$Binary-install"

Write-Host "Downloading from $LatestUrl..."
try {
    Invoke-WebRequest -Uri $LatestUrl -OutFile $TempFile -UseBasicParsing
} catch {
    Write-Error "Failed to download: $_"
    exit 1
}

# Extract
if (Test-Path $TempDir) { Remove-Item -Recurse -Force $TempDir }
New-Item -ItemType Directory -Path $TempDir | Out-Null
Expand-Archive -Path $TempFile -DestinationPath $TempDir -Force

# Install
New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
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
  "ai": {
    "enabled": false,
    "provider": "openai",
    "api_key": "${OPENAI_API_KEY}",
    "base_url": "https://api.openai.com/v1",
    "model": "gpt-4o-mini"
  },
  "claude": {
    "cli_path": "claude",
    "mode": "default",
    "model": "",
    "allowed_tools": ["Read", "Grep", "Glob", "Bash", "Edit", "Write"]
  },
  "feishu": {
    "enabled": true,
    "app_id": "${FEISHU_APP_ID}",
    "app_secret": "${FEISHU_APP_SECRET}",
    "allow_from": "*"
  },
  "workspace": {
    "scan_dirs": ["~/Workspace", "~/Projects"],
    "default_dir": "~/Workspace"
  }
}
"@
    Set-Content -Path "$ConfigDir\config.json" -Value $Config
    Write-Host "Created default config at $ConfigDir\config.json"
    Write-Host "Please edit it to add your Feishu app credentials."
}

# Add to PATH
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    Write-Host "Added $InstallDir to PATH"
}

# Cleanup
Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
Remove-Item -Force $TempFile -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "cc-gateway installed successfully to $InstallDir\$Binary.exe"
Write-Host "Run '$Binary --help' to get started"
