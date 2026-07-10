#!/bin/bash
# ========== 生产编译脚本 ==========
set -e

echo "🚀 开始生产编译..."

cargo clean
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release

echo "✅ 生产编译完成，无告警、无错误"
