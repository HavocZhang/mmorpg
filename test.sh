#!/bin/bash
# ========== 全量TDD+BDD测试脚本 ==========
set -e

echo "🧪 开始全量测试..."

cargo test --all --no-fail-fast
cargo cucumber

echo "✅ 单元/并发/异常/BDD场景全量验收通过"
