#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_DIR="$(dirname "$SCRIPT_DIR")"
WEBUI_DIR="$(dirname "$BACKEND_DIR")/cc-gateway-webui"

echo "=== cc-gateway Build with Frontend ==="
echo ""

# Check if frontend source exists locally
if [ ! -d "$WEBUI_DIR" ]; then
  echo "Frontend source not found at $WEBUI_DIR"
  echo "This is a local build script for closed-source frontend."
  echo ""
  echo "To use the embedded frontend you have two options:"
  echo "  1. Local:   Place cc-gateway-webui at $WEBUI_DIR and run this script."
  echo "  2. CI:      Use GitHub Actions which pulls from a private repo."
  echo ""
  echo "Building backend WITHOUT embedded frontend..."
  cd "$BACKEND_DIR"
  cargo build --release
  echo ""
  echo "Done. The binary will show a placeholder page at /."
  exit 0
fi

# Build frontend
echo "[1/3] Building frontend at $WEBUI_DIR ..."
cd "$WEBUI_DIR"
npm ci
npm run build

# Copy to backend
echo "[2/3] Copying frontend build to backend ..."
rm -rf "$BACKEND_DIR/webui/dist"
mkdir -p "$BACKEND_DIR/webui/dist"
cp -r "$WEBUI_DIR/dist"/* "$BACKEND_DIR/webui/dist/"

# Build backend
echo "[3/3] Building Rust backend ..."
cd "$BACKEND_DIR"
cargo build --release

echo ""
echo "=== Build Complete ==="
echo "Binary: $BACKEND_DIR/target/release/cc-gateway"
echo ""
