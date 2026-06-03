#Requires -Version 5.1
# ASCII-only bootstrap for: irm .../install-irm.ps1 | iex
# Downloads install.ps1 to disk (UTF-8 BOM), then runs it. Avoids leading mojibake from piping install.ps1 directly.
$ErrorActionPreference = "Stop"

$Repo = "caixy-plus/cc-gateway"
$Uri = "https://raw.githubusercontent.com/$Repo/main/install.ps1"
$Installer = Join-Path $env:TEMP "cc-gateway-install.ps1"

Invoke-WebRequest -Uri $Uri -OutFile $Installer -UseBasicParsing

$utf8NoBom = New-Object System.Text.UTF8Encoding $false
$utf8Bom = New-Object System.Text.UTF8Encoding $true
$text = [System.IO.File]::ReadAllText($Installer, $utf8NoBom).TrimStart([char]0xFEFF)
[System.IO.File]::WriteAllText($Installer, $text, $utf8Bom)

& $Installer
exit $LASTEXITCODE
