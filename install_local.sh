#!/bin/sh

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Language (same as install.sh)
detect_lang() {
    if [ -n "$CC_GATEWAY_LANG" ]; then
        case "$CC_GATEWAY_LANG" in zh*) echo "zh" ;; *) echo "en" ;; esac
        return
    fi
    if [ -n "$LANG" ]; then
        case "$LANG" in zh*) echo "zh" ;; *) echo "en" ;; esac
        return
    fi
    echo "en"
}
LANG_CODE=$(detect_lang)
msg() {
    if [ "$LANG_CODE" = "zh" ]; then echo "$2"; else echo "$1"; fi
}

DEFAULT_PORT=17534
CONFIG_FILE="$HOME/.cc-gateway/config.json"
PID_FILE="$HOME/.cc-gateway/daemon.pid"

is_port_in_use() {
    python3 - "$1" "$2" <<'PY' 2>/dev/null
import errno
import socket
import sys

host = sys.argv[1]
port = int(sys.argv[2])
family = socket.AF_INET6 if ":" in host else socket.AF_INET
s = socket.socket(family, socket.SOCK_STREAM)
try:
    s.bind((host, port))
except OSError as e:
    sys.exit(0 if e.errno == errno.EADDRINUSE else 1)
finally:
    s.close()
sys.exit(1)
PY
}

is_process_alive() {
    kill -0 "$1" 2>/dev/null
}

is_cc_gateway_process() {
    [ -n "$1" ] || return 1
    is_process_alive "$1" || return 1
    COMM=$(ps -p "$1" -o comm= 2>/dev/null | awk '{print $1}')
    [ "$(basename "$COMM")" = "cc-gateway" ]
}

configured_port() {
    if [ -f "$CONFIG_FILE" ]; then
        python3 - "$CONFIG_FILE" "$DEFAULT_PORT" <<'PY' 2>/dev/null || echo "$DEFAULT_PORT"
import json
import sys

path = sys.argv[1]
default = int(sys.argv[2])
with open(path, "r", encoding="utf-8") as f:
    config = json.load(f)
port = int(config.get("port") or default)
if not 1 <= port <= 65535:
    raise ValueError("invalid port")
print(port)
PY
    else
        echo "$DEFAULT_PORT"
    fi
}

configured_bind_address() {
    if [ -f "$CONFIG_FILE" ]; then
        python3 - "$CONFIG_FILE" <<'PY' 2>/dev/null || echo "127.0.0.1"
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as f:
    config = json.load(f)
print(config.get("bind_address") or "127.0.0.1")
PY
    else
        echo "127.0.0.1"
    fi
}

write_config_port() {
    python3 - "$CONFIG_FILE" "$1" <<'PY'
import json
import sys

path = sys.argv[1]
port = int(sys.argv[2])
with open(path, "r", encoding="utf-8") as f:
    config = json.load(f)
config["port"] = port
with open(path, "w", encoding="utf-8") as f:
    json.dump(config, f, indent=2, ensure_ascii=False)
    f.write("\n")
PY
}

CONFIG_PORT=$(configured_port)
CONFIG_BIND_ADDRESS=$(configured_bind_address)

# Check the effective configured bind address + port, not always the default port.
# Existing users may already run cc-gateway on a custom port, and default-port
# checks must not rewrite it.
if is_port_in_use "$CONFIG_BIND_ADDRESS" "$CONFIG_PORT"; then
    CG_PID=""
    if [ -f "$PID_FILE" ]; then
        CG_PID=$(cat "$PID_FILE" | tr -d ' \n')
    fi

    if is_cc_gateway_process "$CG_PID"; then
        echo "配置端口 $CONFIG_BIND_ADDRESS:$CONFIG_PORT 正由 cc-gateway 使用，继续..."
    else
        echo "配置端口 $CONFIG_BIND_ADDRESS:$CONFIG_PORT 被其他进程占用"
        NEW_PORT=$CONFIG_PORT
        while is_port_in_use "$CONFIG_BIND_ADDRESS" "$NEW_PORT"; do
            NEW_PORT=$((NEW_PORT + 1))
        done
        echo "自动分配新端口: $NEW_PORT"

        if [ -f "$CONFIG_FILE" ]; then
            write_config_port "$NEW_PORT"
            echo "已更新配置文件: $CONFIG_FILE (port = $NEW_PORT)"
        fi
    fi
fi

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

echo ""
msg "5. Open WebUI (starts daemon if needed)..." "5. 打开 WebUI（如未启动会自动 start）..."
cc-gateway webui || true

if [ -f "$SCRIPT_DIR/scripts/install-docs.sh" ]; then
    # shellcheck disable=SC1090
    . "$SCRIPT_DIR/scripts/install-docs.sh"
    print_install_docs "$SCRIPT_DIR"
fi
