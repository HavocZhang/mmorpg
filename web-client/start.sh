#!/bin/bash
# 启动网关 + WebSocket 代理
# 用法: ./start.sh

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "═══════════════════════════════════════════"
echo "  Rust MMO Gate - 一键启动"
echo "═══════════════════════════════════════════"

# 1. 启动网关
if curl -s http://localhost:9090/health >/dev/null 2>&1; then
  echo "✅ 网关已在运行"
else
  echo "🚀 启动网关服务器..."
  cd "$PROJECT_DIR"
  APP_ENV=dev ./target/debug/rust-mmo-gate.exe &
  GATE_PID=$!
  echo "   PID: $GATE_PID"
  
  # 等待网关就绪
  for i in $(seq 1 10); do
    if curl -s http://localhost:9090/health >/dev/null 2>&1; then
      echo "✅ 网关已就绪"
      break
    fi
    sleep 1
  done
fi

# 2. 启动 WebSocket 代理
if curl -s http://localhost:3000/config >/dev/null 2>&1; then
  echo "✅ 代理已在运行"
else
  echo "🚀 启动 WebSocket 代理..."
  cd "$SCRIPT_DIR"
  node proxy.js &
  PROXY_PID=$!
  echo "   PID: $PROXY_PID"
  sleep 1
  echo "✅ 代理已就绪"
fi

echo ""
echo "═══════════════════════════════════════════"
echo "  全部就绪！"
echo "═══════════════════════════════════════════"
echo "  网页客户端:  http://localhost:3000"
echo "  网关监控:    http://localhost:9090/health"
echo "  网关会话:    http://localhost:9090/sessions"
echo "═══════════════════════════════════════════"
echo ""
echo "打开浏览器访问 http://localhost:3000 开始测试"
echo "可以打开多个标签页来创建更多连接！"
echo ""
echo "按 Ctrl+C 停止所有服务"
echo ""

# 等待退出
wait
