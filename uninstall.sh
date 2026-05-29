#!/bin/sh
# cc-gateway uninstaller (macOS / Linux)
#
# Removes everything the installer created: the daemon, auto-start service,
# the binary, the PATH entry, and (unless --keep-data) the data directory.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/uninstall.sh | sh
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

CONFIG_DIR="$HOME/.cc-gateway"

# --- Locate the installed binary --------------------------------------------
BIN="$(command -v cc-gateway 2>/dev/null || true)"
if [ -z "$BIN" ]; then
    for d in "${INSTALL_DIR:-}" "$HOME/.local/bin" /usr/local/bin /opt/homebrew/bin /usr/bin; do
        [ -n "$d" ] && [ -x "$d/cc-gateway" ] && BIN="$d/cc-gateway" && break
    done
fi
INSTALL_DIR="$(dirname "$BIN" 2>/dev/null || true)"

msg "Uninstalling cc-gateway..." "正在卸载 cc-gateway..."

# --- 1. Stop daemon + remove auto-start (delegate to the binary) -------------
if [ -n "$BIN" ] && [ -x "$BIN" ]; then
    "$BIN" stop >/dev/null 2>&1 || true
    "$BIN" disable >/dev/null 2>&1 || true
fi

# Belt-and-suspenders: remove service files directly in case the binary is gone.
rm -f "$HOME/Library/LaunchAgents/com.cc-gateway.daemon.plist" 2>/dev/null || true
launchctl remove com.cc-gateway.daemon >/dev/null 2>&1 || true
if [ -f "$HOME/.config/systemd/user/cc-gateway.service" ]; then
    systemctl --user disable cc-gateway.service >/dev/null 2>&1 || true
    rm -f "$HOME/.config/systemd/user/cc-gateway.service" 2>/dev/null || true
    systemctl --user daemon-reload >/dev/null 2>&1 || true
fi
msg "  - stopped daemon and removed auto-start" "  - 已停止守护进程并移除自启"

# --- 2. Remove the binary ---------------------------------------------------
if [ -n "$BIN" ] && [ -e "$BIN" ]; then
    rm -f "$BIN" 2>/dev/null || true
    msg "  - removed binary: $BIN" "  - 已删除二进制：$BIN"
fi

# --- 3. Scrub the PATH line the installer appended --------------------------
if [ -n "$INSTALL_DIR" ]; then
    PATH_LINE="export PATH=\"$INSTALL_DIR:\$PATH\""
    for rc in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.profile"; do
        [ -f "$rc" ] || continue
        if grep -qF "$PATH_LINE" "$rc" 2>/dev/null; then
            tmp="$rc.ccg-uninstall.$$"
            if grep -vF "$PATH_LINE" "$rc" > "$tmp" 2>/dev/null; then
                mv "$tmp" "$rc"
                msg "  - cleaned PATH entry in $rc" "  - 已清理 $rc 中的 PATH 配置"
            else
                rm -f "$tmp" 2>/dev/null || true
            fi
        fi
    done
fi

# --- 4. Data directory ------------------------------------------------------
if [ "$KEEP_DATA" -eq 1 ]; then
    msg "  - kept data: $CONFIG_DIR" "  - 已保留数据：$CONFIG_DIR"
else
    rm -rf "$CONFIG_DIR" 2>/dev/null || true
    msg "  - removed data: $CONFIG_DIR" "  - 已删除数据：$CONFIG_DIR"
fi

msg "cc-gateway has been uninstalled." "cc-gateway 已卸载完成。"
msg "Open a new terminal for PATH changes to take effect." "请打开新终端以使 PATH 变更生效。"
