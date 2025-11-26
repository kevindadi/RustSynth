# SyPetype - Rustdoc Petri Net Builder

基于 Colored Petri Nets (CPN) 的 Rust API Fuzzing 工具。

## 概述

SyPetype 将 Rust 项目的 API 依赖关系解析为图结构，并可转换为着色 Petri 网（CPN），用于智能 Fuzzing 测试。

## 架构设计

项目采用模块化设计，职责清晰分离：

### 模块结构

```
src/
├── parse/           # 解析模块：处理 rustdoc JSON 输出
│   └── mod.rs      # 提取类型、函数、Trait 实现关系
│
├── api_graph/      # API 图模块：构建依赖图
│   ├── structure.rs # 图数据结构（Node, Edge, ApiGraph）
│   ├── builder.rs   # 图构建器
│   ├── export.rs    # 导出功能（JSON, DOT）
│   └── mod.rs      # 模块入口
│
├── cpn/            # CPN 模块：转换为着色 Petri 网（待实现）
│   └── mod.rs
│
└── main.rs         # 主程序入口
```

### 核心概念

#### 1. Parse 模块
负责解析 `cargo +nightly rustdoc -- --output-format json` 生成的 JSON 文件：

- **ParsedCrate**: 解析后的完整 Crate 信息
- **FunctionInfo**: 函数签名和泛型约束
- **TypeInfo**: 类型定义（Struct, Enum, Trait 等）
- **TraitMap**: Trait 实现关系映射

#### 2. API Graph 模块
将解析后的数据构建为依赖图：

- **Node**: 图节点（具体类型或 Trait）
  - `ConcreteType`: Struct, Enum 等具体类型
  - `AbstractTrait`: Trait 抽象类型

- **Edge**: 图的边（函数/方法）
  - 表示从输入类型到输出类型的转换
  - 携带泛型约束信息

- **ApiGraph**: 完整的 API 依赖图
  - 提供查询、遍历功能
  - 支持泛型约束验证

#### 3. 关键特性：Trait 解析

**TraitMap** 是核心数据结构，映射 `TraitId -> Vec<ImplementorId>`：

```rust
// 示例：如果代码中有
trait Val {}
struct User {}
impl Val for User {}
fn process(v: impl Val) -> u32 { ... }

// TraitMap 会记录：
// Val -> [User]
// 
// 图构建时：
// User --[process]--> u32
```

这使得工具能够解析 `impl Trait` 和泛型约束，确定哪些具体类型可以满足函数的泛型要求。

## 使用方法

### 1. 生成 rustdoc JSON

首先为目标项目生成 rustdoc JSON：

```bash
cd /path/to/target/project
cargo +nightly rustdoc -- --output-format json
```

这会在 `target/doc/` 目录下生成 JSON 文件（通常是 `<crate_name>.json`）。

### 2. 运行 SyPetype

```bash
cd /path/to/SyPetype
cargo run -- /path/to/target/project/target/doc/<crate_name>.json
```

或使用项目自带的示例：

```bash
cargo run -- ./base64.json
```

### 3. 查看输出

程序会生成两个文件：

1. **api_graph.json**: 用于 CPN 生成的 JSON 格式
   ```json
   {
     "places": [...],         // 所有类型节点
     "transitions": [...],    // 所有函数边
     "trait_implementations": [...]  // Trait 实现关系
   }
   ```

2. **api_graph.dot**: 用于 Graphviz 可视化的 DOT 格式
   ```bash
   # 生成可视化图像
   dot -Tpng api_graph.dot -o api_graph.png
   ```

### 示例输出

```
正在加载 rustdoc JSON: ./base64.json
✓ 成功解析 rustdoc JSON

=== Rustdoc 解析统计 ===
总 Item 数: 1234
函数数: 56
类型数: 78
Trait 实现数: 23
  - Struct: 45
  - Enum: 12
  - Trait: 21

正在构建 API 依赖图...
✓ 图构建完成

=== API Dependency Graph 统计 ===
节点数: 78
边数: 142
  - 具体类型: 57
  - Trait: 21
  - 有泛型约束的边: 18

✓ JSON 已导出到: api_graph.json
✓ DOT 已导出到: api_graph.dot
  可使用以下命令生成可视化图像:
  dot -Tpng api_graph.dot -o api_graph.png

✓ 所有步骤完成!
```

## API 说明

### Parse 模块

```rust
use sypetype::parse::ParsedCrate;

// 从 JSON 文件加载
let parsed = ParsedCrate::from_json_file("path/to/doc.json")?;

// 查询类型名称
let name = parsed.get_type_name(&type_id);

// 查询 Trait 实现者
let implementors = parsed.get_trait_implementors(&trait_id);

// 访问函数列表
for func in &parsed.functions {
    println!("函数: {}", func.name);
}
```

### API Graph 模块

```rust
use sypetype::api_graph::{build_graph, ExportFormat};

// 构建图
let graph = build_graph(parsed_crate);

// 查询节点
let outgoing = graph.outgoing_edges(&node);
let incoming = graph.incoming_edges(&node);

// 验证约束
let satisfies = graph.satisfies_constraint(type_id, &constraint);

// 导出
let json = graph.export(ExportFormat::Json);
let dot = graph.export(ExportFormat::Dot);
```

## 开发计划

- [x] Parse 模块：rustdoc JSON 解析
- [x] API Graph 模块：依赖图构建
- [x] TraitMap：Trait 实现关系提取
- [x] 导出功能：JSON 和 DOT 格式
- [ ] CPN 模块：转换为着色 Petri 网
- [ ] Fuzzing 引擎：基于 CPN 的智能测试生成
- [ ] 泛型约束完整支持：处理复杂的生命周期和 where 子句

## 技术栈

- **rustdoc-types 0.57**: rustdoc JSON 格式的类型定义
- **serde/serde_json**: JSON 序列化/反序列化
- **petgraph**: 图数据结构（可选，当前使用自定义实现）

## License

Apache-2.0 OR MIT

## 作者

Kevin Zhang <zhangkaiwenyy@gmail.com>

