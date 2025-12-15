# SyPetype 项目流程与实现细节 Prompt

## 项目概述

SyPetype 是一个将 Rust 项目的 API 依赖关系解析为图结构,并转换为着色 Petri 网(Labeled Petri Net)的工具,用于智能 Fuzzing 测试.项目采用模块化设计,将 rustdoc JSON 输出转换为中间表示(IR Graph),再转换为 Petri 网.

## 核心架构

### 模块结构

```
src/
├── main.rs              # 主程序入口,使用 Pipeline 执行工作流
├── config.rs            # 命令行配置解析(使用 clap)
├── pipeline.rs          # 工作流管道,协调各阶段执行
├── parse/               # 解析模块：处理 rustdoc JSON 输出
│   └── mod.rs          # 提取类型、函数、Trait 实现关系
├── ir_graph/            # IR Graph 模块：构建中间表示图
│   ├── mod.rs          # 模块入口
│   ├── structure.rs    # 图数据结构(Node, Edge, IrGraph)
│   ├── builder.rs      # 图构建器(核心逻辑)
│   ├── method_builder.rs # 方法节点构建
│   ├── type_cache.rs   # 类型缓存系统
│   ├── generic_scope.rs # 泛型作用域管理
│   ├── node_info.rs    # 节点详细信息定义
│   └── utils.rs        # 工具函数
├── label_pt_net/        # Labeled Petri Net 模块
│   ├── mod.rs          # 模块入口
│   ├── net.rs          # Petri 网数据结构与转换逻辑
│   ├── export.rs       # 导出功能(DOT, JSON, PNML)
│   ├── analysis.rs     # Petri 网分析(API 序列生成等)
│   └── shims.rs        # 基本类型 shim 处理
├── support_types/       # 支持类型定义
│   ├── primitives.rs   # 基本类型定义
│   ├── trait_blacklist.rs # Trait 黑名单
│   ├── method_blacklist.rs # 方法黑名单
│   └── impl_filter.rs  # 实现过滤
└── petri_net_traits.rs # Petri 网通用 Trait 定义
```

## 完整工作流程

### 阶段 1: 解析 rustdoc JSON (parse 模块)

**输入**: rustdoc JSON 文件(通过 `cargo +nightly rustdoc -- --output-format json` 生成)

**处理过程**:
1. 使用 `rustdoc-types` crate 解析 JSON 为 `Crate` 对象
2. 提取并分类所有 Item:
   - **类型集合**: Struct, Enum, Union, TypeAlias
   - **Trait 集合**: 所有定义的 Trait
   - **函数集合**: 顶层函数(排除 impl/trait 中的方法)
   - **常量/静态变量**: Constant, Static
   - **USE 重导出**: 解析 pub use 链
   - **Impl 块**: 收集所有 impl 块

3. 构建 Trait 实现映射:
   - `trait_impls: HashMap<Id, Vec<Id>>` - 类型 ID → 实现的 Trait ID 列表
   - 用于后续解析 `impl Trait` 和泛型约束

4. 预解析 USE 链:
   - `use_resolutions: HashMap<Id, Id>` - USE ID → 最终定义 ID
   - 解决 pub use 重导出,避免重复节点

**输出**: `ParsedCrate` 结构,包含原始 `Crate` 数据和预处理信息

### 阶段 2: 构建 IR Graph (ir_graph 模块)

**输入**: `ParsedCrate`

**处理过程** (按 `IrGraphBuilder::build()` 顺序):

#### 2.1 处理类型节点及其字段/变体 (`build_types()`)

- **Struct**: 
  - 创建类型节点,记录字段信息
  - 处理三种结构: Unit, Tuple, Plain
  - 为每个字段创建类型节点并建立 `Ref` 边

- **Enum**:
  - 创建枚举节点
  - 处理三种变体: Plain, Tuple, Struct
  - 为每个变体创建节点,建立 `Move` 和 `Ref` 双向边

- **Union**: 类似 Struct,但字段共享内存

- **字段处理** (`build_type_fields()`):
  - 遍历所有 `StructField` 和 `Variant`
  - 使用 `TypeCache` 统一管理类型节点,避免重复

#### 2.2 处理 Trait 节点 (`build_traits_nodes()`)

- 为每个 Trait 创建节点
- 创建 `TraitInfo` 记录关联类型、方法、泛型等
- 处理 Trait 的泛型参数(见 2.3)

#### 2.3 处理 Trait 的 Associated Type (`build_trait_assoc_types()`)

- 为每个 Associated Type 创建 `Trait.AssocType` 节点
- 建立 `Include` 边: Trait → Trait.AssocType
- 如果有默认类型,建立 `Alias` 边: Trait.AssocType → TargetType

#### 2.4 处理类型和 Trait 的泛型参数 (`build_type_generics()`)

- 为每个泛型参数创建节点(格式: `TypeName:GenericName`)
- 使用 `TypeCache` 的 `GenericScope` 区分不同作用域的泛型
- 处理 Trait 约束:
  - 创建 `Require` 边: Trait → 泛型参数
  - 支持带具体类型参数的 Trait(如 `AsRef<[u8]>`)

- **归一化 Trait 方法泛型** (`normalize_trait_method_generics()`):
  - 如果多个方法有同名泛型且约束相同,合并为一个节点
  - 使用 `Trait:GenericName` 作为归一化名称

#### 2.5 构建 Trait 定义的方法 (`build_trait_defined_methods()`)

- 为 Trait 中定义的每个方法创建 `TraitMethod` 节点
- 处理方法的输入参数和返回值
- 识别 `self` 参数并连接到 Trait 节点
- 处理方法的泛型参数(优先使用归一化的 Trait 级别泛型)

#### 2.6 展开 impl 块为方法 ID (`expand_impl_blocks()`)

- 将 `struct_data.impls` 中的 impl 块 ID 展开为方法 ID
- 对于 trait impl,创建 `Implements` 边: 类型 → Trait
- 处理 impl 块中的 Associated Type 重新定义

#### 2.7 构建类型实现的方法节点 (`build_impl_methods()`)

- 为每个类型实现的方法创建 `ImplMethod` 节点
- 处理方法的输入参数(识别 `self` 并连接到类型)
- 处理返回值(包括 Result/Option 展开)
- 过滤黑名单方法,但记录对应的 Trait

#### 2.8 处理 Constant 和 Static (`build_constants_and_statics()`)

- 为每个 Constant/Static 创建节点
- 解析类型并建立 `Instance` 边: Constant/Static → Type
- 解析初始值作为初始 token 数量(用于 Petri 网)

#### 2.9 后处理: Primitive 到 Trait 的 Implements 边 (`postprocess_primitive_trait_edges()`)

- 为基本类型(如 `u8`, `i32` 等)添加到默认 Trait 的 `Implements` 边
- 使用 `support_types::primitives::get_primitive_default_traits()` 获取默认 Trait

#### 2.10 后处理: Generic 约束检查与 Instance 边 (`postprocess_generic_constraints()`)

- 检查哪些 Primitive 类型满足泛型参数的 Trait 约束
- 为满足约束的 Primitive 添加 `Instance` 边: Primitive → Generic

**输出**: `IrGraph` 结构,包含:
- `type_graph: DiGraph<String, TypeRelation>` - 类型依赖图
- `node_types: HashMap<NodeIndex, NodeType>` - 节点类型映射
- `node_infos: HashMap<NodeIndex, NodeInfo>` - 节点详细信息

### 阶段 3: 转换为 Labeled Petri Net (label_pt_net 模块)

**输入**: `IrGraph`

**转换规则** (`LabeledPetriNet::from_ir_graph()`):

1. **节点分类**:
   - **数据节点 → Place**: Struct, Enum, Union, Constant, Static, Primitive, Tuple, Variant, Generic, TypeAlias, Trait
   - **操作节点 → Transition**: ImplMethod, TraitMethod, Function, UnwrapOp

2. **边转换**:
   - **Place → Transition**: 输入弧(消耗资源)
   - **Transition → Place**: 输出弧(产生资源)
   - **Place → Place**: 创建虚拟 transition(用于字段访问、类型包含等)
   - **Transition → Transition**: 忽略(不符合 Petri 网语义)

3. **初始标记**:
   - Constant/Static 节点: 解析 `init_value` 为 token 数量
   - 其他节点: 默认 0

4. **EdgeMode 映射**:
   - `Move`: 消耗性弧(权重 1)
   - `Ref`/`MutRef`: 非消耗性弧(需要守卫逻辑)
   - `Implements`/`Require`: 约束弧(用于守卫条件)
   - `Include`/`Alias`/`Instance`: 结构性弧

5. **添加基本类型 shim** (`add_primitive_shims()`):
   - 为基本类型添加构造器 transition(如 `u8::new()`)
   - 用于 Fuzzing 时生成基本类型实例

**输出**: `LabeledPetriNet` 结构,包含:
- `places: Vec<String>` - 库所列表
- `transitions: Vec<String>` - 变迁列表
- `transition_attrs: Vec<TransitionAttr>` - 变迁属性(is_const, is_async, is_unsafe)
- `arcs: Vec<Arc>` - 弧列表
- `initial_marking: Vec<usize>` - 初始标记
- `trans_to_node` / `place_to_node` - 到原 IrGraph 的映射

### 阶段 4: 导出 Petri Net

**支持格式**:
- **DOT**: Graphviz 可视化格式
- **JSON**: 结构化数据格式
- **PNML**: Petri Net Markup Language (XML 格式)

**导出逻辑** (`label_pt_net::export.rs`):
- `to_dot()`: 生成 DOT 格式,Places 为圆形,Transitions 为方框
- `to_json()`: 使用 serde 序列化整个结构
- `to_pnml()`: 生成标准 PNML XML 格式

### 阶段 5: 生成 Fuzz 项目 (可选)

**输入**: 配置中的 `--fuzz` 标志

**处理过程** (`Pipeline::gen_fuzz()`):
1. 创建 `fuzz/` 目录结构
2. 生成 `Cargo.toml`,包含 libfuzzer-sys 和 arbitrary 依赖
3. 生成基础的 `fuzz_target_1.rs` 模板
4. 配置被测库的路径依赖

## 关键技术细节

### TypeCache 系统

**目的**: 统一管理类型节点,避免重复创建

**核心结构**:
- `id_to_node: HashMap<Id, NodeIndex>` - rustdoc ID → 图节点索引
- `type_key_to_node: HashMap<TypeKey, NodeIndex>` - 类型键 → 图节点索引
- `primitive_to_node: HashMap<String, NodeIndex>` - 基本类型名 → 图节点索引
- `generic_to_node: HashMap<String, NodeIndex>` - 泛型名 → 图节点索引
- `assoc_type_cache: HashMap<(String, String), NodeIndex>` - (类型名, 关联类型名) → 图节点索引

**TypeKey 类型**:
- `Resolved(Id)` - 已解析的类型 ID
- `Primitive(String)` - 基本类型名
- `Generic { name: String, scope: GenericScope }` - 泛型参数
- `Tuple(Vec<TypeKey>)` - 元组类型
- `TraitWithArgs { trait_id: Id, args_repr: String }` - 带参数的 Trait

**GenericScope**:
- `Type(Id)` - 类型级别的泛型
- `Trait(Id)` - Trait 级别的泛型
- `Method(Id)` - 方法级别的泛型

### EdgeMode 语义

- **Move**: 按值移动,所有权转移(消耗性)
- **Ref**: 共享引用 `&T`(非消耗性,需要借用检查)
- **MutRef**: 可变引用 `&mut T`(非消耗性,独占)
- **Implements**: 类型实现 Trait(关系边)
- **Require**: 泛型需要满足 Trait 约束(关系边)
- **Include**: 类型包含泛型参数(关系边)
- **Alias**: 类型别名,如 Associated Type(关系边)
- **Instance**: Const/Static 是某个类型的实例(关系边)
- **UnwrapOk/UnwrapErr/UnwrapNone**: Result/Option 展开操作

### 方法节点构建细节

**输入参数处理** (`process_function_inputs_with_self()`):
1. 识别 `self` 参数:
   - `self` → `Move` 边: 类型 → 方法
   - `&self` → `Ref` 边: 类型 → 方法
   - `&mut self` → `MutRef` 边: 类型 → 方法

2. 处理其他参数:
   - 解析参数类型,创建或获取类型节点
   - 建立输入弧: 类型 → 方法

**返回值处理** (`process_function_output()`):
1. 解析返回类型
2. 处理 `Result<T, E>`:
   - 创建 `ResultWrapper` 节点
   - 创建 `UnwrapOp` transition
   - 建立 `UnwrapOk` 和 `UnwrapErr` 边

3. 处理 `Option<T>`:
   - 创建 `OptionWrapper` 节点
   - 创建 `UnwrapOp` transition
   - 建立 `UnwrapOk` 和 `UnwrapNone` 边

### 黑名单机制

**Trait 黑名单** (`support_types::trait_blacklist`):
- 过滤标准库中的通用 Trait(如 `Clone`, `Debug` 等)
- 避免生成过多无意义的边

**方法黑名单** (`support_types::method_blacklist`):
- 过滤标准库中的通用方法(如 `clone`, `fmt` 等)
- 但记录对应的 Trait,用于类型信息完整性

### 泛型约束处理

**Trait 约束解析**:
1. 从泛型参数的 `bounds` 中提取 Trait 约束
2. 处理带具体类型参数的 Trait(如 `AsRef<[u8]>`)
3. 创建 `Require` 边: Trait → 泛型参数

**约束满足检查**:
1. 收集所有 Primitive 类型的默认 Trait
2. 检查 Primitive 是否满足泛型的所有 Trait 约束
3. 为满足约束的 Primitive 添加 `Instance` 边

## 使用示例

### 基本用法

```bash
# 1. 生成 rustdoc JSON
cd /path/to/target/project
cargo +nightly rustdoc -- --output-format json

# 2. 运行 SyPetype
cd /path/to/SyPetype
cargo run -- /path/to/target/project/target/doc/<crate_name>.json

# 3. 查看输出
# - graph/petri_net.dot: DOT 格式可视化
# - graph/petri_net.json: JSON 格式数据
# - graph/petri_net.pnml: PNML 格式
```

### 高级用法

```bash
# 导出所有格式
cargo run -- lib.json -f all

# 同时导出 IR Graph
cargo run -- lib.json -e ir -v

# 仅解析,打印统计
cargo run -- lib.json -s parse

# 生成 fuzz 项目
cargo run -- lib.json --fuzz -c my_crate
```

## 数据流图

```
rustdoc JSON
    ↓
[Parse Module]
    ↓
ParsedCrate (预处理信息)
    ↓
[IR Graph Builder]
    ├─ build_types()          → 类型节点
    ├─ build_traits_nodes()   → Trait 节点
    ├─ build_type_generics()  → 泛型节点
    ├─ build_impl_methods()   → 方法节点
    └─ postprocess_*()        → 后处理
    ↓
IrGraph (中间表示)
    ├─ type_graph: DiGraph
    ├─ node_types: HashMap
    └─ node_infos: HashMap
    ↓
[Petri Net Converter]
    ├─ 节点分类 (Place/Transition)
    ├─ 边转换 (Arc)
    ├─ 初始标记设置
    └─ 添加 shim
    ↓
LabeledPetriNet
    ├─ places: Vec<String>
    ├─ transitions: Vec<String>
    ├─ arcs: Vec<Arc>
    └─ initial_marking: Vec<usize>
    ↓
[Export Module]
    ├─ to_dot()   → DOT 文件
    ├─ to_json()  → JSON 文件
    └─ to_pnml()  → PNML 文件
```

## 关键设计决策

1. **使用 TypeCache 统一管理类型节点**: 避免重复创建,支持泛型作用域区分
2. **分离数据节点和操作节点**: 符合 Petri 网语义,便于后续分析
3. **保留原始 rustdoc ID 映射**: 便于回溯和调试
4. **支持多种导出格式**: DOT(可视化)、JSON(程序处理)、PNML(标准格式)
5. **模块化设计**: 每个阶段职责清晰,便于扩展和维护

## 扩展点

1. **Fuzzing 引擎**: 基于 Petri 网生成 API 调用序列
2. **泛型约束完整支持**: 处理复杂的生命周期和 where 子句
3. **借用检查器集成**: 在 Petri 网模拟中实现借用规则
4. **更多导出格式**: 支持其他 Petri 网工具格式
5. **增量更新**: 支持只更新变更部分的图结构

## 依赖关系

- `rustdoc-types = "0.57.0"` - rustdoc JSON 格式解析
- `petgraph = "0.6"` - 图数据结构
- `serde` / `serde_json` - 序列化/反序列化
- `clap = "4.5"` - 命令行参数解析
- `anyhow` - 错误处理
- `log` / `env_logger` - 日志系统

## 注意事项

1. **需要 nightly Rust**: rustdoc JSON 输出格式需要 nightly 工具链
2. **内存占用**: 大型项目的图可能很大,注意内存使用
3. **泛型约束简化**: 当前实现简化了复杂的泛型约束,可能不完全准确
4. **黑名单机制**: 某些标准库 Trait/方法被过滤,可能影响完整性
