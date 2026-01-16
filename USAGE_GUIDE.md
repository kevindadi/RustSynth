# SyPetype 使用指南

**版本**: 1.0  
**更新日期**: 2026-01-17

---

## 📖 概述

SyPetype 是一个从 Rust 库的 rustdoc JSON 文档构建**下推着色 Petri 网 (PCPN)** 的工具，用于分析 Rust API 的使用序列和借用规则。

---

## 🚀 快速开始

### 1. 准备输入文件

首先需要生成 rustdoc JSON 文档：

```bash
# 进入你的 Rust 项目目录
cd your-rust-project

# 生成 rustdoc JSON (需要 nightly Rust)
cargo +nightly rustdoc --lib -- -Zunstable-options --output-format json

# JSON 文件通常位于 target/doc/<crate_name>.json
```

### 2. 构建 SyPetype

```bash
cd sypetype
cargo build --release
```

### 3. 基本使用

```bash
# 生成 API Graph
./target/release/sypetype apigraph -i path/to/doc.json -o output/

# 生成 PCPN
./target/release/sypetype pcpn -i path/to/doc.json -o output/

# 同时生成两者
./target/release/sypetype all -i path/to/doc.json -o output/
```

---

## 📊 命令详解

### 1. `apigraph` - 生成 API Graph

**功能**: 从 rustdoc JSON 提取 API 信息，构建函数-类型二分图

**语法**:
```bash
sypetype apigraph [OPTIONS] -i <INPUT> -o <OUT>
```

**选项**:
- `-i, --input <FILE>`: rustdoc JSON 文件路径 (必需)
- `-o, --out <DIR>`: 输出目录 (默认: .)
- `--module <MODULE>`: 仅分析指定模块 (可多次指定)

**输出**:
- `apigraph.dot`: API Graph 的 DOT 格式图
- 控制台统计信息

**示例**:
```bash
# 分析整个 crate
sypetype apigraph -i target/doc/my_crate.json -o output/

# 只分析特定模块
sypetype apigraph -i target/doc/my_crate.json -o output/ --module my_module
```

**可视化**:
```bash
# 使用 Graphviz 渲染图
dot -Tpng output/apigraph.dot -o apigraph.png
dot -Tsvg output/apigraph.dot -o apigraph.svg
```

---

### 2. `pcpn` - 生成 PCPN

**功能**: 将 API Graph 转换为下推着色 Petri 网

**语法**:
```bash
sypetype pcpn [OPTIONS] -i <INPUT> -o <OUT>
```

**选项**:
- `-i, --input <FILE>`: rustdoc JSON 文件路径 (必需)
- `-o, --out <DIR>`: 输出目录 (默认: .)
- `--module <MODULE>`: 仅分析指定模块

**输出**:
- `pcpn.dot`: PCPN 的 DOT 格式图
- 控制台统计信息（Place 数、Transition 数等）

**示例**:
```bash
sypetype pcpn -i target/doc/my_crate.json -o output/
```

**可视化**:
```bash
dot -Tpng output/pcpn.dot -o pcpn.png
```

---

### 3. `all` - 同时生成 API Graph 和 PCPN

**功能**: 一次性生成 API Graph 和 PCPN

**语法**:
```bash
sypetype all [OPTIONS] -i <INPUT> -o <OUT>
```

**输出**:
- `apigraph.dot`
- `pcpn.dot`
- 控制台统计信息

---

### 4. `simulate` - 运行 PCPN 仿真器

**功能**: 搜索可行的 API 调用序列

**语法**:
```bash
sypetype simulate [OPTIONS] -i <INPUT>
```

**选项**:
- `-i, --input <FILE>`: rustdoc JSON 文件路径 (必需)
- `--max-steps <N>`: 最大步数 (默认: 50)
- `--min-steps <N>`: 最小步数 (默认: 3)
- `--strategy <STRATEGY>`: 搜索策略 (bfs/dfs，默认: bfs)
- `--module <MODULE>`: 仅分析指定模块

**输出**:
- 找到的可行序列
- 仿真统计信息

**示例**:
```bash
# 使用 BFS 搜索
sypetype simulate -i target/doc/my_crate.json --max-steps 20

# 使用 DFS 搜索
sypetype simulate -i target/doc/my_crate.json --strategy dfs --max-steps 30
```

---

### 5. `reachability` - 生成可达图

**功能**: 生成 PCPN 的可达图

**语法**:
```bash
sypetype reachability [OPTIONS] -i <INPUT> -o <OUT>
```

**选项**:
- `-i, --input <FILE>`: rustdoc JSON 文件路径 (必需)
- `-o, --out <DIR>`: 输出目录
- `--max-states <N>`: 最大状态数 (默认: 100)
- `--module <MODULE>`: 仅分析指定模块

**输出**:
- `reachability.dot`: 可达图的 DOT 格式
- 状态和转移统计

**示例**:
```bash
sypetype reachability -i target/doc/my_crate.json -o output/ --max-states 200
```

---

### 6. ✨ `enumerate` - 枚举所有可执行序列 (TODO)

**功能**: 枚举所有从初始标识开始的可执行函数链

**语法**:
```bash
sypetype enumerate [OPTIONS] -i <INPUT>
```

**选项**:
- `-i, --input <FILE>`: rustdoc JSON 文件路径 (必需)
- `--max-length <N>`: 最大序列长度 (默认: 10)
- `--only-api`: 只输出 API 调用，过滤结构性变迁 (默认: true)
- `--format <FORMAT>`: 输出格式 (text/json，默认: text)
- `-o, --output <FILE>`: 输出文件路径 (默认: 控制台)
- `--module <MODULE>`: 仅分析指定模块

**输出示例** (text 格式):
```
=== 可执行序列枚举 ===

总共找到 15 条序列

序列 1:
  1. Counter::new
  2. Counter::increment
  3. Counter::get

序列 2:
  1. Counter::new
  2. Counter::get
  3. Counter::increment

...
```

**示例**:
```bash
# 枚举所有序列（只显示 API 调用）
sypetype enumerate -i target/doc/my_crate.json --max-length 10

# 包含所有变迁（借用、归还等）
sypetype enumerate -i target/doc/my_crate.json --max-length 10 --only-api false

# 输出 JSON 格式
sypetype enumerate -i target/doc/my_crate.json --format json -o sequences.json
```

---

### 7. ✨ `stats` - 生成统计信息 (TODO)

**功能**: 生成详细的统计报告

**语法**:
```bash
sypetype stats [OPTIONS] -i <INPUT>
```

**选项**:
- `-i, --input <FILE>`: rustdoc JSON 文件路径 (必需)
- `--format <FORMAT>`: 输出格式 (table/json，默认: table)
- `-o, --output <FILE>`: 输出文件路径 (默认: 控制台)
- `--max-states <N>`: 最大状态数 (默认: 1000)
- `--module <MODULE>`: 仅分析指定模块

**输出示例** (table 格式):
```
=== 仿真统计信息 ===

状态空间:
  可达状态总数: 245
  死锁状态数:   3

序列统计:
  最长序列长度: 12
  平均序列长度: 6.5

操作统计:
  变迁总数:     428
  借用操作:     156
  归还操作:     152

API 调用统计:
  Counter::new: 42 次
  Counter::increment: 38 次
  Counter::get: 35 次
  Counter::reset: 28 次
```

**示例**:
```bash
# 生成表格格式统计
sypetype stats -i target/doc/my_crate.json

# 生成 JSON 格式
sypetype stats -i target/doc/my_crate.json --format json -o stats.json

# 限制状态空间大小
sypetype stats -i target/doc/my_crate.json --max-states 500
```

---

## 📐 PCPN 模型说明

### 三库所模型

SyPetype 使用三库所模型表示 Rust 的所有权和借用：

1. **Own Place (所有权库所)**
   - 存放类型 `T` 的所有权
   - 颜色: 浅蓝色

2. **Shr Place (共享引用库所)**
   - 存放类型 `&T` 的共享引用
   - 颜色: 浅青色

3. **Mut Place (可变借用库所)**
   - 存放类型 `&mut T` 的可变借用
   - 颜色: 浅玫瑰色

### 变迁类型

1. **ApiCall** (API 调用)
   - 调用 crate 中的函数或方法
   - 颜色: 淡绿色

2. **BorrowMut / BorrowShr** (借用)
   - 从 Own Place 创建借用
   - 颜色: 淡紫色

3. **EndBorrowMut / EndBorrowShr** (归还)
   - 归还借用，恢复所有权
   - 颜色: 蜜瓜色

4. **CreatePrimitive** (创建基本类型)
   - 创建 primitive 类型的值
   - 颜色: 浅青色
   - 形状: 菱形

5. **Drop** (销毁)
   - 销毁值
   - 颜色: 灰色

### Guard 保护条件

1. **RequireOwn**: 需要完全所有权（无借用存在）
2. **RequireShr**: 需要共享引用（无可变借用）
3. **RequireMut**: 需要可变借用（无其他借用）
4. **RequireNotBorrowed**: Token 不能被生命周期栈阻塞

---

## 🔧 高级选项

### 模块过滤

只分析特定模块：

```bash
sypetype all -i input.json -o output/ --module my_module --module sub_module
```

### 日志级别

设置日志级别：

```bash
# 详细日志
RUST_LOG=debug sypetype all -i input.json -o output/

# 只显示错误
RUST_LOG=error sypetype all -i input.json -o output/
```

---

## 📊 输出文件说明

### 1. apigraph.dot

API Graph 的 DOT 格式图，包含：
- 函数节点（方框）- 绿色
- 类型节点（椭圆）- 蓝色
- 边（箭头）- 黑色/蓝色/红色（对应 Own/Shr/Mut）

### 2. pcpn.dot

PCPN 的 DOT 格式图，包含：
- Place（圆形）- 按 capability 着色
- Transition（方框）- 按类型着色
- 弧（箭头）

### 3. reachability.dot

可达图，包含：
- 状态节点（方框）
- 转移边（标注变迁名称）

---

## 💡 典型工作流

### 工作流 1: 完整分析

```bash
#!/bin/bash

# 1. 生成 rustdoc JSON
cd your-project
cargo +nightly rustdoc --lib -- -Zunstable-options --output-format json

# 2. 分析并生成所有输出
cd ../sypetype
./target/release/sypetype all \
    -i ../your-project/target/doc/your_crate.json \
    -o output/

# 3. 生成可达图
./target/release/sypetype reachability \
    -i ../your-project/target/doc/your_crate.json \
    -o output/ \
    --max-states 200

# 4. 可视化
cd output/
dot -Tsvg apigraph.dot -o apigraph.svg
dot -Tsvg pcpn.dot -o pcpn.svg
dot -Tsvg reachability.dot -o reachability.svg
```

### 工作流 2: 序列分析 (TODO)

```bash
#!/bin/bash

# 1. 枚举所有序列
./target/release/sypetype enumerate \
    -i input.json \
    --max-length 15 \
    --only-api true \
    -o sequences.txt

# 2. 生成统计信息
./target/release/sypetype stats \
    -i input.json \
    --format table \
    -o stats.txt

# 3. 查看结果
cat sequences.txt
cat stats.txt
```

---

## 🐛 常见问题

### Q1: 编译时提示 "format version too old"

**A**: 确保使用最新的 nightly Rust:
```bash
rustup update nightly
cargo +nightly rustdoc --lib -- -Zunstable-options --output-format json
```

### Q2: 生成的图太大无法查看

**A**: 可以使用模块过滤：
```bash
sypetype all -i input.json -o output/ --module core_module
```

或限制状态数：
```bash
sypetype reachability -i input.json -o output/ --max-states 50
```

### Q3: 没有找到可执行序列

**A**: 尝试增加搜索步数：
```bash
sypetype simulate -i input.json --max-steps 100
```

或使用 DFS 策略：
```bash
sypetype simulate -i input.json --strategy dfs --max-steps 50
```

---

## 📚 参考资料

1. **Rust 文档**
   - [The Rust Book](https://doc.rust-lang.org/book/)
   - [rustdoc JSON format](https://doc.rust-lang.org/rustdoc/json.html)

2. **Petri 网理论**
   - [Petri Nets - Wikipedia](https://en.wikipedia.org/wiki/Petri_net)
   - Colored Petri Nets

3. **Graphviz**
   - [Graphviz 官网](https://graphviz.org/)
   - [DOT 语言指南](https://graphviz.org/doc/info/lang.html)

---

## 🤝 贡献

欢迎贡献代码和报告问题！

---

**维护者**: Claude Sonnet 4.5  
**版本**: 1.0  
**最后更新**: 2026-01-17
