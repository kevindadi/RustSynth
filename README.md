# SyPetype - Rust API 签名分析与 PCPN 构建工具

[![Rust](https://img.shields.io/badge/rust-nightly-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)

SyPetype 是一个从 Rust crate 的 rustdoc JSON 提取 API 签名，构建二分 API Graph，转换为下推着色 Petri 网 (PCPN)，并生成可编译 Rust 代码的工具。

## 目录

- [功能特性](#功能特性)
- [安装指南](#安装指南)
- [快速开始](#快速开始)
- [使用说明](#使用说明)
- [PCPN 模型说明](#pcpn-模型说明)
- [示例](#示例)
- [技术说明](#技术说明)
- [限制与简化](#限制与简化)
- [许可证](#许可证)

## 功能特性

- **API Graph 构建**：从 rustdoc JSON 提取函数签名，构建类型-函数二分图
- **PCPN 转换**：将 API Graph 转换为下推着色 Petri 网，完整建模 Rust 所有权和借用语义
- **有界仿真**：BFS/DFS 状态空间探索，找到满足约束的 witness 轨迹
- **代码生成**：从 firing 序列生成可编译的 Rust 代码
- **可视化**：输出 DOT 格式，可用 Graphviz 生成图片

## 安装指南

### 安装 Rust

SyPetype 需要 Rust nightly 工具链（用于生成 rustdoc JSON）。

#### macOS / Linux

```bash
# 安装 rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 按照提示完成安装，然后重新加载环境
source ~/.cargo/env

# 安装 nightly 工具链
rustup install nightly

# 验证安装
rustc --version
cargo --version
```

#### Windows

1. 下载并运行 [rustup-init.exe](https://win.rustup.rs/)
2. 按照安装向导完成安装
3. 打开新的命令行窗口，执行：

```powershell
rustup install nightly
rustc --version
cargo --version
```

### 安装 Graphviz (可选)

Graphviz 用于将 DOT 文件转换为图片。

```bash
# macOS
brew install graphviz

# Ubuntu / Debian
sudo apt-get install graphviz

# Windows: 从 https://graphviz.org/download/ 下载
```

### 编译 SyPetype

```bash
git clone <repository-url>
cd SyPetype
cargo build --release
```

## 快速开始

```bash
# 1. 进入示例目录，生成 rustdoc JSON
cd examples/simple_counter
cargo +nightly rustdoc -- -Z unstable-options --output-format json
cd ../..

# 2. 运行完整流水线（生成 PCPN + 仿真 + 代码生成）
cargo run --release -- generate \
  --input examples/simple_counter/target/doc/simple_counter.json \
  --out output/

# 3. (可选) 生成可视化图片
dot -Tpng output/pcpn.dot -o output/pcpn.png
```

## 使用说明

### 生成 rustdoc JSON

```bash
cd your-crate
cargo +nightly rustdoc -- -Z unstable-options --output-format json
```

生成的 JSON 文件位于 `target/doc/<crate_name>.json`。

### 命令列表

| 命令 | 说明 |
|------|------|
| `apigraph` | 构建 API Graph（二分图） |
| `pcpn` | 构建 PCPN（下推着色 Petri 网） |
| `all` | 同时生成 API Graph 和 PCPN |
| `simulate` | 运行 PCPN 仿真器 |
| `generate` | 完整流水线：PCPN → 仿真 → 代码生成 |

### generate 命令（推荐）

```bash
sypetype generate --input <rustdoc.json> --out <output-dir> [OPTIONS]
```

**选项**：

| 选项 | 默认值 | 说明 |
|------|--------|------|
| `--max-tokens` | 3 | 每个 place 的最大 token 数 |
| `--max-stack` | 5 | 借用栈的最大深度 |
| `--max-steps` | 50 | 最大探索步数 |
| `--min-steps` | 5 | 最小步数（目标条件） |
| `--strategy` | bfs | 搜索策略：`bfs` 或 `dfs` |

**输出文件**：
- `pcpn.dot` - PCPN 的 Graphviz DOT 格式
- `apigraph.dot` - API Graph 的 DOT 格式
- `generated.rs` - 生成的可编译 Rust 代码

## PCPN 模型说明

### 类型宇宙（已单态化）

```
Ty ::= T | RefShr(T) | RefMut(T)
```

### Places（库所）

每个基础类型 T 有三个库所：

| Place | 含义 |
|-------|------|
| `Own(T)` | 拥有 T 的所有权 |
| `Frz(T)` | T 被冻结（有活跃的共享借用） |
| `Blk(T)` | T 被阻塞（有活跃的可变借用） |

引用类型有单独的 Own 库所：
- `Own(RefShr(T))` - 拥有 `&T` 引用
- `Own(RefMut(T))` - 拥有 `&mut T` 引用

### Token 结构

```rust
Token {
    vid: u32,           // 变量 ID
    bind_mut: bool,     // 是否 let mut
    region: Option<u32> // None: owned, Some(L): 引用的 region
}
```

### 函数参数绑定规则（关键）

| Rust 参数类型 | 连接到的 Place |
|--------------|---------------|
| `T` | `Own(T)` |
| `&T` | `Own(RefShr(T))` |
| `&mut T` | `Own(RefMut(T))` |

**注意**：函数参数直接连接到正确的引用库所，而不是先连到 `Own(T)` 再借用。

### 结构性变迁

| 变迁 | 效果 | 说明 |
|------|------|------|
| `BorrowShrFirst` | `Own(T) → Frz(T) + Own(&T)` | 首次共享借用 |
| `BorrowShrNext` | `Frz(T) → Frz(T) + Own(&T)` | 后续共享借用 |
| `EndShrKeepFrz` | `Frz(T) + Own(&T) → Frz(T)` | 结束借用，保持冻结 |
| `EndShrUnfreeze` | `Frz(T) + Own(&T) → Own(T)` | 结束最后一个借用，解冻 |
| `BorrowMut` | `Own(T) → Blk(T) + Own(&mut T)` | 可变借用（需要 bind_mut） |
| `EndMut` | `Blk(T) + Own(&mut T) → Own(T)` | 结束可变借用 |
| `MakeMutByMove` | `Own(T, mut=false) → Own(T, mut=true)` | `let mut y = x;` |
| `Drop` | `Own(T) → ε` | drop 值 |

### & / &mut 冲突的自然抑制

通过库所设计自动保证借用规则：

1. **共享借用时**：owner 在 `Frz(T)`，`BorrowMut` 的前条件 `Own(T)` 不满足 → 自动禁止可变借用
2. **可变借用时**：owner 在 `Blk(T)`，任何 borrow 的前条件都不满足 → 自动禁止其他借用

## 示例

### simple_counter

```rust
pub struct Counter { value: i32 }

impl Counter {
    pub fn new() -> Self { ... }
    pub fn get(&self) -> i32 { ... }
    pub fn increment(&mut self) { ... }
}
```

运行后生成的 Rust 代码示例：

```rust
fn main() {
    let v0 = Counter::new();
    let v1 = Counter::new();
    let mut v0 = v0;  // MakeMutByMove
    let r2 = &mut v0; // BorrowMut
    Counter::increment(r2);
    drop(r2);         // EndMut
}
```

### generic_example

包含泛型结构体、关联类型、Trait bounds 等复杂场景的测试。

## 技术说明

### 项目结构

```
SyPetype/
├── src/
│   ├── main.rs          # CLI 入口
│   ├── apigraph.rs      # API Graph 数据结构
│   ├── extract.rs       # rustdoc JSON 解析
│   ├── pcpn.rs          # PCPN 数据结构和转换
│   ├── simulator.rs     # 有界仿真器
│   ├── emitter.rs       # Rust 代码生成
│   ├── type_model.rs    # 类型表示
│   └── rustdoc_loader.rs# rustdoc JSON 加载
├── examples/
│   ├── simple_counter/  # 简单示例
│   └── generic_example/ # 泛型示例
├── Cargo.toml
└── README.md
```

### 工具流水线

```
rustdoc JSON → API-Graph → PCPN → Simulator → Firing Sequence → Rust Code
     ↓            ↓           ↓        ↓              ↓              ↓
 签名提取     二分图构建   Petri网   状态搜索      witness轨迹    可编译代码
```

## 限制与简化

本工具是工程化原型，有以下简化：

1. **不支持**：`unsafe`、`dyn Trait`、HRTB
2. **生命周期**：仅考虑函数签名中的 `&T`/`&mut T`，不考虑复杂 outlives
3. **借用规则**：LIFO + 显式 drop（比 NLL 保守）
4. **泛型**：在 API-Graph 阶段完成单态化，PCPN 中不出现类型变量

## 许可证

双许可：MIT 或 Apache-2.0，任选其一。

- [MIT License](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)
