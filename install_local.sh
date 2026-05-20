#!/bin/sh

echo "1. 构建 release 版本"
cargo build --release

echo "2. 复制到 PATH（避免 inode 复用问题）"
rm -f ~/.local/bin/cc-gateway
cp target/release/cc-gateway ~/.local/bin/cc-gateway
chmod +x ~/.local/bin/cc-gateway

echo "3. macOS 重签名（必须，否则 daemon 会被 Gatekeeper 杀掉）"
codesign -s - -f ~/.local/bin/cc-gateway

cc-gateway restart