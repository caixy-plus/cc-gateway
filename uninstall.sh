#!/bin/sh
# cc-gateway uninstaller (macOS / Linux)
#
# Removes the binary, service files, and (unless --keep-data) the data directory.
#
# Usage:
#   sh uninstall.sh
#   sh uninstall.sh -- --keep-data
#   curl -fsSL .../uninstall.sh | sh
#   curl -fsSL .../uninstall.sh | sh -s -- --keep-data
set -u

# --- Bilingual messages -----------------------------------------------------
LANG_PREF="${LANG:-}"
msg() {
    case "$LANG_PREF" in
        zh_*|zh-*|*ZH*) printf '%s\n' "$2" ;;
        *) printf '%s\n' "$1" ;;
    esac
}

KEEP_DATA=0
for arg in "$@"; do
    case "$arg" in
        --keep-data) KEEP_DATA=1 ;;
    esac
done

BIN="$HOME/.local/bin/cc-gateway"
CONFIG_DIR="$HOME/.cc-gateway"

msg "Uninstalling cc-gateway..." "正在卸载 cc-gateway..."

# --- 1. Stop daemon + remove auto-start -------------------------------------
if [ -x "$BIN" ]; then
    "$BIN" stop >/dev/null 2>&1 || true
    "$BIN" disable >/dev/null 2>&1 || true
fi

# Force-kill daemon by PID file in case graceful shutdown failed.
if [ -f "$CONFIG_DIR/daemon.pid" ]; then
    kill "$(cat "$CONFIG_DIR/daemon.pid" 2>/dev/null)" 2>/dev/null || true
fi

# Remove service files directly.
rm -f "$HOME/Library/LaunchAgents/com.cc-gateway.daemon.plist" 2>/dev/null || true
launchctl remove com.cc-gateway.daemon >/dev/null 2>&1 || true
if [ -f "$HOME/.config/systemd/user/cc-gateway.service" ]; then
    systemctl --user disable cc-gateway.service >/dev/null 2>&1 || true
    rm -f "$HOME/.config/systemd/user/cc-gateway.service" 2>/dev/null || true
    systemctl --user daemon-reload >/dev/null 2>&1 || true
fi
msg "  - stopped daemon and removed auto-start" "  - 已停止守护进程并移除自启"

# --- 2. Remove the binary ---------------------------------------------------
if [ -f "$BIN" ]; then
    rm -f "$BIN" 2>/dev/null || true
    msg "  - removed binary: $BIN" "  - 已删除二进制：$BIN"
fi

# --- 3. Data directory ------------------------------------------------------
if [ "$KEEP_DATA" -eq 1 ]; then
    msg "  - kept data: $CONFIG_DIR" "  - 已保留数据：$CONFIG_DIR"
else
    rm -rf "$CONFIG_DIR" 2>/dev/null || true
    msg "  - removed data: $CONFIG_DIR" "  - 已删除数据：$CONFIG_DIR"
fi

# --- 4. Verify --------------------------------------------------------------
hash -r 2>/dev/null || true
REMAINING="$(command -v cc-gateway 2>/dev/null || true)"
if [ -n "$REMAINING" ]; then
    msg "" ""
    msg "WARNING: cc-gateway is still found at: $REMAINING" "警告：cc-gateway 仍在以下位置存在：$REMAINING"
    msg "It may have been installed to a non-standard location." "它可能被安装到了非标准位置。"
fi

msg "" ""
msg "cc-gateway has been uninstalled." "cc-gateway 已卸载完成。"
msg "Run 'hash -r' or open a new terminal." "请执行 'hash -r' 或打开新终端。"
