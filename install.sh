#!/bin/sh
set -e

REPO="caixy-plus/cc-gateway"
BINARY="cc-gateway"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
DEFAULT_PORT=17534

# Language detection
detect_lang() {
    if [ -n "$CC_GATEWAY_LANG" ]; then
        case "$CC_GATEWAY_LANG" in
            zh*) echo "zh" ;;
            *) echo "en" ;;
        esac
        return
    fi
    if [ -n "$LANG" ]; then
        case "$LANG" in
            zh*) echo "zh" ;;
            *) echo "en" ;;
        esac
        return
    fi
    echo "en"
}

LANG_CODE=$(detect_lang)

msg() {
    en="$1"
    zh="$2"
    if [ "$LANG_CODE" = "zh" ]; then
        echo "$zh"
    else
        echo "$en"
    fi
}

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$ARCH" in
    x86_64) ARCH="x86_64" ;;
    amd64)  ARCH="x86_64" ;;
    arm64)  ARCH="aarch64" ;;
    aarch64) ARCH="aarch64" ;;
    *) msg "Unsupported architecture: $ARCH" "不支持的架构: $ARCH"; exit 1 ;;
esac

case "$OS" in
    linux)  TARGET="${ARCH}-unknown-linux-gnu" ;;
    darwin) TARGET="${ARCH}-apple-darwin" ;;
    *) msg "Unsupported OS: $OS" "不支持的操作系统: $OS"; exit 1 ;;
esac

msg "Installing cc-gateway for ${TARGET}..." "正在安装 cc-gateway (${TARGET})..."

# Get latest release URL
LATEST_URL="https://github.com/${REPO}/releases/latest/download/${BINARY}-${TARGET}.tar.gz"

# Create temp directory
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

# Download
msg "Downloading from ${LATEST_URL}..." "正在下载: ${LATEST_URL}..."
if command -v curl > /dev/null 2>&1; then
    curl -fsSL "$LATEST_URL" -o "$TMP_DIR/${BINARY}.tar.gz"
elif command -v wget > /dev/null 2>&1; then
    wget -q "$LATEST_URL" -O "$TMP_DIR/${BINARY}.tar.gz"
else
    msg "curl or wget is required" "需要安装 curl 或 wget"
    exit 1
fi

# Extract
tar -xzf "$TMP_DIR/${BINARY}.tar.gz" -C "$TMP_DIR"

# macOS: re-sign locally so the ad-hoc signature is bound to this machine.
# Signing on the GitHub Actions runner sometimes fails after download due
# to Gatekeeper / signature-validation caches when overwriting an existing
# signed binary. We sign in the temp dir, remove the old binary, then copy
# the freshly-signed one — this avoids inode-reuse issues.
if [ "$OS" = "darwin" ] && command -v codesign > /dev/null 2>&1; then
    if ! codesign -s - -f "$TMP_DIR/${BINARY}" 2>/dev/null; then
        msg "Warning: local codesign failed, falling back to GitHub-signed binary" "警告: 本地签名失败，将使用 GitHub 构建的签名版本"
    fi
fi

# Install binary
mkdir -p "$INSTALL_DIR"
rm -f "$INSTALL_DIR/${BINARY}"
cp "$TMP_DIR/${BINARY}" "$INSTALL_DIR/"
chmod +x "$INSTALL_DIR/${BINARY}"

# macOS: remove quarantine attribute so Gatekeeper doesn't kill the binary
if [ "$OS" = "darwin" ] && command -v xattr > /dev/null 2>&1; then
    xattr -dr com.apple.quarantine "$INSTALL_DIR/${BINARY}" 2>/dev/null || true
fi

# Post-install setup (config wizard, port, PATH, service files, init) — skipped for `cc-gateway update`.
if [ -n "${CC_GATEWAY_SKIP_SETUP:-}" ]; then
    msg "" ""
    msg "cc-gateway installed successfully to $INSTALL_DIR/${BINARY}" "cc-gateway 已成功安装到 $INSTALL_DIR/${BINARY}"
    exit 0
fi

# Create config directory
CONFIG_DIR="$HOME/.cc-gateway"
mkdir -p "$CONFIG_DIR/logs"

# Config file creation and initialization are handled by `cc-gateway init`.

# Setup PATH
SHELL_CONFIG=""
if [ -n "$ZSH_VERSION" ] || [ "$SHELL" = "/bin/zsh" ]; then
    SHELL_CONFIG="$HOME/.zshrc"
elif [ "$SHELL" = "/bin/bash" ]; then
    SHELL_CONFIG="$HOME/.bashrc"
fi

if [ -n "$SHELL_CONFIG" ] && [ -f "$SHELL_CONFIG" ]; then
    if ! grep -q "$INSTALL_DIR" "$SHELL_CONFIG"; then
        echo "export PATH=\"$INSTALL_DIR:\$PATH\"" >> "$SHELL_CONFIG"
        msg "Added $INSTALL_DIR to PATH in $SHELL_CONFIG" "已将 $INSTALL_DIR 添加到 PATH ($SHELL_CONFIG)"
    fi
    # Apply shell config in this install session (init/start need cc-gateway on PATH).
    # shellcheck disable=SC1090
    . "$SHELL_CONFIG" 2>/dev/null || true
fi
# Ensure install dir is on PATH for the rest of this install run.
export PATH="$INSTALL_DIR:$PATH"

# macOS: setup launchd plist
if [ "$OS" = "darwin" ]; then
    PLIST_DIR="$HOME/Library/LaunchAgents"
    PLIST_FILE="$PLIST_DIR/com.cc-gateway.daemon.plist"
    mkdir -p "$PLIST_DIR"
    cat > "$PLIST_FILE" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.cc-gateway.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>$INSTALL_DIR/cc-gateway</string>
        <string>start</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>$CONFIG_DIR/logs/daemon.stdout</string>
    <key>StandardErrorPath</key>
    <string>$CONFIG_DIR/logs/daemon.stderr</string>
</dict>
</plist>
EOF
    msg "Created launchd plist at $PLIST_FILE" "已创建 launchd plist: $PLIST_FILE"
    msg "Run 'launchctl load $PLIST_FILE' to start on boot" "运行 'launchctl load $PLIST_FILE' 以开机启动"
fi

# Linux: setup systemd user service
if [ "$OS" = "linux" ]; then
    SYSTEMD_DIR="$HOME/.config/systemd/user"
    SERVICE_FILE="$SYSTEMD_DIR/cc-gateway.service"
    mkdir -p "$SYSTEMD_DIR"
    cat > "$SERVICE_FILE" << EOF
[Unit]
Description=cc-gateway daemon
After=network.target

[Service]
Type=simple
ExecStart=$INSTALL_DIR/cc-gateway start
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
EOF
    msg "Created systemd user service at $SERVICE_FILE" "已创建 systemd 用户服务: $SERVICE_FILE"
    msg "Run 'systemctl --user enable --now cc-gateway' to start on boot" "运行 'systemctl --user enable --now cc-gateway' 以开机启动"
fi

msg "" ""
msg "Running initial setup..." "正在运行初始设置..."
if command -v cc-gateway > /dev/null 2>&1; then
    INIT_CMD="cc-gateway init"
else
    INIT_CMD="$INSTALL_DIR/cc-gateway init"
fi
if [ -t 0 ]; then
    $INIT_CMD
elif [ -r /dev/tty ]; then
    $INIT_CMD </dev/tty
else
    $INIT_CMD < /dev/null
fi

msg "" ""
msg "Restarting cc-gateway daemon..." "正在重启 cc-gateway 守护进程..."
if command -v cc-gateway > /dev/null 2>&1; then
    cc-gateway restart || true
else
    "$INSTALL_DIR/cc-gateway" restart || true
fi

msg "" ""
msg "cc-gateway installed successfully to $INSTALL_DIR/${BINARY}" "cc-gateway 已成功安装到 $INSTALL_DIR/${BINARY}"
msg "Run '${BINARY} --help' to get started" "运行 '${BINARY} --help' 开始使用"
msg "" ""
msg "For Feishu bot setup instructions, see:" "飞书机器人配置说明请参阅:"
msg "  https://github.com/caixy-plus/cc-gateway/blob/main/docs/config.md#feishu-setup" "  https://github.com/caixy-plus/cc-gateway/blob/main/docs/config.md#feishu-setup"
