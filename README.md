# SyPetype - 签名层协议可达性分析与见证代码生成器

[![Rust](https://img.shields.io/badge/rust-2021%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)

基于 Colored Petri Net (CPN) 和 Pushdown Colored Petri Net (PCPN) 理论的 Rust API 可达性搜索工具。

## 概述

SyPetype 从 rustdoc JSON 中提取公开 API 签名，构建一个资源状态机/着色网模型，执行有界可达性搜索，找到可行的 API 调用序列并生成对应的 Rust 代码片段（可通过 `cargo check` 验证）。

### 核心特性

- ✅ **类型规范化**：将所有类型归一化为全称路径 TypeKey
- ✅ **资源 Token 建模**：Token 携带 capability (own/shr/mut)、变量 ID、借用关系
- ✅ **借用约束追踪**：通过 OwnerStatus 实现 Rust 借用规则（共享/可变互斥）
- ✅ **API + 结构性变迁**：支持函数调用 + borrow/drop 等操作
- ✅ **参数自动适配**：own→shr (&)、own→mut (&mut)、mut→shr (&*)
- ✅ **状态规范化 (α-重命名)**：去重状态空间，提高搜索效率
- ✅ **可选 Pushdown 模式**：支持 LoanStack LIFO 借用栈
- ✅ **代码生成**：输出简洁可编译的 Rust 代码片段

## 快速开始

### 安装

```bash
git clone <repository>
cd SyPetype
cargo build --release
```

### 运行示例

```bash
# 1. 生成示例 crate 的 rustdoc JSON
cd examples/simple_counter
cargo +nightly rustdoc --lib -- -Z unstable-options --output-format json

# 2. 运行 SyPetype
cd ../..
cargo run --release -- \
    --input examples/simple_counter/target/doc/simple_counter.json \
    --verbose
```

### 预期输出

```rust
fn generated_witness() {
    // Step 0: call crate::Counter::new()
    let x0 = Counter::new();
    // Step 1: call crate::Counter::increment with args (&mut x0)
    Counter::increment(&mut x0);
    // Step 2: call crate::Counter::get with args (&x0)
    let x1 = Counter::get(&x0);
}
```

## 使用方法

### 基本用法

```bash
sypetype --input <rustdoc-json> [OPTIONS]
```

### 常用选项

| 选项 | 说明 | 默认值 |
|------|------|--------|
| `-i, --input <FILE>` | Rustdoc JSON 文件路径 | 必需 |
| `-o, --output <FILE>` | 输出代码文件路径 | stdout |
| `--max-steps <N>` | 最大搜索步数 | 20 |
| `--max-tokens-per-type <N>` | 每种类型最大 token 数 | 5 |
| `--max-borrow-depth <N>` | 最大借用嵌套深度 | 3 |
| `--enable-loan-stack` | 启用 LIFO 借用栈 | false |
| `--module <PATH>` | 仅探索指定模块 | 全部 |
| `--target-type <TYPE>` | 目标类型 | - |
| `--verify` | 验证生成的代码 | false |
| `-v, --verbose` | 输出详细 trace | false |

### 生成 Rustdoc JSON

```bash
# 对于库 crate
cargo +nightly rustdoc --lib -- -Z unstable-options --output-format json

# 对于二进制 crate
cargo +nightly rustdoc --bin <name> -- -Z unstable-options --output-format json

# JSON 文件位于: target/doc/<crate_name>.json
```

## 工作原理

### 1. 类型归一化

所有类型使用全称路径字符串作为 TypeKey：
- `std::vec::Vec` (忽略泛型参数)
- `crate::model::User`
- `bool`, `i32` 等 primitive

引用类型提取为 base type + capability：
- `&T` → TypeKey=T, cap=shr
- `&mut T` → TypeKey=T, cap=mut

### 2. Token 模型

每个 Token 携带：
- `cap`: own/shr/mut
- `id`: 变量 ID
- `ty`: TypeKey (base type)
- `origin`: 借用来源（仅 shr/mut）
- `is_copy`: 是否 Copy 类型

### 3. 借用规则

通过 `OwnerStatus` 追踪每个 owned token 的借用状态：
- `Free`: 无借用
- `ShrCount(n)`: n 个共享借用活跃
- `MutActive`: 一个可变借用活跃

规则：
- 可变借用互斥（MutActive 时禁止任何借用）
- 共享借用可多个但与可变互斥

### 4. 搜索算法

BFS 搜索状态空间：
- 初始状态：空
- 每步生成所有 enabled transitions
- 应用 transition 得到新状态
- 状态去重（通过 α-重命名）
- 目标：封闭状态（无未结束借用）+ 至少有 tokens

## 项目结构

```
src/
├── main.rs           # CLI 入口
├── model.rs          # 核心数据模型 (Token, State, OwnerStatus)
├── rustdoc_loader.rs # Rustdoc JSON 加载
├── type_norm.rs      # 类型归一化
├── api_extract.rs    # API 签名提取
├── transition.rs     # Transition 定义与应用
├── canonicalize.rs   # 状态规范化 (α-重命名)
├── search.rs         # 可达性搜索 (BFS)
└── emit.rs           # Rust 代码生成与验证

examples/
└── simple_counter/   # 示例 crate

docs/
├── USAGE.md          # 详细使用指南
└── ARCHITECTURE.md   # 架构设计文档
```

## 理论背景

本工具的理论基础源自：

1. **Colored Petri Net (CPN)**: Place/Transition 系统 + Token 着色
2. **Pushdown CPN**: 增加栈结构追踪 LIFO 借用
3. **Binding-Element Enabling**: 参数到 token 的绑定 + 使能条件检查
4. **α-Equivalence**: 状态规范化与去重

关键创新：
- 将 Rust 借用检查建模为 Place 中的 capability 约束
- 用 OwnerStatus 而非拆分 Place 实现借用互斥
- 参数适配策略（临时借用 vs 持久借用）

## 限制与未来工作

### 当前限制

1. **泛型不展开**：TypeKey 只保留 base path，不处理具体泛型实例化
2. **Copy trait 近似判断**：从 rustdoc 推断，可能不准确
3. **返回引用 origin 推断**：简化为使用第一个参数
4. **组合爆炸**：参数绑定枚举限制为前 3 个候选
5. **Unsafe 不支持**：不处理 unsafe 代码

### 未来增强

- [ ] 更精确的 trait 分析（Copy, Clone）
- [ ] 支持泛型单态化
- [ ] Reborrow 和 Deref 变迁
- [ ] 返回值生命周期分析
- [ ] 并行搜索
- [ ] 交互式调试模式
- [ ] 图形化可视化

## 文档

- [使用指南](USAGE.md) - 详细的使用说明和示例
- [架构设计](ARCHITECTURE.md) - 系统架构和算法详解

## 许可证

双许可：MIT / Apache-2.0

## 作者

资深 Rust 工具链工程师 + 形式化语义工程师

---

**注意**: 本工具用于研究和教学目的。生成的代码片段可能不符合最佳实践，仅用于展示 API 可达性。 - 签名层协议可达性分析与见证代码生成器

基于 Colored Petri Net (CPN) 和 Pushdown Colored Petri Net (PCPN) 理论的 Rust API 可达性搜索工具。

## 概述

SyPetype 从 rustdoc JSON 中提取公开 API 签名，构建一个资源状态机/着色网模型，执行有界可达性搜索，找到可行的调用轨迹并生成对应的 Rust 代码片段（可通过 `cargo check` 验证）。

### 核心特性

- **类型规范化**：将所有类型归一化为全称路径 TypeKey
- **资源 Token 建模**：Token 携带 capability (own/shr/mut)、变量 ID、借用关系
- **借用约束追踪**：通过 OwnerStatus 实现 Rust 借用规则（共享/可变互斥）
- **API + 结构性变迁**：支持函数调用 + borrow/drop/reborrow 等操作
- **参数自动适配**：own→shr (&)、own→mut (&mut)、mut→shr (&*) 
- **状态规范化 (α-重命名)**：去重状态空间，提高搜索效率
- **可选 Pushdown 模式**：支持 LoanStack LIFO 借用栈
- **代码生成**：输出简洁可编译的 Rust 代码片段

## 安装

### 前置要求

- Rust nightly (用于生成 rustdoc JSON)
- Cargo

### 构建

```bash
git clone <repository>
cd SyPetype
cargo build --release
```

## 使用方法

### 1. 生成目标 crate 的 rustdoc JSON

首先需要为你想分析的 crate 生成 rustdoc JSON：

```bash
# 进入目标 crate 目录
cd /path/to/your/crate

# 生成 rustdoc JSON (需要 nightly)
cargo +nightly rustdoc --lib -- -Z unstable-options --output-format json

# JSON 文件会生成在 target/doc/<crate_name>.json
```

### 2. 运行 SyPetype

```bash
# 基本用法
sypetype --input target/doc/your_crate.json

# 指定最大步数和 token 数量
sypetype --input target/doc/your_crate.json --max-steps 30 --max-tokens-per-type 10

# 仅探索特定模块
sypetype --input target/doc/your_crate.json --module crate::model

# 尝试合成指定类型
sypetype --input target/doc/your_crate.json --target-type "crate::User"

# 启用详细输出（显示内部 trace）
sypetype --input target/doc/your_crate.json --verbose

# 验证生成的代码（运行 cargo check）
sypetype --input target/doc/your_crate.json --verify

# 输出到文件
sypetype --input target/doc/your_crate.json --output witness.rs
```

### 3. 命令行参数

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `-i, --input` | Rustdoc JSON 文件路径（必需） | - |
| `-o, --output` | 输出代码文件路径（可选） | stdout |
| `--max-steps` | 最大搜索步数 | 20 |
| `--max-tokens-per-type` | 每种类型最大 token 数 | 5 |
| `--max-borrow-depth` | 最大借用嵌套深度 | 3 |
| `--enable-loan-stack` | 启用 LIFO 借用栈 (pushdown) | false |
| `--module` | 仅探索指定模块（可多次指定） | 全部 |
| `--target-type` | 目标类型（尝试合成此类型） | - |
| `--verify` | 在临时 crate 中验证代码 | false |
| `-v, --verbose` | 输出详细 trace | false |

## 示例

### 示例 1: 分析简单的 crate

假设有以下简单 crate:

```rust
// lib.rs
pub struct Counter {
    value: i32,
}

impl Counter {
    pub fn new() -> Self {
        Counter { value: 0 }
    }
    
    pub fn increment(&mut self) {
        self.value += 1;
    }
    
    pub fn get(&self) -> i32 {
        self.value
    }
}
```

运行 SyPetype:

```bash
cargo +nightly rustdoc --lib -- -Z unstable-options --output-format json
sypetype --input target/doc/counter.json --verbose
```

可能生成的代码：

```rust
fn generated_witness() {
    // Step 0: call crate::Counter::new()
    let x0 = Counter::new();
    // Step 1: call crate::Counter::increment with args (&mut x0)
    Counter::increment(&mut x0);
    // Step 2: call crate::Counter::get with args (&x0)
    let x1 = Counter::get(&x0);
    // Step 3: drop x0
    drop(x0);
}
```

## 工作原理

### 1. 类型归一化

所有类型使用全称路径字符串作为 TypeKey：
- `std::vec::Vec` (忽略泛型参数)
- `crate::model::User`
- `bool`, `i32` 等 primitive

引用类型提取为 base type + capability：
- `&T` → TypeKey=T, cap=shr
- `&mut T` → TypeKey=T, cap=mut

### 2. Token 模型

每个 Token 携带：
- `cap`: own/shr/mut
- `id`: 变量 ID
- `ty`: TypeKey (base type)
- `origin`: 借用来源（仅 shr/mut）
- `is_copy`: 是否 Copy 类型

### 3. 借用规则

通过 `OwnerStatus` 追踪每个 owned token 的借用状态：
- `Free`: 无借用
- `ShrCount(n)`: n 个共享借用活跃
- `MutActive`: 一个可变借用活跃

规则：
- 可变借用互斥（MutActive 时禁止任何借用）
- 共享借用可多个但与可变互斥

### 4. Transition

**API Transition**: 
- 参数匹配（ByValue/SharedRef/MutRef）
- 自动适配（own→&, own→&mut, mut→&*）
- 返回值产生

**Structural Transition**:
- `DropOwned`: drop 非借用的 owned token
- `BorrowShr/BorrowMut`: 创建引用
- `EndBorrow`: 结束借用

### 5. 搜索

BFS 搜索状态空间：
- 初始状态：空
- 每步生成所有 enabled transitions
- 应用 transition 得到新状态
- 状态去重（通过 α-重命名）
- 目标：封闭状态（无未结束借用）+ 至少有 tokens

### 6. 代码生成

从 trace 生成 Rust 代码：
- 变量命名：x0, x1, ... (owned), r0, r1, ... (refs)
- 不写类型注解（让编译器推导）
- 不写生命周期（依赖 NLL）
- 使用 scope block 控制借用生命周期

## 项目结构

```
src/
├── main.rs           # CLI 入口
├── model.rs          # 核心数据模型 (Token, State, OwnerStatus)
├── rustdoc_loader.rs # Rustdoc JSON 加载
├── type_norm.rs      # 类型归一化
├── api_extract.rs    # API 签名提取
├── transition.rs     # Transition 定义与应用
├── canonicalize.rs   # 状态规范化 (α-重命名)
├── search.rs         # 可达性搜索 (BFS)
└── emit.rs           # Rust 代码生成与验证
```

## 限制与未来工作

### 当前限制

1. **泛型不展开**：TypeKey 只保留 base path，不处理具体泛型实例化
2. **Copy trait 近似判断**：从 rustdoc 推断，可能不准确
3. **返回引用 origin 推断**：简化为使用第一个参数
4. **组合爆炸**：参数绑定枚举限制为前 3 个候选
5. **Unsafe 不支持**：不处理 unsafe 代码

### 未来增强

- [ ] 更精确的 trait 分析（Copy, Clone）
- [ ] 支持泛型单态化
- [ ] Reborrow 和 Deref 变迁
- [ ] 返回值生命周期分析
- [ ] 并行搜索
- [ ] 交互式调试模式
- [ ] 图形化可视化




