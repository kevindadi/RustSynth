# Capability 标注实现总结

## 问题

用户反馈：
1. **API 重复**：`api_graph_new.dot` 中存在重复的 API（`$Self` 和 `Counter` 版本都存在）
2. **缺少 Capability 信息**：边上需要标注引用类型（`own`, `&`, `&mut`），方便后续 PCPN 建立

## 解决方案

### 1. 消除 API 重复

**问题分析**：
- `api_graph_new.dot` 是测试时的旧文件（生成于修复 `impl` 块处理之前）
- 修复后的正确文件是 `api_graph_fixed.dot`，已经没有重复

**确认**：
```bash
$ ls -la *.dot
-rw-r--r--  api_graph_fixed.dot  (修复后，正确)
-rw-r--r--  api_graph_new.dot    (测试中，有重复)
```

### 2. 添加 Capability 标注

#### 修改 1：扩展 `TypeEdge` 结构

```rust
// src/api_graph.rs
pub struct TypeEdge {
    pub from_api: usize,
    pub to_api: usize,
    pub type_key: TypeKey,
    pub capability: crate::model::Capability,  // ← 新增
}
```

#### 修改 2：扩展 `ApiNode` 结构

```rust
// src/api_graph.rs
pub struct ApiNode {
    pub index: usize,
    pub api: ApiSignature,
    // 保存 (TypeKey, Capability) 而不是只有 TypeKey
    pub inputs: Vec<(TypeKey, Capability)>,    // ← 修改
    pub outputs: Vec<(TypeKey, Capability)>,   // ← 修改
    pub is_entry: bool,
    pub source: ApiSource,
}
```

#### 修改 3：`add_api_node` 提取 Capability

```rust
// src/api_graph.rs: add_api_node
let inputs: Vec<(TypeKey, Capability)> = api
    .all_params()
    .iter()
    .map(|p| {
        let cap = match p {
            ParamMode::ByValue(_, _) => Capability::Own,
            ParamMode::SharedRef(_) => Capability::Shr,
            ParamMode::MutRef(_) => Capability::Mut,
        };
        (p.type_key().clone(), cap)
    })
    .collect();

let outputs: Vec<(TypeKey, Capability)> = match &api.return_mode {
    ReturnMode::OwnedValue(ty, _) => vec![(ty.clone(), Capability::Own)],
    ReturnMode::SharedRef(ty) => vec![(ty.clone(), Capability::Shr)],
    ReturnMode::MutRef(ty) => vec![(ty.clone(), Capability::Mut)],
    ReturnMode::Unit => vec![],
};
```

#### 修改 4：`build_edges` 考虑 Capability 兼容性

```rust
// src/api_graph.rs: build_edges
for (output_ty, output_cap) in &producer_node.outputs {
    for (input_ty, input_cap) in &consumer_node.inputs {
        if output_ty != input_ty {
            continue;
        }
        
        // 检查 Capability 兼容性
        let compatible = match (output_cap, input_cap) {
            // Own 可以传给任何类型（通过临时借用）
            (Capability::Own, _) => true,
            // Shr 只能传给 Shr（共享引用可以复制）
            (Capability::Shr, Capability::Shr) => true,
            // Mut 可以传给 Shr 或 Mut
            (Capability::Mut, Capability::Shr) => true,
            (Capability::Mut, Capability::Mut) => true,
            // 其他情况不兼容
            _ => false,
        };
        
        if compatible {
            self.edges.push(TypeEdge {
                from_api: producer_node.index,
                to_api: consumer_node.index,
                type_key: output_ty.clone(),
                capability: *input_cap,  // 边的 capability 是消费者要求的
            });
        }
    }
}
```

#### 修改 5：节点标签显示 Capability

```rust
// src/api_graph.rs: format_node_label
let inputs = node
    .inputs
    .iter()
    .map(|(ty, cap)| {
        let ty_str = Self::simplify_type(ty);
        match cap {
            Capability::Own => ty_str,
            Capability::Shr => format!("&{}", ty_str),
            Capability::Mut => format!("&mut {}", ty_str),
        }
    })
    .collect::<Vec<_>>()
    .join(", ");
```

#### 修改 6：边标签显示 Capability

```rust
// src/api_graph.rs: to_dot
for edge in &self.edges {
    let cap_label = Self::capability_to_label(&edge.capability);
    let type_label = Self::simplify_type(&edge.type_key);
    
    let label = if edge.capability == Capability::Own {
        type_label  // Own 不需要特殊标记
    } else {
        format!("{} ({})", type_label, cap_label)
    };
    
    dot.push_str(&format!(
        "  n{} -> n{} [label=\"{}\", color=\"{}\"];\n",
        edge.from_api, edge.to_api, label, color
    ));
}

fn capability_to_label(cap: &Capability) -> &'static str {
    match cap {
        Capability::Own => "own",
        Capability::Shr => "&",
        Capability::Mut => "&mut",
    }
}
```

## 结果对比

### 修复前（`api_graph_new.dot`）
```dot
// 有重复的 API
n0 [label="increment\n($Self)", ...];              // ← 重复 1
n2 [label="increment\n(Counter)", ...];            // ← 重复 2
n6 [label="get\n($Self) → i32", ...];              // ← $Self 未解析
n7 [label="new\n() → $Self", ...];                 // ← $Self 未解析

// 边没有 Capability 信息
n7 -> n0 [label="$Self", ...];                     // ← 缺少 & 或 &mut
```

### 修复后（`api_graph.dot`）
```dot
// 没有重复，类型正确
n0 [label="print_counter\n(&Counter)", ...];       // ✓ 显示 &
n2 [label="new\n() → Counter", ...];               // ✓ Self 已解析
n3 [label="increment\n(&mut Counter)", ...];       // ✓ 显示 &mut
n4 [label="get\n(&Counter) → i32", ...];           // ✓ 显示 &

// 边显示 Capability
n2 -> n0 [label="Counter (&)", ...];               // ✓ 显示传递方式
n2 -> n3 [label="Counter (&mut)", ...];            // ✓ 显示传递方式
n4 -> n1 [label="i32", ...];                       // ✓ primitive 不显示 own
```

## Capability 兼容性规则

在 PCPN 模型中，边的 Capability 表示**消费者**要求的传递方式：

| 生产者 | 消费者 | 是否兼容 | 说明 |
|--------|--------|----------|------|
| `Own(T)` | `Own(T)` | ✓ | 直接转移所有权 |
| `Own(T)` | `&T` | ✓ | 临时共享借用（NLL） |
| `Own(T)` | `&mut T` | ✓ | 临时可变借用（NLL） |
| `&T` | `&T` | ✓ | 共享引用可复制 |
| `&T` | `Own(T)` | ✗ | 无法从引用获得所有权 |
| `&T` | `&mut T` | ✗ | 无法从共享引用升级为可变引用 |
| `&mut T` | `&T` | ✓ | 可变引用可降级为共享引用 |
| `&mut T` | `&mut T` | ✓ | 可变引用可转移（如果未被使用） |
| `&mut T` | `Own(T)` | ✗ | 无法从引用获得所有权 |

**关键点**：
- `Own` 是最灵活的，可以适配任何需求（通过临时借用）
- `&` 只能传给 `&`（共享引用可复制）
- `&mut` 可以传给 `&` 或 `&mut`（可降级）
- 引用无法获得所有权（需要 `.clone()` 等）

## 对 PCPN 建模的意义

### 1. Place 和 Token 的精确建模
```rust
Place(TypeKey) : multiset<Token>

Token {
    id: VarId,
    ty: TypeKey,
    cap: Capability,  // ← 这就是边上标注的信息！
    origin: Option<VarId>,
}
```

### 2. Transition 的前置条件检查
```rust
// 如果边标注为 Counter (&)，说明需要 Capability::Shr
// 在 enable 检查时：
for token in place.tokens {
    if token.ty == "Counter" && token.cap == Capability::Own {
        // 可以临时借用！
        owner_status.add_shr(token.id);  // 记录借用
    } else if token.cap == Capability::Shr {
        // 直接使用共享引用
    }
}
```

### 3. 状态转移的精确控制
```rust
// 如果边标注为 Counter (&mut)，说明需要 Capability::Mut
if edge.capability == Capability::Mut {
    // 检查 OwnerStatus
    assert!(owner_status.is_free(var_id));
    owner_status.set_mut(var_id);
}
```

### 4. 借用冲突的自动检测
```rust
// 图中如果有多条边从同一个 API 出发：
new() → n3 [label="Counter (&mut)"]  // ← 需要独占借用
new() → n4 [label="Counter (&)"]     // ← 需要共享借用

// 这两条边不能同时使能！PCPN 引擎会自动检测冲突
```

## 修改文件清单

1. **`src/api_graph.rs`**
   - 修改 `TypeEdge` 结构（添加 `capability` 字段）
   - 修改 `ApiNode` 结构（`inputs` 和 `outputs` 改为 `(TypeKey, Capability)`）
   - 修改 `add_api_node`（提取 Capability 信息）
   - 修改 `build_edges`（考虑 Capability 兼容性）
   - 修改 `format_node_label`（节点显示 Capability）
   - 修改 `to_dot`（边显示 Capability）
   - 添加 `capability_to_label` 辅助函数

2. **无需修改**
   - `src/model.rs`：`Capability` 枚举已存在
   - `src/api_extract.rs`：`ParamMode` 和 `ReturnMode` 已经包含 Capability 信息

## 测试验证

```bash
$ cargo build
   Compiling sypetype v0.1.0
    Finished `dev` profile

$ ./target/debug/sypetype --input examples/simple_counter/target/doc/simple_counter.json --graph api_graph.dot
   INFO: 提取到 6 个 API
   INFO: API 图: 6 个节点, 9 条边
   INFO: ✓ API 依赖图已生成: "api_graph.dot"
```

## 后续工作

1. **Trait 实现**：添加 `Default::default()` 作为入口点
2. **Public 字段**：提取 `struct` 的 public 字段作为可访问的"API"
3. **搜索引擎**：在 `src/search.rs` 中使用 API 图的 Capability 信息来指导状态搜索
4. **借用栈**：在 PCPN 执行时维护 `LoanStack`，跟踪活跃的借用

**最后更新**：2026-01-08
