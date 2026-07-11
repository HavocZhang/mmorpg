#!/bin/bash
# MMORPG 全栈启动脚本
# 用法: bash start_all.sh
set -e
cd "$(dirname "$0")"

echo "Starting MMORPG full stack..."
echo ""

# 1. Gateway
echo "[1/4] Gateway (7888/9090)..."
rm -f gate1.log
./target/release/rust-mmo-gate.exe > gate1.log 2>&1 &
sleep 2

# 2. Logic Server
echo "[2/4] Logic Server (50051)..."
rm -f logic.log
./logic-server.exe > logic.log 2>&1 &
sleep 2

# 3. WS Proxy
echo "[3/4] WS Proxy (9000)..."
cd web-client
NODE_PATH="$HOME/.workbuddy/binaries/node/workspace/node_modules" node ws_proxy.js > /dev/null 2>&1 &
sleep 1

# 4. HTTP Server
echo "[4/4] HTTP Server (4000)..."
python -m http.server 4000 > /dev/null 2>&1 &
sleep 2

# Verify
echo ""
curl -s http://127.0.0.1:9090/health | grep -q '"status":"ok"' && echo "✓ Gateway OK" || echo "✗ Gateway FAIL"
curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:4000/game.html | grep -q 200 && echo "✓ HTTP OK" || echo "✗ HTTP FAIL"
echo ""
echo "Ready: http://localhost:4000/game.html"
echo "Tests: node test_game_e2e.js"
