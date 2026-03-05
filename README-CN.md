# RustSynth - 基于 Pushdown CPN 的 Safe Rust 合成器

[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)

RustSynth 是一个基于 **Pushdown Colored Petri Net (Pushdown CPN，下推着色 Petri 网)** 的 Safe Rust 代码片段合成器。它从 rustdoc JSON 文档中解析公开 API 签名，构建 9-place 所有权模型的 PCPN，通过有界可达性搜索合成可编译的 Safe Rust 代码。

LLM 大语言模型在辅助程序开发和生成测试用例方面效果显著，但可能无法同时保证**编译正确性**和**覆盖率**。RustSynth 通过使用形式化的 Petri 网语义来建模 Rust 的所有权和生命周期系统，确保生成的每一段代码在借用检查规则层面都是结构有效的。

## 理论基础

### 下推着色 Petri 网 (PCPN)

PCPN 在经典着色 Petri 网 (CPN) 基础上扩展了一个下推栈，使其能够建模嵌套的借用作用域。形式化定义：

**PCPN = (P, T, A, C, G, I, S)** 其中：

- **P** (库所/Places)：表示带类型的 token 容器。每个基础类型 T 通过笛卡尔积 `{T, &T, &mut T} × {own, frz, blk}` 生成 9 个库所。
- **T** (变迁/Transitions)：表示 API 调用和结构操作（借用、释放、复制）。
- **A** (弧/Arcs)：连接库所和变迁，可携带可选的弧表达式（inscription）进行 token 变换。
- **C** (颜色集/Color sets)：token 颜色包含变量 ID (`vid`)、类型信息、区域标签和借用来源。
- **G** (守卫/Guards)：变迁使能的布尔条件（如 `NoFrzNoBlk`、`PlaceCountRange`、`StackDepthMax`）。
- **I** (弧表达式/Inscriptions)：定义 token 变换的可选弧表达式。
- **S** (栈/Stack)：LIFO 下推栈，通过 `Freeze`、`Shr`、`Mut` 帧追踪未完成的借用。

### 9-Place 所有权模型

对于每个基础类型 `T`，模型区分 9 个库所：

```
             own        frz        blk
  T       [T,own]    [T,frz]    [T,blk]
  &T      [&T,own]   [&T,frz]   [&T,blk]
  &mut T  [&mut T,own] [&mut T,frz] [&mut T,blk]
```

- **own** (拥有)：token 被完全拥有，可以移动或借用
- **frz** (冻结)：所有者因存在未完成的共享借用而被冻结
- **blk** (阻塞)：所有者因存在未完成的可变借用而被阻塞

### Token 流语义

关键的结构变迁建模了 Rust 的借用检查器规则：

| 变迁 | 输入 | 输出 | 栈效应 | 守卫 |
|---|---|---|---|---|
| `borrow_shr_first(T)` | `[T,own]` | `[T,frz] + [&T,own]` | push Freeze, push Shr | NoFrzNoBlk |
| `borrow_shr_next(T)` | (读取 `[T,frz]`) | `[&T,own]` | push Shr | NoBlk |
| `end_shr_keep_frz(T)` | `[T,frz] + [&T,own]` | `[T,frz]` | pop Shr | StackTopMatches |
| `end_shr_unfreeze(T)` | `[T,frz] + [&T,own]` | `[T,own]` | pop Shr, pop Freeze | StackTopMatches |
| `borrow_mut(T)` | `[T,own]` | `[T,blk] + [&mut T,own]` | push Mut | NoFrzNoOtherBlk |
| `end_mut(T)` | `[T,blk] + [&mut T,own]` | `[T,own]` | pop Mut | StackTopMatches |
| `drop(T)` | `[T,own]` | (空) | - | NotBlocked |

## 核心特性

- **9-Place 模型**：对每个基础类型 T，区分 `{T, &T, &mut T} × {own, frz, blk}` = 9 个库所
- **Pushdown 栈**：记录未完成的借用，实现 LIFO 借用/归还语义
- **多搜索策略**：BFS（最短路径）、DFS（低内存）、IDDFS（有界内存的最优深度搜索）
- **多 Trace 生成**：单次搜索中收集多条 witness trace，提升测试覆盖率
- **类型统一**：支持泛型函数的 unification 和 completion
- **规范化**：状态空间 canonicalization（vid/region 重命名）避免无限状态爆炸
- **生命周期省略**：实现 Rust 的 3 条生命周期省略规则，自动推断生命周期绑定
- **扩展守卫**：`PlaceCountRange`（库所 token 数量约束）、`StackDepthMax`（栈深度约束）、组合 `And` 守卫
- **弧表达式**：可选的 token 变换表达式（Identity、Project、Wrap、Filter）
- **0-ary Producer**：自动识别无参数的 const fn 作为值源
- **TOML 配置**：灵活的任务规范，支持目标、边界、过滤器和策略选择
- **代码生成**：将 witness firing 序列转换为可编译的 Rust 代码，支持 `use` 导入

## 快速开始

### 前置条件

- Rust 工具链 (1.85+)
- Rust nightly（用于生成 rustdoc JSON）

### 安装

```bash
git clone https://github.com/example/RustSynth.git
cd RustSynth
cargo build --release
```

### 基本用法

```bash
# 1. 为目标 crate 生成 rustdoc JSON
cd examples/toy_api
cargo +nightly rustdoc -Z unstable-options --output-format json --lib

# 2. 运行合成器
cd ../..
cargo run --release -- synth \
    --doc-json examples/toy_api/target/doc/toy_api.json \
    --task examples/toy_api/task.toml \
    --out synthesized.rs

# 3. 验证生成的代码能编译
rustc --edition 2021 synthesized.rs --crate-type lib
```

### 一键测试

```bash
python3 run_tests.py
```

或使用 Docker：

```bash
docker build -t RustSynth .
docker run --rm RustSynth
```

## 架构

```
rustdoc JSON
     |
     v
+-----------------+
|  extract.rs     |  rustdoc JSON -> 二分 API 图（函数 <-> 类型）
+-----------------+
     |
     v
+-----------------+
|  pcpn.rs        |  API 图 -> 9-Place PCPN 模型（单态化 + 结构变迁）
+-----------------+
     |
     v
+-----------------+
|  simulator.rs   |  PCPN -> 有界可达性搜索（BFS/DFS/IDDFS + 规范化）
+-----------------+
     |
     v
+-----------------+
|  emitter.rs     |  Witness Trace -> 可编译 Safe Rust 代码（单/多 trace 模式）
+-----------------+
```

### 数据流

1. **提取** (`extract.rs`)：解析 rustdoc JSON `Crate`，提取公开函数签名（参数、返回类型、self 接收者、生命周期绑定），构建 `FunctionNode` <-> `TypeNode` 的二分图 `ApiGraph`。

2. **PCPN 构建** (`pcpn.rs`)：将 `ApiGraph` 转换为下推着色 Petri 网。执行泛型单态化，为每个类型创建 9 个库所，生成带守卫的 API 变迁，添加结构变迁（借用/释放/复制）。

3. **仿真** (`simulator.rs`)：在 PCPN 上执行有界可达性搜索。支持 BFS（找最短 witness）、DFS（低内存）和 IDDFS（有界内存的最优深度搜索）。使用状态规范化（vid/region 重命名）去重已访问状态。支持多 trace 收集。

4. **代码发射** (`emitter.rs`)：将 witness firing 序列转换为可编译的 Rust 代码。处理变量命名、类型标注、方法调用语法、借用表达式和 drop 插入。

## 任务配置

任务配置使用 TOML 格式：

```toml
[inputs]
doc_json = "target/doc/my_crate.json"

[search]
stack_depth = 8           # 最大借用栈深度
default_place_bound = 2   # 每个库所的默认 token 上界
max_steps = 100           # 最大搜索步数
strategy = "bfs"          # 搜索策略："bfs"、"dfs" 或 "iddfs"
max_traces = 1            # 要收集的 witness trace 数量

[search.place_bounds]
"own_i32" = 3             # 覆盖特定库所的上界

[filter]
allow = ["Counter::new", "Counter::inc", "Counter::get"]

[goal]
want = "own i32"          # 目标：获得一个 owned i32
count = 1
```

### 搜索策略

| 策略 | 说明 | 适用场景 |
|------|------|----------|
| `bfs` | 广度优先搜索（默认） | 找最短 witness trace |
| `dfs` | 深度优先搜索 | 低内存消耗，探索深层路径 |
| `iddfs` | 迭代加深深度优先搜索 | 有界内存的最优深度搜索 |

### 多 Trace 模式

设置 `max_traces > 1` 可收集多条不同的 witness trace。每条 trace 代表一条不同的、能达到目标的合法 API 调用序列。代码发射器会为每条 trace 生成独立的测试函数。

## 命令列表

| 命令           | 说明                           |
| -------------- | ------------------------------ |
| `synth`        | 使用任务配置运行完整合成流水线 |
| `apigraph`     | 生成 API Graph (DOT/JSON)      |
| `pcpn`         | 生成 PCPN 模型 (DOT/JSON)      |
| `simulate`     | 运行仿真器搜索 witness（支持 `--strategy`） |
| `reachability` | 生成可达图                     |
| `generate`     | 完整流水线：PCPN -> 仿真 -> 代码  |

## 模块概览

| 模块           | 行数 | 说明                                                    |
| -------------- | ---- | ------------------------------------------------------- |
| `types.rs`     | 617  | 9-place 类型定义 (TypeForm, Capability, Token, Marking, BorrowStack) |
| `type_model.rs`| 491  | 内部类型表示 (TypeKey, PassingMode)                     |
| `config.rs`    | 331  | TOML 任务配置解析，支持策略选择和多 trace 模式          |
| `unify.rs`     | 395  | 类型统一和补全，用于泛型实例化                          |
| `pcpn.rs`      | 1165 | Pushdown CPN 模型构建，扩展守卫和弧表达式              |
| `simulator.rs` | 1436 | BFS/DFS/IDDFS 可达性搜索 + 规范化 + 多 trace 收集      |
| `emitter.rs`   | 635  | Witness 到 Rust 代码转换（单/多 trace 模式）            |
| `extract.rs`   | 719  | rustdoc JSON -> API Graph 提取                          |
| `apigraph.rs`  | 649  | API 二分图 (FunctionNode, TypeNode, ApiEdge)            |
| `lifetime_analyzer.rs` | 555 | 生命周期分析，实现 Rust 省略规则              |
| `rustdoc_loader.rs` | 25 | rustdoc JSON 文件加载                                 |
| `main.rs`      | 529  | CLI 入口，支持多个子命令                                |

## 示例输出

```rust
//! Generated by RustSynth PCPN Synthesizer

fn main() {
    let mut counter_0: Counter = Counter::new();
    let ref_counter_1 = &counter_0;
    let mut i32_2: i32 = ref_counter_1.get();
    drop(ref_counter_1);
}
```

### 多 Trace 模式输出

```rust
//! Generated by RustSynth PCPN Synthesizer
//!
//! 3 test functions generated from 3 witness traces.

use Counter;

fn test_0() {
    let mut counter_0: Counter = Counter::new();
    let mut i32_1: i32 = counter_0.into_value();
}

fn test_1() {
    let mut counter_0: Counter = Counter::new();
    let ref_counter_1 = &counter_0;
    let mut i32_2: i32 = ref_counter_1.get();
    drop(ref_counter_1);
}

fn test_2() {
    let mut counter_0: Counter = Counter::new();
    let mut_counter_1 = &mut counter_0;
    mut_counter_1.inc();
    drop(mut_counter_1);
    let mut i32_2: i32 = counter_0.into_value();
}

fn main() {
    test_0();
    test_1();
    test_2();
}
```

## 支持的 Rust 结构

- 基本类型 (i32, u64, bool 等)
- 用户定义的结构体
- 带 bounds 的泛型 (Copy, Clone)
- 共享引用 (&T)
- 可变引用 (&mut T)
- 方法 (self, &self, &mut self)
- 自由函数
- const fn（作为 0-ary producer）
- 生命周期省略（3 条规则）

## 限制

- 不支持关联类型 / trait impl 分析
- 不支持 async/await
- 通过栈顺序简化 outlives 检查
- 不支持高阶类型和 GATs

## 许可证

可选择以下任一许可证：

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

## 参考

- Pushdown Colored Petri Net 理论
- Rust 所有权和借用语义
- rustdoc JSON 格式规范
- Rust Reference: 生命周期省略规则
