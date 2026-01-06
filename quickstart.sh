#!/bin/bash
# SyPetype 快速开始脚本

set -e

echo "==================================="
echo "SyPetype 快速开始"
echo "==================================="
echo ""

# 检查 nightly
if ! rustup toolchain list | grep -q nightly; then
    echo "❌ 未安装 Rust nightly"
    echo "正在安装..."
    rustup install nightly
fi

# 构建 SyPetype
echo "📦 构建 SyPetype..."
cargo build --release
echo "✅ 构建完成"
echo ""

# 构建示例 crate
echo "📦 构建示例 crate (simple_counter)..."
cd examples/simple_counter

# 生成 rustdoc JSON
echo "📄 生成 rustdoc JSON..."
cargo +nightly rustdoc --lib -- -Z unstable-options --output-format json 2>/dev/null || {
    echo "❌ 生成 rustdoc JSON 失败"
    exit 1
}

JSON_PATH="target/doc/simple_counter.json"
if [ ! -f "$JSON_PATH" ]; then
    echo "❌ 未找到 JSON 文件: $JSON_PATH"
    exit 1
fi

echo "✅ JSON 文件已生成: $JSON_PATH"
echo ""

# 返回项目根目录
cd ../..

# 运行 SyPetype
echo "🚀 运行 SyPetype..."
echo ""
echo "==================================="
echo "输出结果:"
echo "==================================="
echo ""

./target/release/sypetype \
    --input examples/simple_counter/target/doc/simple_counter.json \
    --verbose \
    --max-steps 15

echo ""
echo "==================================="
echo "✅ 完成！"
echo "==================================="
echo ""
echo "下一步："
echo "  1. 查看 USAGE.md 了解更多用法"
echo "  2. 查看 ARCHITECTURE.md 了解设计原理"
echo "  3. 尝试分析你自己的 crate"
echo ""
echo "示例命令："
echo "  # 分析你的 crate"
echo "  cargo +nightly rustdoc --lib -- -Z unstable-options --output-format json"
echo "  ./target/release/sypetype --input target/doc/your_crate.json --verbose"
echo ""

