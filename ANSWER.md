# 回答：为什么只找到一条轨迹？

## 问题分析

你的观察非常准确！从输出看，工具提取了 6 个 API，但只生成了重复调用 `new()` 的轨迹：

```rust
let x0 = new();
let x1 = new();
let x2 = new();
```

明显可以生成更有意义的轨迹，比如：
```rust
let counter = Counter::new();
counter.increment();
let val = counter.get();
print_counter(&counter);
```

## 根本原因

### 1. ✅ 搜索过早停止（已修复）
**问题**: 原来的目标条件是"封闭状态 + 有 tokens"，第一次 `new()` 就满足了。

**修复**: 增加最小深度要求（至少 3 步），避免过早停止。

### 2. ✅ 缺少 Primitive 常量（已添加）
**问题**: `create_counter_with_value(i32)` 需要 `i32` 参数，但没有办法创建。

**修复**: 添加 `CreatePrimitive` transition，自动生成 `let x: i32 = 0;`

### 3. ❌ BFS 搜索策略问题（核心问题）

**问题**: 当前使用简单的 BFS：
- 从空状态开始
- 每步枚举所有 enabled transitions
- 找到第一个满足条件的就停止

这导致：
- 无参函数（如 `new()`）总是优先被探索
- 需要参数的函数（如 `increment(&mut self)`）被延后
- 没有鼓励"使用已有 token"的机制

**示例搜索过程**:
```
深度 0: [] (空状态)
深度 1: [new()], [new()], [new()], ... (可以一直调用 new)
深度 2: [new(), new()], [new(), get(&x0)], ...
深度 3: [new(), new(), new()] ← 满足条件，停止！
```

**真正期望的探索顺序**:
```
深度 0: []
深度 1: [new()] → 产生 Counter token x0
深度 2: [new(), increment(&mut x0)] → 使用 x0
深度 3: [new(), increment(&mut x0), get(&x0)] → 继续使用 x0
```

## 你提出的关键见解

### 1. "明显内部函数都是可以调用的呀"
是的！但 BFS 没有优先探索"使用已有 token"的路径。

### 2. "还可能存在一些 constant，已经被定义好的类型"
✅ 已添加 primitive 常量支持（i32, bool 等）

### 3. "复合类型内部还是有 const 方法，能够直接发生，不需要库所的"
好想法！需要识别 `const fn` 和关联函数（`T::new()`）。

### 4. "你要不先构建一个 API graph？"
**这是最关键的改进！** API 图可以：
- 识别类型依赖：`new() -> Counter`, `increment(& mut Counter) -> ()`
- 发现调用链：`new() → increment() → get() → print_counter()`
- 指导搜索：优先探索有意义的路径

### 5. "所有 trait 都是实例化，内部对应的关联类型也被实例化？"
重要！当前忽略了：
- `Default::default()` → 可以创建很多类型
- `Clone::clone()` → 可以复制 token
- `Deref` → 自动解引用
- 关联类型（`Iterator::Item`）

## 完整解决方案

### 短期修复（已完成）
1. ✅ 增加最小深度要求
2. ✅ 添加 primitive 常量支持
3. ✅ 改进方法调用语法生成

### 中期改进（建议实现）

#### 方案 A: 多候选解搜索
不要在第一个解就停止，继续搜索收集多个候选，按质量排序：

```rust
fn search_multiple(apis, config) -> Vec<(State, Vec<Transition>, f64)> {
    let mut solutions = Vec::new();
    // 继续搜索直到找到 N 个解或超时
    while solutions.len() < config.max_solutions {
        if let Some(solution) = find_next_solution(...) {
            let quality = evaluate_quality(&solution);
            solutions.push((state, trace, quality));
        }
    }
    solutions.sort_by_key(|(_, _, q)| -q); // 按质量降序
    solutions
}

fn evaluate_quality(trace: &[Transition]) -> f64 {
    let mut score = 0.0;
    
    // 1. API 多样性
    let unique_apis = count_unique_apis(trace);
    score += unique_apis as f64 * 10.0;
    
    // 2. Token 重用率（使用已有 token）
    let reuses = count_token_reuses(trace);
    score += reuses as f64 * 5.0;
    
    // 3. 惩罚重复调用
    let repetitions = count_api_repetitions(trace);
    score -= repetitions as f64 * 20.0;
    
    score
}
```

#### 方案 B: API 依赖图（你的建议）

**第一步：构建类型依赖图**
```rust
struct ApiGraph {
    nodes: Vec<ApiNode>,
    edges: Vec<TypeEdge>,
}

struct ApiNode {
    api: ApiSignature,
    inputs: Vec<TypeKey>,   // 需要的类型
    outputs: Vec<TypeKey>,  // 产生的类型
    is_entry: bool,         // 无参数或只需 primitive
}

struct TypeEdge {
    from_api: usize,  // 产生此类型的 API
    to_api: usize,    // 消费此类型的 API
    type_key: TypeKey,
}

fn build_api_graph(apis: &[ApiSignature]) -> ApiGraph {
    let mut graph = ApiGraph::new();
    
    for api in apis {
        let inputs = api.all_params().iter()
            .map(|p| p.type_key().clone())
            .collect();
        let outputs = match &api.return_mode {
            ReturnMode::OwnedValue(ty, _) => vec![ty.clone()],
            _ => vec![],
        };
        let is_entry = inputs.is_empty() || 
            inputs.iter().all(|t| is_primitive(t));
        
        graph.add_node(ApiNode { api: api.clone(), inputs, outputs, is_entry });
    }
    
    // 构建边
    for (i, node_i) in graph.nodes.iter().enumerate() {
        for (j, node_j) in graph.nodes.iter().enumerate() {
            for out_ty in &node_i.outputs {
                if node_j.inputs.contains(out_ty) {
                    graph.add_edge(TypeEdge {
                        from_api: i,
                        to_api: j,
                        type_key: out_ty.clone(),
                    });
                }
            }
        }
    }
    
    graph
}
```

**第二步：基于图的启发式搜索**
```rust
fn heuristic_search(apis: &[ApiSignature], graph: &ApiGraph) -> Vec<Transition> {
    // 1. 从入口 API 开始
    let entry_apis: Vec<_> = graph.nodes.iter()
        .filter(|n| n.is_entry)
        .collect();
    
    // 2. 探索调用链
    for entry in entry_apis {
        let mut path = vec![entry];
        let mut available_types = entry.outputs.clone();
        
        // 3. 贪心选择下一个 API
        while let Some(next) = find_next_api(&graph, &available_types) {
            path.push(next);
            available_types.extend(next.outputs.clone());
            
            // 停止条件：达到目标或无路可走
            if is_satisfactory(&path) {
                break;
            }
        }
        
        if is_satisfactory(&path) {
            return build_trace_from_path(path);
        }
    }
    
    vec![]
}

fn find_next_api<'a>(
    graph: &'a ApiGraph, 
    available_types: &[TypeKey]
) -> Option<&'a ApiNode> {
    graph.nodes.iter()
        .filter(|node| {
            // 所有输入都可用
            node.inputs.iter().all(|t| available_types.contains(t))
        })
        .max_by_key(|node| {
            // 启发式：选择最"有趣"的 API
            // - 优先选择使用刚产生的类型
            // - 优先选择产生新类型
            let uses_new_types = node.inputs.iter()
                .filter(|t| available_types.contains(t))
                .count();
            let produces_new_types = node.outputs.iter()
                .filter(|t| !available_types.contains(t))
                .count();
            uses_new_types * 2 + produces_new_types
        })
}
```

#### 方案 C: Trait 实例化

```rust
fn extract_trait_impls(krate: &Crate) -> Vec<TraitImpl> {
    let mut impls = Vec::new();
    
    for item in &krate.index {
        if let ItemEnum::Impl(impl_) = &item.inner {
            // 提取 trait impl
            if let Some(trait_ref) = &impl_.trait_ {
                // Default::default()
                if trait_ref.name == "Default" {
                    impls.push(TraitImpl {
                        trait_name: "Default",
                        method: "default",
                        self_ty: impl_.for_,
                        inputs: vec![],
                        output: impl_.for_.clone(),
                    });
                }
                // Clone::clone()
                if trait_ref.name == "Clone" {
                    impls.push(TraitImpl {
                        trait_name: "Clone",
                        method: "clone",
                        self_ty: impl_.for_,
                        inputs: vec![ParamMode::SharedRef(impl_.for_.clone())],
                        output: impl_.for_.clone(),
                    });
                }
            }
        }
    }
    
    impls
}
```

## 实际应用示例

假设我们实现了 API 图，对 `simple_counter` 的分析：

```
API 图：
  Entry APIs (无参数):
    - Counter::new() → Counter
    - Counter::default() → Counter (via Default trait)
  
  消费 Counter:
    - Counter::increment(&mut Counter) → ()
    - Counter::get(&Counter) → i32
    - Counter::reset(&mut Counter) → ()
    - print_counter(&Counter) → ()
  
  消费 i32:
    - create_counter_with_value(i32) → Counter

调用链推荐：
  Chain 1: Counter::new() → increment() → get() → print_counter()
  Chain 2: Counter::new() → reset() → get()
  Chain 3: <create i32> → create_counter_with_value() → get()
```

搜索会优先探索这些链，生成：
```rust
fn generated_witness() {
    let counter = Counter::new();
    counter.increment();
    let val = counter.get();
    print_counter(&counter);
}
```

## 总结

你的问题抓住了关键：

1. ✅ **常量支持** - 已添加
2. ✅ **最小深度** - 已修复  
3. ❌ **API 图** - 最重要的改进，待实现
4. ❌ **Trait 实例化** - 需要完整实现
5. ❌ **启发式搜索** - 用质量度量指导搜索

**建议实施顺序**:
1. API 图构建（1-2 天）
2. 基于图的启发式搜索（1 天）
3. 多候选解 + 质量排序（半天）
4. Trait 实例化（1-2 天）

这些改进后，工具将能生成真正有意义的调用轨迹！
