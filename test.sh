#!/bin/bash
# SyPetype 测试脚本 - 简化版 PCPN Simulator
# 输出抽象 Trace（类型 + 所有权/借用）

set -e

# 颜色
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

step() { echo -e "${BLUE}▶ $1${NC}"; }
success() { echo -e "${GREEN}✓ $1${NC}"; }

echo "============================================"
echo "SyPetype - 简化版 PCPN Simulator 测试"
echo "============================================"
echo ""

# 编译
step "1. 编译 SyPetype"
cargo build --release 2>&1 | tail -3
success "编译完成"
echo ""

SYPETYPE="./target/release/sypetype"

# 测试 simple_counter
step "2. 测试 simple_counter"
cd examples/simple_counter
cargo +nightly rustdoc -- -Z unstable-options --output-format json 2>&1 | tail -1
cd ../..

mkdir -p test_output/simple_counter
$SYPETYPE generate \
    --input examples/simple_counter/target/doc/simple_counter.json \
    --out test_output/simple_counter \
    --max-steps 12 --min-steps 6 --max-tokens 2 2>&1

echo ""
echo "=== simple_counter Trace ==="
cat test_output/simple_counter/trace.txt
echo ""

# 测试 type_flow
step "3. 测试 type_flow"
cd examples/type_flow
cargo +nightly rustdoc -- -Z unstable-options --output-format json 2>&1 | tail -1
cd ../..

mkdir -p test_output/type_flow
$SYPETYPE generate \
    --input examples/type_flow/target/doc/type_flow.json \
    --out test_output/type_flow \
    --max-steps 15 --min-steps 5 --max-tokens 2 2>&1

echo ""
echo "=== type_flow Trace ==="
cat test_output/type_flow/trace.txt
echo ""

# 生成可达图
step "4. 生成可达图"
$SYPETYPE reachability \
    --input examples/simple_counter/target/doc/simple_counter.json \
    --out test_output/simple_counter \
    --max-states 30 --max-tokens 2 2>&1

# 生成图片（如果有 graphviz）
if command -v dot &> /dev/null; then
    step "5. 生成图片"
    dot -Tpng test_output/simple_counter/pcpn.dot -o test_output/simple_counter/pcpn.png
    dot -Tpng test_output/simple_counter/reachability.dot -o test_output/simple_counter/reachability.png
    success "图片已生成"
fi

echo ""
echo "============================================"
echo "测试完成！"
echo ""
echo "输出文件:"
echo "  - test_output/simple_counter/trace.txt (抽象 Trace)"
echo "  - test_output/simple_counter/pcpn.dot (PCPN)"
echo "  - test_output/simple_counter/reachability.dot (可达图)"
echo "============================================"
