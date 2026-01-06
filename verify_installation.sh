#!/bin/bash
# SyPetype 安装验证脚本

set -e

echo "==================================="
echo "SyPetype 安装验证"
echo "==================================="
echo ""

# 颜色定义
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 检查函数
check() {
    if [ $? -eq 0 ]; then
        echo -e "${GREEN}✓${NC} $1"
        return 0
    else
        echo -e "${RED}✗${NC} $1"
        return 1
    fi
}

warn() {
    echo -e "${YELLOW}⚠${NC} $1"
}

# 1. 检查 Rust 工具链
echo "1. 检查 Rust 工具链..."
rustc --version > /dev/null 2>&1
check "Rust stable 已安装"

rustup toolchain list | grep -q nightly
if [ $? -eq 0 ]; then
    check "Rust nightly 已安装"
else
    warn "Rust nightly 未安装（生成 rustdoc JSON 需要）"
    echo "  安装命令: rustup install nightly"
fi
echo ""

# 2. 检查项目结构
echo "2. 检查项目结构..."
[ -f "Cargo.toml" ]
check "Cargo.toml 存在"

[ -f "src/main.rs" ]
check "src/main.rs 存在"

[ -d "examples/simple_counter" ]
check "示例 crate 存在"

[ -f "README.md" ]
check "README.md 存在"

[ -f "USAGE.md" ]
check "USAGE.md 存在"

[ -f "ARCHITECTURE.md" ]
check "ARCHITECTURE.md 存在"
echo ""

# 3. 编译项目
echo "3. 编译项目..."
cargo build --release > /dev/null 2>&1
check "Release 构建成功"

[ -f "target/release/sypetype" ]
check "可执行文件已生成"
echo ""

# 4. 运行测试
echo "4. 运行测试..."
cargo test > /dev/null 2>&1
check "所有测试通过"
echo ""

# 5. 检查示例
echo "5. 检查示例 crate..."
cd examples/simple_counter

# 检查是否已生成 JSON
if [ ! -f "target/doc/simple_counter.json" ]; then
    warn "示例 JSON 未生成，正在生成..."
    cargo +nightly rustdoc --lib -- -Z unstable-options --output-format json > /dev/null 2>&1
    if [ $? -eq 0 ]; then
        check "示例 JSON 生成成功"
    else
        warn "示例 JSON 生成失败（需要 nightly）"
    fi
else
    check "示例 JSON 已存在"
fi

cd ../..
echo ""

# 6. 运行示例
echo "6. 运行示例分析..."
if [ -f "examples/simple_counter/target/doc/simple_counter.json" ]; then
    OUTPUT=$(./target/release/sypetype \
        --input examples/simple_counter/target/doc/simple_counter.json \
        --max-steps 10 2>&1)
    
    if [ $? -eq 0 ]; then
        check "示例分析成功"
        
        # 检查输出是否包含代码
        if echo "$OUTPUT" | grep -q "fn generated_witness"; then
            check "生成了代码片段"
        else
            warn "未生成代码片段（可能未找到可行轨迹）"
        fi
    else
        warn "示例分析失败"
        echo "$OUTPUT"
    fi
else
    warn "跳过示例分析（JSON 未生成）"
fi
echo ""

# 7. 检查文档
echo "7. 检查文档完整性..."
grep -q "签名层协议可达性" README.md
check "README.md 包含中文描述"

grep -q "快速开始" USAGE.md
check "USAGE.md 包含使用指南"

grep -q "架构设计" ARCHITECTURE.md
check "ARCHITECTURE.md 包含架构说明"
echo ""

# 8. 统计信息
echo "8. 项目统计..."
echo "  Rust 源文件数: $(find src -name "*.rs" | wc -l | tr -d ' ')"
echo "  Markdown 文档数: $(find . -maxdepth 1 -name "*.md" | wc -l | tr -d ' ')"
echo "  示例数量: $(find examples -name "Cargo.toml" | wc -l | tr -d ' ')"
echo ""

# 9. 总结
echo "==================================="
echo "验证完成！"
echo "==================================="
echo ""
echo "下一步："
echo "  1. 阅读 README.md 了解项目概述"
echo "  2. 阅读 USAGE.md 学习使用方法"
echo "  3. 运行 ./quickstart.sh 快速体验"
echo "  4. 分析你自己的 crate"
echo ""
echo "命令示例："
echo "  # 快速开始"
echo "  ./quickstart.sh"
echo ""
echo "  # 分析自己的 crate"
echo "  cd /path/to/your/crate"
echo "  cargo +nightly rustdoc --lib -- -Z unstable-options --output-format json"
echo "  /path/to/sypetype/target/release/sypetype --input target/doc/your_crate.json --verbose"
echo ""

