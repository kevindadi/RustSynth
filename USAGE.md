# SyPetype 使用指南

## 快速开始

### 1. 构建工具

```bash
cd /Volumes/Samsung990/CodeSynthesis/SyPetype
cargo build --release
```

### 2. 准备测试 crate

我们提供了一个简单的计数器示例：

```bash
cd examples/simple_counter

# 生成 rustdoc JSON (需要 nightly)
cargo +nightly rustdoc --lib -- -Z unstable-options --output-format json

# JSON 文件位于: target/doc/simple_counter.json
```

### 3. 运行 SyPetype

```bash
# 回到项目根目录
cd ../..

# 运行分析
cargo run --release -- \
    --input examples/simple_counter/target/doc/simple_counter.json \
    --verbose \
    --max-steps 15

# 或使用已构建的二进制
./target/release/sypetype \
    --input examples/simple_counter/target/doc/simple_counter.json \
    --verbose
```

### 4. 预期输出

工具会输出类似以下的代码片段：

```rust
fn generated_witness() {
    // Step 0: call simple_counter::Counter::new()
    let x0 = Counter::new();
    // Step 1: call simple_counter::Counter::increment with args (&mut x0)
    Counter::increment(&mut x0);
    // Step 2: call simple_counter::Counter::get with args (&x0)
    let x1 = Counter::get(&x0);
    // Step 3: drop x0
    drop(x0);
}
```

## 高级用法

### 指定目标类型

尝试合成特定类型的 owned token：

```bash
cargo run --release -- \
    --input examples/simple_counter/target/doc/simple_counter.json \
    --target-type "crate::Counter" \
    --verbose
```

### 调整搜索参数

```bash
cargo run --release -- \
    --input examples/simple_counter/target/doc/simple_counter.json \
    --max-steps 30 \
    --max-tokens-per-type 10 \
    --max-borrow-depth 5
```

### 启用 Pushdown 模式（LIFO 借用栈）

```bash
cargo run --release -- \
    --input examples/simple_counter/target/doc/simple_counter.json \
    --enable-loan-stack \
    --verbose
```

### 模块过滤

仅探索特定模块：

```bash
cargo run --release -- \
    --input examples/simple_counter/target/doc/simple_counter.json \
    --module crate::Counter \
    --verbose
```

### 验证生成的代码

```bash
cargo run --release -- \
    --input examples/simple_counter/target/doc/simple_counter.json \
    --verify
```

注意：`--verify` 需要目标 crate 在本地可访问。

### 输出到文件

```bash
cargo run --release -- \
    --input examples/simple_counter/target/doc/simple_counter.json \
    --output witness.rs \
    --verbose
```

## 分析你自己的 crate

### 步骤 1: 生成 rustdoc JSON

```bash
cd /path/to/your/crate

# 确保使用 nightly
rustup default nightly

# 生成 JSON
cargo rustdoc --lib -- -Z unstable-options --output-format json

# 或者对于二进制 crate
cargo rustdoc --bin your_bin_name -- -Z unstable-options --output-format json
```

### 步骤 2: 运行 SyPetype

```bash
sypetype --input /path/to/your/crate/target/doc/your_crate.json --verbose
```

## 理解输出

### Trace 格式

当使用 `--verbose` 时，工具会输出详细的执行轨迹：

```
Step 0: call crate::Counter::new()
Step 1: call crate::Counter::increment with args (&mut v0)
Step 2: call crate::Counter::get with args (&v0)
Step 3: drop v0
```

每一步显示：
- **API 调用**：函数全路径 + 参数（包括适配策略）
- **结构性操作**：borrow、drop、reborrow 等

### 参数适配标记

- `&x` - owned → shared ref (临时借用)
- `&mut x` - owned → mutable ref (临时可变借用)
- `&*r` - mutable ref → shared ref (重借用)
- 无标记 - 直接使用

### 代码生成策略

生成的代码：
- 变量命名：`x0, x1, ...` (owned), `r0, r1, ...` (refs)
- 不写类型注解（让编译器推导）
- 不写生命周期（依赖 NLL）
- 使用 `drop()` 显式结束生命周期

## 故障排除

### 问题 1: "未找到任何公开 API"

**原因**：rustdoc JSON 可能不包含公开 API，或模块过滤太严格。

**解决**：
- 检查 crate 是否有 `pub` 函数/方法
- 移除 `--module` 过滤
- 检查 JSON 文件是否正确生成

### 问题 2: "未找到可行轨迹"

**原因**：搜索空间受限，或 API 签名不兼容。

**解决**：
- 增加 `--max-steps`（例如 50）
- 增加 `--max-tokens-per-type`（例如 10）
- 增加 `--max-borrow-depth`（例如 5）
- 检查是否有无参数的构造函数（如 `new()`）

### 问题 3: rustdoc JSON 生成失败

**原因**：需要 nightly Rust。

**解决**：
```bash
rustup install nightly
cargo +nightly rustdoc --lib -- -Z unstable-options --output-format json
```

### 问题 4: 编译警告或错误

**原因**：rustdoc-types 版本不匹配。

**解决**：
- 确保使用最新的 nightly
- 更新 `rustdoc-types` 依赖版本

## 性能调优

### 减少搜索时间

1. **限制步数**：`--max-steps 10`
2. **限制 token 数**：`--max-tokens-per-type 3`
3. **模块过滤**：`--module crate::specific_module`

### 增加覆盖率

1. **增加步数**：`--max-steps 50`
2. **增加 token 数**：`--max-tokens-per-type 10`
3. **启用 loan stack**：`--enable-loan-stack`

## 示例场景

### 场景 1: 探索新 crate 的 API

```bash
# 快速探索，找到基本调用序列
sypetype --input target/doc/new_crate.json --max-steps 10
```

### 场景 2: 深度分析复杂 API

```bash
# 深度搜索，生成复杂调用轨迹
sypetype --input target/doc/complex_crate.json \
    --max-steps 50 \
    --max-tokens-per-type 10 \
    --enable-loan-stack \
    --verbose
```

### 场景 3: 测试特定模块

```bash
# 只分析特定模块
sypetype --input target/doc/my_crate.json \
    --module crate::database \
    --module crate::models \
    --verbose
```

### 场景 4: 合成特定类型

```bash
# 尝试合成 Builder 类型
sypetype --input target/doc/builder_crate.json \
    --target-type "crate::Builder" \
    --max-steps 30
```

## 环境变量

设置日志级别：

```bash
# 详细日志
RUST_LOG=debug sypetype --input ...

# 仅错误
RUST_LOG=error sypetype --input ...

# 特定模块
RUST_LOG=sypetype::search=debug sypetype --input ...
```

## 已知限制

1. **泛型不展开**：`Vec<T>` 和 `Vec<U>` 被视为同一类型
2. **Copy trait 近似**：可能误判某些类型
3. **返回引用 origin**：简化为使用第一个参数
4. **组合爆炸**：参数绑定限制为前 3 个候选
5. **Unsafe 不支持**：不处理 unsafe 代码

## 贡献与反馈

欢迎提交 issue 和 PR！

---

**提示**：首次使用建议从简单的示例 crate 开始（如 `examples/simple_counter`），熟悉工具行为后再分析复杂项目。

