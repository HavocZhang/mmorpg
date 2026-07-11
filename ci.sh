#!/usr/bin/env bash
# ===========================================================
#  Rust MMO Gateway CI 流水线
#  用途: 本地 + CI 环境一键质量检查
#  用法: bash ci.sh [--full] [--bench] [--audit]
#
#  --full:  含 BDD + 长稳压测（需要 Docker/Redis）
#  --bench: 含 cargo bench（耗时长）
#  --audit: 含 cargo audit 安全扫描
# ===========================================================
set -euo pipefail

cd "$(dirname "$0")"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

pass=0
fail=0

check() {
    local desc="$1" cmd="$2"
    printf "${YELLOW}[CHECK]${NC} %-50s " "$desc"
    if eval "$cmd" > /dev/null 2>&1; then
        echo -e "${GREEN}PASS${NC}"
        ((pass++))
    else
        echo -e "${RED}FAIL${NC}"
        ((fail++))
    fi
}

echo "============================================================"
echo "  Rust MMO Gateway CI Pipeline"
echo "  $(date '+%Y-%m-%d %H:%M:%S')"
echo "============================================================"
echo ""

# ── 编译检查 ──
echo "── 编译检查 ──"
check "cargo check (lib + bins)"          "cargo check"
check "cargo check (tests)"               "cargo check --tests"
check "cargo build --release"             "cargo build --release"
echo ""

# ── 整洁检查 ──
echo "── 整洁检查 ──"
check "cargo fmt --check"                 "cargo fmt --check"
check "cargo clippy (lib + bins)"         "cargo clippy --lib --bins -- -D warnings"
check "cargo clippy (tests)"              "cargo clippy --tests -- -D warnings"
echo ""

# ── 单元测试 ──
echo "── 单元测试 ──"
check "cargo test --lib"                  "cargo test --lib"

for t in tdd_protocol tdd_crypto tdd_config tdd_session tdd_network \
         tdd_io_engine tdd_security tdd_cluster tdd_admin tdd_concurrent \
         tdd_exception tdd_fuzz tdd_scene tdd_chat tdd_combat; do
    check "  test: $t"                    "cargo test --test $t"
done
echo ""

# ── 安全审计（可选） ──
if [[ "${1:-}" == "--audit" ]]; then
    echo "── 安全审计 ──"
    if command -v cargo-audit &>/dev/null || cargo install cargo-audit --quiet 2>/dev/null; then
        check "cargo audit"               "cargo audit --quiet"
    else
        echo "  (cargo-audit not available, skipping)"
    fi
    echo ""
fi

# ── 性能基准（可选） ──
if [[ "${1:-}" == "--bench" ]]; then
    echo "── 性能基准 ──"
    check "cargo bench --no-run"          "cargo bench --no-run"
    echo ""
fi

# ── BDD + 集成测试（可选） ──
if [[ "${1:-}" == "--full" ]]; then
    echo "── BDD 场景测试 ──"
    check "cargo test --test bdd"          "cargo test --test bdd"
    echo ""
fi

# ── 结果 ──
echo "============================================================"
printf "  Total: %d  |  " $((pass + fail))
echo -e "${GREEN}PASS $pass${NC}  |  ${RED}FAIL $fail${NC}"
echo "============================================================"

if [ $fail -gt 0 ]; then
    exit 1
fi
