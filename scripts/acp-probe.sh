#!/usr/bin/env sh
# ACP 权限请求探测器 —— 对每个已安装的 provider CLI 跑一遍 acp-probe.mjs，
# 把原始 JSON 流写到 acp-probe-<provider>.log，把权限请求汇总写到 acp-probe-summary.json。
#
# 用法：
#   ./scripts/acp-probe.sh            # 自动检测所有已装 provider
#   ./scripts/acp-probe.sh codex-acp  # 只跑指定 provider
#
# 依赖：Node.js（用来跑 acp-probe.mjs）。
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROBE="$SCRIPT_DIR/acp-probe.mjs"
WORK_DIR="${ACPPROBE_WORK_DIR:-$(mktemp -d -t acp-probe-XXXXXX)}"
OUT_DIR="${ACPPROBE_OUT_DIR:-$PWD/acp-probe-results}"

ALL_PROVIDERS="codex-acp opencode kimi gemini qoder pi"
TARGETS="${*:-$ALL_PROVIDERS}"

mkdir -p "$OUT_DIR"
echo "[acp-probe] work_dir=$WORK_DIR"
echo "[acp-probe] out_dir=$OUT_DIR"

summary_file="$OUT_DIR/summary.json"
: > "$summary_file"
echo "[" >> "$summary_file"
first=1

for p in $TARGETS; do
  bin=""
  case "$p" in
    codex-acp) bin="codex-acp" ;;
    opencode)  bin="opencode"  ;;
    kimi)      bin="kimi"      ;;
    gemini)    bin="gemini"    ;;
    qoder)     bin="qoderclicn";;
    pi)        bin="pi"        ;;
    *) echo "[acp-probe] unknown provider: $p"; continue ;;
  esac

  if ! command -v "$bin" >/dev/null 2>&1; then
    echo "[acp-probe] skip $p (binary $bin not on PATH)"
    continue
  fi

  echo "[acp-probe] ====== $p ======"
  log_file="$OUT_DIR/$p.log"
  json_file="$OUT_DIR/$p.json"
  (
    cd "$WORK_DIR"
    node "$PROBE" "$p" "$WORK_DIR" > "$json_file" 2> "$log_file" || true
  )

  # 把结果追加到汇总 JSON 数组
  if [ $first -eq 1 ]; then first=0; else echo "," >> "$summary_file"; fi
  # 只把 permissionRequests 部分抽出来（用 node 解析，避免依赖 jq）
  node -e "
    const fs = require('fs');
    try {
      const obj = JSON.parse(fs.readFileSync('$json_file', 'utf8'));
      const out = {
        provider: obj.provider,
        captured: obj.permissionRequestCount,
        requests: obj.permissionRequests,
      };
      process.stdout.write(JSON.stringify(out, null, 2));
    } catch (e) {
      process.stdout.write(JSON.stringify({ provider: '$p', error: e.message }));
    }
  " >> "$summary_file"
done

echo "" >> "$summary_file"
echo "]" >> "$summary_file"

echo "[acp-probe] summary written to $summary_file"
echo "[acp-probe] raw logs in $OUT_DIR"
