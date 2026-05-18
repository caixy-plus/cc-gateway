#!/usr/bin/env bash
set -e

REPO="caixy-plus/cc-gateway"
BINARY="cc-gateway"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$ARCH" in
    x86_64) ARCH="x86_64" ;;
    amd64)  ARCH="x86_64" ;;
    arm64)  ARCH="aarch64" ;;
    aarch64) ARCH="aarch64" ;;
    *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

case "$OS" in
    linux)  TARGET="${ARCH}-unknown-linux-gnu" ;;
    darwin) TARGET="${ARCH}-apple-darwin" ;;
    *) echo "Unsupported OS: $OS"; exit 1 ;;
esac

echo "Installing cc-gateway for ${TARGET}..."

# Get latest release URL
LATEST_URL="https://github.com/${REPO}/releases/latest/download/${BINARY}-${TARGET}.tar.gz"

# Create temp directory
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

# Download
echo "Downloading from ${LATEST_URL}..."
if command -v curl &> /dev/null; then
    curl -fsSL "$LATEST_URL" -o "$TMP_DIR/${BINARY}.tar.gz"
elif command -v wget &> /dev/null; then
    wget -q "$LATEST_URL" -O "$TMP_DIR/${BINARY}.tar.gz"
else
    echo "curl or wget is required"
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
    echo "Created default config at $CONFIG_DIR/config.json"
    echo "Please edit it to add your Feishu app credentials."
fi

# Setup PATH
SHELL_CONFIG=""
if [ -n "$ZSH_VERSION" ] || [ "$SHELL" = "/bin/zsh" ]; then
    SHELL_CONFIG="$HOME/.zshrc"
elif [ -n "$BASH_VERSION" ] || [ "$SHELL" = "/bin/bash" ]; then
    SHELL_CONFIG="$HOME/.bashrc"
fi

if [ -n "$SHELL_CONFIG" ] && [ -f "$SHELL_CONFIG" ]; then
    if ! grep -q "$INSTALL_DIR" "$SHELL_CONFIG"; then
        echo "export PATH=\"$INSTALL_DIR:\$PATH\"" >> "$SHELL_CONFIG"
        echo "Added $INSTALL_DIR to PATH in $SHELL_CONFIG"
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
    echo "Created launchd plist at $PLIST_FILE"
    echo "Run 'launchctl load $PLIST_FILE' to start on boot"
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
    echo "Created systemd user service at $SERVICE_FILE"
    echo "Run 'systemctl --user enable --now cc-gateway' to start on boot"
fi

echo ""
echo "cc-gateway installed successfully to $INSTALL_DIR/${BINARY}"
echo "Run '${BINARY} --help' to get started"
