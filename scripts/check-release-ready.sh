#!/usr/bin/env bash
# Pre-release checks for cc-gateway. Run from backend repo root before tagging.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_DIR="$(dirname "$SCRIPT_DIR")"
WEBUI_DIR="${WEBUI_DIR:-$(dirname "$BACKEND_DIR")/cc-gateway-webui}"
WEBUI_REMOTE="${WEBUI_REMOTE:-origin}"
WEBUI_BRANCH="${WEBUI_BRANCH:-main}"

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

fail() {
  echo -e "${RED}ERROR:${NC} $*" >&2
  exit 1
}

ok() {
  echo -e "${GREEN}OK:${NC} $*"
}

warn() {
  echo "WARN: $*" >&2
}

echo "=== cc-gateway release pre-flight ==="
echo ""

# Backend dirty (warning only — may be intentional before commit)
if [ -d "$BACKEND_DIR/.git" ]; then
  if [ -n "$(git -C "$BACKEND_DIR" status --porcelain)" ]; then
    warn "Backend repo has uncommitted changes (commit before tag if shipping them)."
  else
    ok "Backend working tree clean."
  fi
fi

# Frontend repo required for a correct release embed
if [ ! -d "$WEBUI_DIR/.git" ]; then
  fail "Frontend repo not found at $WEBUI_DIR — clone cc-gateway-webui as a sibling directory, or set WEBUI_DIR."
fi

if [ -n "$(git -C "$WEBUI_DIR" status --porcelain)" ]; then
  fail "cc-gateway-webui has uncommitted changes. Commit and push before tagging the backend."
fi
ok "Frontend working tree clean."

if ! git -C "$WEBUI_DIR" remote get-url "$WEBUI_REMOTE" &>/dev/null; then
  fail "Remote '$WEBUI_REMOTE' not configured in $WEBUI_DIR"
fi

git -C "$WEBUI_DIR" fetch "$WEBUI_REMOTE" "$WEBUI_BRANCH" --quiet

LOCAL_SHA="$(git -C "$WEBUI_DIR" rev-parse HEAD)"
REMOTE_SHA="$(git -C "$WEBUI_DIR" rev-parse "$WEBUI_REMOTE/$WEBUI_BRANCH")"

if [ "$LOCAL_SHA" != "$REMOTE_SHA" ]; then
  fail "cc-gateway-webui is not pushed to $WEBUI_REMOTE/$WEBUI_BRANCH.
  local:  $LOCAL_SHA
  remote: $REMOTE_SHA
  Run: cd $WEBUI_DIR && git push $WEBUI_REMOTE $WEBUI_BRANCH"
fi
ok "Frontend pushed ($WEBUI_REMOTE/$WEBUI_BRANCH @ ${LOCAL_SHA:0:12})."

# Cargo version hint when on a tag or about to release
CARGO_VER="$(grep -m1 '^version' "$BACKEND_DIR/Cargo.toml" | sed 's/.*"\(.*\)"/\1/')"
echo ""
echo "Backend Cargo.toml version: $CARGO_VER"
echo "Tag must be exactly: v$CARGO_VER"
echo ""
echo "CI will embed WebUI from GitHub: caixy-plus/cc-gateway-webui @ $WEBUI_BRANCH"
echo "See docs/release.md (docs/release.zh-CN.md) for the full checklist."
echo ""
ok "Pre-flight passed. You may bump version, tag, and push."
