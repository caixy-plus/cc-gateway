#!/bin/sh
set -e

REPO="caixy-plus/cc-gateway"
BINARY="cc-gateway"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

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

# Install binary
mkdir -p "$INSTALL_DIR"
cp "$TMP_DIR/${BINARY}" "$INSTALL_DIR/"
chmod +x "$INSTALL_DIR/${BINARY}"

# Create config directory
CONFIG_DIR="$HOME/.cc-gateway"
mkdir -p "$CONFIG_DIR/logs"

if [ ! -f "$CONFIG_DIR/config.json" ]; then
    cat > "$CONFIG_DIR/config.json" << 'EOF'
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
  "default_dir": "~"
}
EOF
    msg "Created default config at $ConfigDir/config.json" "已创建默认配置: $ConfigDir/config.json"
    msg "Please edit it to add your Feishu app credentials." "请编辑配置文件添加飞书应用凭证。"
fi

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
    # Source the config so PATH is effective immediately in the current session
    if [ -n "$PS1" ] || [ -n "$ZSH_VERSION" ]; then
        # shellcheck source=/dev/null
        . "$SHELL_CONFIG"
        msg "Sourced $SHELL_CONFIG" "已加载 $SHELL_CONFIG"
    fi
fi

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
    cc-gateway init
else
    "$INSTALL_DIR/cc-gateway" init
fi

msg "" ""
msg "cc-gateway installed successfully to $INSTALL_DIR/${BINARY}" "cc-gateway 已成功安装到 $INSTALL_DIR/${BINARY}"
msg "Run '${BINARY} --help' to get started" "运行 '${BINARY} --help' 开始使用"
msg "" ""
msg "For Feishu bot setup instructions, see:" "飞书机器人配置说明请参阅:"
msg "  https://github.com/caixy-plus/cc-gateway/blob/main/docs/config.md#feishu-setup" "  https://github.com/caixy-plus/cc-gateway/blob/main/docs/config.md#feishu-setup"
