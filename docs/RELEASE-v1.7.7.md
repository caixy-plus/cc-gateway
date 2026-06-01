# Release v1.7.7

- **Windows 安装脚本编码** / Add UTF-8 BOM to PowerShell install scripts so Chinese strings parse correctly on PowerShell 5.1 (GBK default)
- **`cc-gateway update` 修复** / Re-write downloaded `install.ps1` with UTF-8 BOM before execution; fixes `TerminatorExpectedAtEndOfString` on update
- **安装文档输出** / Fix garbled Chinese in post-install documentation links when `install-docs.ps1` is downloaded from GitHub
