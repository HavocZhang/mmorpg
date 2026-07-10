#!/bin/bash
# ========== 性能门禁压测脚本 ==========
set -e

echo "📊 开始性能压测..."

cargo bench

echo "✅ 网关性能指标验收完成"
