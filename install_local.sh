#!/bin/sh

DEFAULT_PORT=17534
CONFIG_FILE="$HOME/.cc-gateway/config.json"
PID_FILE="$HOME/.cc-gateway/daemon.pid"

is_port_in_use() {
    python3 -c "import socket; s=socket.socket(); s.settimeout(0.5); s.connect(('127.0.0.1', $1)); s.close()" 2>/dev/null
}

is_process_alive() {
    kill -0 "$1" 2>/dev/null
}

# Check if default port is occupied by another program
if is_port_in_use "$DEFAULT_PORT"; then
    CG_PID=""
    if [ -f "$PID_FILE" ]; then
        CG_PID=$(cat "$PID_FILE" | tr -d ' \n')
    fi

    if [ -n "$CG_PID" ] && is_process_alive "$CG_PID"; then
        echo "默认端口 $DEFAULT_PORT 已被 cc-gateway 占用 (PID: $CG_PID)，继续..."
    else
        echo "默认端口 $DEFAULT_PORT 被其他进程占用"
        NEW_PORT=$DEFAULT_PORT
        while is_port_in_use "$NEW_PORT"; do
            NEW_PORT=$((NEW_PORT + 1))
        done
        echo "自动分配新端口: $NEW_PORT"

        if [ -f "$CONFIG_FILE" ]; then
            python3 -c "
import json
with open('$CONFIG_FILE', 'r') as f:
    config = json.load(f)
config['port'] = $NEW_PORT
with open('$CONFIG_FILE', 'w') as f:
    json.dump(config, f, indent=2, ensure_ascii=False)
    f.write('\n')
"
            echo "已更新配置文件: $CONFIG_FILE (port = $NEW_PORT)"
        fi
    fi
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WEBUI_DIR="$(dirname "$SCRIPT_DIR")/cc-gateway-webui"

# Build frontend if source exists
if [ -d "$WEBUI_DIR" ] && command -v npm >/dev/null 2>&1; then
    echo "1. 构建前端 ..."
    cd "$WEBUI_DIR"
    npm ci
    npm run build
    rm -rf "$SCRIPT_DIR/webui/dist"
    mkdir -p "$SCRIPT_DIR/webui/dist"
    cp -r "$WEBUI_DIR/dist"/* "$SCRIPT_DIR/webui/dist/"
    cd "$SCRIPT_DIR"
    echo "   前端已嵌入"
else
    echo "1. 跳过前端构建（源码未找到或缺少 npm）"
fi

echo "2. 构建 release 版本"
cargo build --release

echo "2. 复制到 PATH（避免 inode 复用问题）"
rm -f ~/.local/bin/cc-gateway
cp target/release/cc-gateway ~/.local/bin/cc-gateway
chmod +x ~/.local/bin/cc-gateway

OS=$(uname -s)
if [ "$OS" = "Darwin" ]; then
    echo "3. macOS 重签名（必须，否则 daemon 会被 Gatekeeper 杀掉）"
    codesign -s - -f ~/.local/bin/cc-gateway
fi

echo "4. 重启 cc-gateway..."
cc-gateway restart
