# API 依赖图实现说明

## 已实现的功能

### 1. 核心功能 ✅

#### 类型依赖分析
- ✅ 识别 API 的输入类型（参数）
- ✅ 识别 API 的输出类型（返回值）
- ✅ 构建类型依赖边（从生产者到消费者）
- ✅ 标记入口 API（无参数或只需 primitive）

#### Trait 实例化
- ✅ 框架已实现，可检测 trait impls
- ✅ 支持 `Default` trait
- ✅ 支持 `Clone` trait
- 📝 可轻松扩展其他 traits（`From`, `Into`, `Iterator` 等）

#### Public 字段访问
- ✅ 框架已实现
- ✅ 扫描 struct 的 public 字段
- ✅ 生成字段访问节点（`struct.field`）
- 📝 可作为类型来源使用

#### DOT 可视化
- ✅ 生成标准 DOT 格式
- ✅ 节点按来源分类着色（普通/Trait/字段）
- ✅ 入口 API 用双八边形标记
- ✅ 边按类型着色（primitive=灰色，其他=黑色）
- ✅ 节点标签显示参数和返回值

### 2. 使用方法

```bash
# 生成 API 依赖图
sypetype --input target/doc/your_crate.json --graph api_graph.dot

# 渲染为图片
dot -Tpng api_graph.dot -o api_graph.png
dot -Tsvg api_graph.dot -o api_graph.svg
dot -Tpdf api_graph.dot -o api_graph.pdf

# 结合搜索使用
sypetype --input target/doc/your_crate.json \
    --graph api_graph.dot \
    --output witness.rs \
    --verbose
```

### 3. 生成的图示例

对于 `simple_counter` 示例，生成的图显示：

**入口 APIs** (双八边形):
- `new()` → 返回 Counter
- `create_counter_with_value(i32)` → 返回 Counter

**消费 Counter 的 APIs**:
- `increment(&mut Counter)` → 修改 Counter
- `get(&Counter)` → 返回 i32
- `reset(&mut Counter)` → 修改 Counter
- `print_counter(&Counter)` → 打印

**类型流动**:
```
new() ────Counter───→ increment()
  └─────Counter───→ get() ──i32──→ create_counter_with_value()
  └─────Counter───→ reset()

create_counter_with_value() ──Counter──→ print_counter()
```

### 4. 数据结构

#### ApiNode
```rust
pub struct ApiNode {
    pub index: usize,        // 节点索引
    pub api: ApiSignature,   // API 签名
    pub inputs: Vec<TypeKey>,  // 输入类型
    pub outputs: Vec<TypeKey>, // 输出类型
    pub is_entry: bool,      // 是否入口
    pub source: ApiSource,   // 来源（Normal/Trait/Field）
}
```

#### TypeEdge
```rust
pub struct TypeEdge {
    pub from_api: usize,     // 生产者 API
    pub to_api: usize,       // 消费者 API
    pub type_key: TypeKey,   // 传递的类型
}
```

#### ApiGraph
```rust
pub struct ApiGraph {
    pub nodes: Vec<ApiNode>,
    pub edges: Vec<TypeEdge>,
    pub producers: IndexMap<TypeKey, Vec<usize>>,  // 类型→生产者
    pub consumers: IndexMap<TypeKey, Vec<usize>>,  // 类型→消费者
    pub trait_impls: HashMap<String, Vec<TraitImplInfo>>,
    pub public_fields: HashMap<TypeKey, Vec<FieldInfo>>,
}
```

## 实现要点

### 1. Trait 实例化的实现

```rust
// 扫描 rustdoc JSON 中的 impl 块
for (id, item) in &krate.index {
    if let ItemEnum::Impl(impl_) = &item.inner {
        // 获取 impl 的 self 类型
        let self_type = extract_self_type(&impl_.for_, type_ctx);
        
        // 检查是否实现了 trait
        if let Some(trait_ref) = &impl_.trait_ {
            let trait_name = get_trait_name(trait_ref, krate);
            
            match trait_name.as_str() {
                "Default" => {
                    // 添加 Default::default() API
                    add_trait_api("default", vec![], self_type);
                }
                "Clone" => {
                    // 添加 Clone::clone(&self) API
                    add_trait_api("clone", vec![&self_type], self_type);
                }
                // 可扩展其他 traits...
            }
        }
    }
}
```

### 2. Public 字段访问的实现

```rust
// 扫描 struct 定义
for (id, item) in &krate.index {
    if let ItemEnum::Struct(struct_) = &item.inner {
        let struct_type = get_type_name(id, type_ctx);
        
        // 遍历字段
        for field_id in &struct_.fields {
            let field_item = krate.index.get(field_id)?;
            
            // 检查可见性
            if matches!(field_item.visibility, Visibility::Public) {
                let field_name = field_item.name?;
                let field_type = get_field_type(field_item)?;
                
                // 添加字段访问 API: struct.field
                // 相当于：fn field(&struct_type) -> field_type
                add_field_api(struct_type, field_name, field_type);
            }
        }
    }
}
```

### 3. 泛型参数的处理

当前实现：
- ✅ 泛型参数显示为 `$T`, `$Self` 等占位符
- ✅ 在依赖图中保留占位符，便于识别泛型关系
- 📝 未来可以实现：
  - 展开具体的泛型实例（`Vec<i32>`, `Option<String>`）
  - 为每个实例生成单独的 API 节点
  - 处理 trait bounds（`T: Clone`）

示例：
```rust
// 源代码
pub fn process<T: Clone>(item: T) -> T { item.clone() }

// 当前在图中显示为
process
($T) → $T

// 可以扩展为
process<i32>       process<String>
(i32) → i32        (String) → String
```

## 如何扩展

### 添加新的 Trait 支持

在 `api_graph.rs` 的 `extract_trait_impls` 中添加：

```rust
match trait_name.as_str() {
    "Default" => { /* 已实现 */ }
    "Clone" => { /* 已实现 */ }
    
    // 添加 From trait
    "From" => {
        // From<U>: fn from(U) -> Self
        self.trait_impls.entry("From".to_string())
            .or_insert_with(Vec::new)
            .push(TraitImplInfo {
                trait_name: "From".to_string(),
                self_type: self_type.clone(),
                method_name: "from".to_string(),
                inputs: vec![extract_from_type(impl_)],
                output: Some(self_type),
            });
    }
    
    // 添加 Iterator trait
    "Iterator" => {
        // Iterator: fn next(&mut self) -> Option<Item>
        let item_type = extract_associated_type(impl_, "Item");
        self.trait_impls.entry("Iterator".to_string())
            .or_insert_with(Vec::new)
            .push(TraitImplInfo {
                trait_name: "Iterator".to_string(),
                self_type: self_type.clone(),
                method_name: "next".to_string(),
                inputs: vec![self_type.clone()],
                output: Some(format!("Option<{}>", item_type)),
            });
    }
    
    _ => {}
}
```

### 处理关联类型

```rust
fn extract_associated_type(impl_: &Impl, name: &str) -> TypeKey {
    // 在 impl 块中查找关联类型定义
    for item_id in &impl_.items {
        if let Some(item) = krate.index.get(item_id) {
            if let ItemEnum::AssocType { type_, .. } = &item.inner {
                if item.name == Some(name.to_string()) {
                    return type_ctx.normalize_type(type_)?;
                }
            }
        }
    }
    format!("${}", name) // 占位符
}
```

### 添加方法重载支持

```rust
// 对于有多个签名的方法（不同参数类型）
// 为每个签名生成独立的节点

pub struct ApiNode {
    // ...
    pub overload_id: Option<usize>, // 重载编号
}

// 在生成图时区分
format!("{}#{}", method_name, overload_id)
```

## 与搜索的集成

### 使用 API 图指导搜索

```rust
pub fn heuristic_search(
    apis: &[ApiSignature],
    graph: &ApiGraph,
    config: &SearchConfig,
) -> Result<Vec<Transition>> {
    // 1. 从入口 APIs 开始
    let entry_apis: Vec<_> = graph.nodes.iter()
        .filter(|n| n.is_entry)
        .collect();
    
    // 2. 使用图的边优先探索有连接的 API
    for entry in entry_apis {
        let reachable = graph.find_reachable_from(entry.index);
        // 优先搜索可达的 API 子集
    }
    
    // 3. 利用 trait impls 创建 tokens
    for (trait_name, impls) in &graph.trait_impls {
        // 如果需要某类型，检查是否有 Default impl
        if trait_name == "Default" {
            // 添加 T::default() 作为候选
        }
    }
    
    // 4. 利用字段访问转换类型
    for (parent_ty, fields) in &graph.public_fields {
        // 如果有 parent_ty，可以通过 .field 获取 field_ty
    }
}
```

### 评分函数改进

```rust
fn evaluate_quality(trace: &[Transition], graph: &ApiGraph) -> f64 {
    let mut score = 0.0;
    
    // 1. API 多样性
    let unique_apis = count_unique_apis(trace);
    score += unique_apis as f64 * 10.0;
    
    // 2. 遵循依赖图的边
    for i in 0..trace.len()-1 {
        let current_api = &trace[i];
        let next_api = &trace[i+1];
        
        // 如果图中有从 current 到 next 的边，加分
        if graph.has_edge(current_api, next_api) {
            score += 20.0;
        }
    }
    
    // 3. 使用 trait impls 加分
    for trans in trace {
        if matches!(trans.source, ApiSource::TraitImpl { .. }) {
            score += 15.0;
        }
    }
    
    score
}
```

## 可视化示例

### 命令

```bash
# 生成图
sypetype --input examples/simple_counter/target/doc/simple_counter.json \
    --graph api_graph.dot

# 渲染（不同布局）
dot -Tpng api_graph.dot -o graph_lr.png           # 左到右
dot -Tpng -Grankdir=TB api_graph.dot -o graph_tb.png  # 上到下
fdp -Tpng api_graph.dot -o graph_fdp.png          # 力导向布局
circo -Tpng api_graph.dot -o graph_circo.png      # 圆形布局
```

### 在线查看

访问 [GraphvizOnline](https://dreampuf.github.io/GraphvizOnline/)，粘贴生成的 DOT 内容。

## 实际效果

对于 `simple_counter` 示例，图清晰展示了：

1. **入口点**：`new()` 和 `create_counter_with_value(i32)`
2. **类型流动**：Counter 从入口流向各个方法
3. **类型转换**：`get()` 将 Counter 转为 i32
4. **调用链**：可以看出合理的调用顺序

这为搜索算法提供了重要指导：
- 优先调用入口 API 创建初始 tokens
- 沿着边的方向探索后续调用
- 识别类型转换路径

## 未来改进

1. **泛型展开**：为具体类型实例生成单独节点
2. **关联类型**：完整支持 trait 关联类型
3. **更多 traits**：From, Into, Iterator, Debug 等
4. **方法链分析**：识别常见的方法链模式
5. **交互式可视化**：Web UI 查看和探索图

## 总结

API 依赖图功能已完整实现，包括：
- ✅ 类型依赖分析
- ✅ Trait 实例化框架
- ✅ Public 字段访问
- ✅ DOT 可视化

这为改进搜索策略和代码生成质量提供了坚实基础！
