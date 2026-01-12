#!/bin/bash
# SyPetype 测试脚本
# 运行所有示例并验证输出

set -e  # 遇错退出

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 计数器
PASS=0
FAIL=0

# 打印分隔线
separator() {
    echo ""
    echo "============================================================"
    echo ""
}

# 打印步骤
step() {
    echo -e "${BLUE}▶ $1${NC}"
}

# 打印成功
success() {
    echo -e "${GREEN}✓ $1${NC}"
    ((PASS++))
}

# 打印失败
fail() {
    echo -e "${RED}✗ $1${NC}"
    ((FAIL++))
}

# 打印警告
warn() {
    echo -e "${YELLOW}⚠ $1${NC}"
}

# 检查命令是否存在
check_command() {
    if command -v "$1" &> /dev/null; then
        success "$1 已安装: $(command -v $1)"
        return 0
    else
        fail "$1 未安装"
        return 1
    fi
}

separator
echo -e "${BLUE}SyPetype 测试脚本 - PCPN 工程化实现${NC}"
echo "开始时间: $(date)"
separator

# ========== 1. 环境检查 ==========
step "1. 检查环境"

check_command "cargo" || { echo "请先安装 Rust"; exit 1; }
check_command "rustc" || { echo "请先安装 Rust"; exit 1; }

# 检查 nightly
if rustup run nightly rustc --version &> /dev/null; then
    success "Rust nightly 已安装"
else
    fail "Rust nightly 未安装，请运行: rustup install nightly"
    exit 1
fi

# Graphviz 可选
if check_command "dot"; then
    HAS_GRAPHVIZ=1
else
    warn "Graphviz 未安装，跳过图片生成"
    HAS_GRAPHVIZ=0
fi

separator

# ========== 2. 编译项目 ==========
step "2. 编译 SyPetype"

if cargo build --release 2>&1 | tail -5; then
    success "编译成功"
else
    fail "编译失败"
    exit 1
fi

SYPETYPE="./target/release/sypetype"

separator

# ========== 3. 测试 simple_counter 示例 ==========
step "3. 测试 simple_counter 示例"

EXAMPLE_DIR="examples/simple_counter"
OUTPUT_DIR="test_output/simple_counter"
mkdir -p "$OUTPUT_DIR"

# 生成 rustdoc JSON
step "3.1 生成 rustdoc JSON"
cd "$EXAMPLE_DIR"
if cargo +nightly rustdoc -- -Z unstable-options --output-format json 2>&1 | tail -3; then
    success "rustdoc JSON 生成成功"
else
    fail "rustdoc JSON 生成失败"
    cd ../..
    exit 1
fi
cd ../..

JSON_FILE="$EXAMPLE_DIR/target/doc/simple_counter.json"
if [ -f "$JSON_FILE" ]; then
    success "JSON 文件存在: $JSON_FILE"
else
    fail "JSON 文件不存在"
    exit 1
fi

# 运行完整流水线：generate 命令
step "3.2 运行完整流水线 (generate)"
echo ""
if $SYPETYPE generate --input "$JSON_FILE" --out "$OUTPUT_DIR" --max-steps 30 --min-steps 5 2>&1; then
    success "generate 命令执行成功"
else
    fail "generate 命令执行失败"
fi

# 检查输出文件
if [ -f "$OUTPUT_DIR/pcpn.dot" ] && [ -f "$OUTPUT_DIR/apigraph.dot" ]; then
    success "DOT 文件已生成"
else
    fail "DOT 文件缺失"
fi

# 检查生成的 Rust 代码
if [ -f "$OUTPUT_DIR/generated.rs" ]; then
    success "Rust 代码已生成"
    echo ""
    echo "=== simple_counter 生成的 Rust 代码 ==="
    cat "$OUTPUT_DIR/generated.rs"
    echo ""
else
    warn "未生成 Rust 代码（可能未找到 witness）"
fi

# 生成图片（如果有 Graphviz）
if [ "$HAS_GRAPHVIZ" -eq 1 ]; then
    step "3.3 生成图片"
    if dot -Tpng "$OUTPUT_DIR/apigraph.dot" -o "$OUTPUT_DIR/apigraph.png" && \
       dot -Tpng "$OUTPUT_DIR/pcpn.dot" -o "$OUTPUT_DIR/pcpn.png"; then
        success "图片生成成功"
        echo "  - $OUTPUT_DIR/apigraph.png"
        echo "  - $OUTPUT_DIR/pcpn.png"
    else
        fail "图片生成失败"
    fi
fi

separator

# ========== 4. 测试 generic_example 示例 ==========
step "4. 测试 generic_example 示例"

EXAMPLE_DIR="examples/generic_example"
OUTPUT_DIR="test_output/generic_example"
mkdir -p "$OUTPUT_DIR"

# 生成 rustdoc JSON
step "4.1 生成 rustdoc JSON"
cd "$EXAMPLE_DIR"
if cargo +nightly rustdoc -- -Z unstable-options --output-format json 2>&1 | tail -3; then
    success "rustdoc JSON 生成成功"
else
    fail "rustdoc JSON 生成失败"
    cd ../..
    exit 1
fi
cd ../..

JSON_FILE="$EXAMPLE_DIR/target/doc/generic_example.json"
if [ -f "$JSON_FILE" ]; then
    success "JSON 文件存在: $JSON_FILE"
else
    fail "JSON 文件不存在"
    exit 1
fi

# 运行完整流水线：generate 命令
step "4.2 运行完整流水线 (generate)"
echo ""
if $SYPETYPE generate --input "$JSON_FILE" --out "$OUTPUT_DIR" --max-steps 30 --min-steps 5 2>&1; then
    success "generate 命令执行成功"
else
    fail "generate 命令执行失败"
fi

# 检查输出文件
if [ -f "$OUTPUT_DIR/pcpn.dot" ] && [ -f "$OUTPUT_DIR/apigraph.dot" ]; then
    success "DOT 文件已生成"
else
    fail "DOT 文件缺失"
fi

# 检查生成的 Rust 代码
if [ -f "$OUTPUT_DIR/generated.rs" ]; then
    success "Rust 代码已生成"
    echo ""
    echo "=== generic_example 生成的 Rust 代码 ==="
    cat "$OUTPUT_DIR/generated.rs"
    echo ""
else
    warn "未生成 Rust 代码（可能未找到 witness）"
fi

# 生成图片（如果有 Graphviz）
if [ "$HAS_GRAPHVIZ" -eq 1 ]; then
    step "4.3 生成图片"
    if dot -Tpng "$OUTPUT_DIR/apigraph.dot" -o "$OUTPUT_DIR/apigraph.png" && \
       dot -Tpng "$OUTPUT_DIR/pcpn.dot" -o "$OUTPUT_DIR/pcpn.png"; then
        success "图片生成成功"
        echo "  - $OUTPUT_DIR/apigraph.png"
        echo "  - $OUTPUT_DIR/pcpn.png"
    else
        fail "图片生成失败"
    fi
fi

separator

# ========== 5. PCPN 结构说明 ==========
step "5. PCPN 结构说明"
echo ""
echo "PCPN (Pushdown Colored Petri Net) 核心设计:"
echo ""
echo "  Places (库所):"
echo "    - Own(T):      拥有 T 的所有权"
echo "    - Frz(T):      T 被冻结（有活跃的共享借用）"
echo "    - Blk(T):      T 被阻塞（有活跃的可变借用）"
echo "    - Own(&T):     拥有 &T 引用"
echo "    - Own(&mut T): 拥有 &mut T 引用"
echo ""
echo "  Token:"
echo "    - vid:      变量 ID"
echo "    - bind_mut: 是否 let mut"
echo "    - region:   引用的 region ID"
echo ""
echo "  结构性变迁:"
echo "    - BorrowShrFirst: Own(T) → Frz(T) + Own(&T)"
echo "    - BorrowShrNext:  Frz(T) → Frz(T) + Own(&T)"
echo "    - EndShr:         Frz(T) + Own(&T) → Own(T) / Frz(T)"
echo "    - BorrowMut:      Own(T) → Blk(T) + Own(&mut T)"
echo "    - EndMut:         Blk(T) + Own(&mut T) → Own(T)"
echo "    - MakeMutByMove:  let mut y = x; (非 mut → mut)"
echo ""

separator

# ========== 6. 显示 PCPN DOT 预览 ==========
step "6. PCPN DOT 预览 (simple_counter)"
echo ""
head -50 test_output/simple_counter/pcpn.dot
echo "..."

separator

# ========== 7. 显示 generic_example PCPN 预览 ==========
step "7. PCPN DOT 预览 (generic_example)"
echo ""
head -50 test_output/generic_example/pcpn.dot 2>/dev/null || echo "(文件不存在)"
echo "..."

separator

# ========== 测试结果汇总 ==========
echo -e "${BLUE}测试结果汇总${NC}"
echo ""
echo -e "通过: ${GREEN}$PASS${NC}"
echo -e "失败: ${RED}$FAIL${NC}"
echo ""

if [ "$FAIL" -eq 0 ]; then
    echo -e "${GREEN}========================================${NC}"
    echo -e "${GREEN}  所有测试通过！ 🎉${NC}"
    echo -e "${GREEN}========================================${NC}"
    exit 0
else
    echo -e "${RED}========================================${NC}"
    echo -e "${RED}  有 $FAIL 个测试失败${NC}"
    echo -e "${RED}========================================${NC}"
    exit 1
fi
