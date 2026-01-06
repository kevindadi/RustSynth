# SyPetype 架构设计文档

## 概述

SyPetype 是一个基于 Colored Petri Net (CPN) 理论的 Rust API 可达性分析工具。它将 Rust 的所有权和借用系统建模为着色网中的 token 和 transition，通过状态空间搜索找到可行的 API 调用序列。

## 理论基础

### Colored Petri Net (CPN)

- **Place (库所)**：按类型分桶的资源容器
- **Token (着色 token)**：携带 capability (own/shr/mut) 的资源实例
- **Transition (变迁)**：API 调用或结构性操作（borrow/drop）
- **Marking (标识)**：当前状态下所有 place 中的 token 分布

### Pushdown 扩展

可选的 LoanStack 提供 LIFO 借用栈，用于：
- 追踪借用的嵌套结构
- 强制 LIFO 借用结束顺序
- 支持更精确的生命周期建模

### Binding-Element Enabling

- **Binding**：参数到 token 的绑定
- **Enabling**：检查绑定是否满足借用规则
- **Firing**：应用 transition 产生新状态

## 系统架构

```
┌─────────────────────────────────────────────────────────────┐
│                         CLI (main.rs)                        │
│  - 参数解析 (clap)                                           │
│  - 日志初始化 (tracing)                                      │
│  - 流程编排                                                  │
└───────────────┬─────────────────────────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────────────────────────┐
│              rustdoc_loader.rs                               │
│  - 加载 rustdoc JSON                                         │
│  - 反序列化 Crate 结构                                       │
└───────────────┬─────────────────────────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────────────────────────┐
│              type_norm.rs                                    │
│  - 构建 TypeContext                                          │
│  - Type → TypeKey 归一化                                     │
│  - 提取 base type + capability                               │
│  - Copy trait 近似判断                                       │
└───────────────┬─────────────────────────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────────────────────────┐
│              api_extract.rs                                  │
│  - 遍历 Crate items                                          │
│  - 提取 Function/Method 签名                                 │
│  - 生成 ApiSignature (ParamMode + ReturnMode)                │
└───────────────┬─────────────────────────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────────────────────────┐
│              search.rs                                       │
│  - BFS 状态空间搜索                                          │
│  - 初始状态：空                                              │
│  - 目标：封闭状态 + 有 tokens                                │
│  - Bounds 检查                                               │
│  - Trace 重建                                                │
└───────────────┬─────────────────────────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────────────────────────┐
│              transition.rs                                   │
│  - 生成 enabled transitions                                  │
│  - 参数绑定枚举（含适配）                                    │
│  - Apply transition (consume/produce tokens)                 │
│  - 借用规则检查                                              │
└───────────────┬─────────────────────────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────────────────────────┐
│              canonicalize.rs                                 │
│  - α-重命名 (VarId 规范化)                                   │
│  - 状态 hash 计算                                            │
│  - 去重状态空间                                              │
└───────────────┬─────────────────────────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────────────────────────┐
│              emit.rs                                         │
│  - Trace → Rust 代码                                         │
│  - 变量命名 (x0, r0, ...)                                    │
│  - 适配策略转换为语法 (&, &mut, &*)                          │
│  - 可选：cargo check 验证                                    │
└─────────────────────────────────────────────────────────────┘
```

## 核心数据结构

### model.rs

#### Token
```rust
pub struct Token {
    cap: Capability,      // own/shr/mut
    id: VarId,            // 变量 ID
    ty: TypeKey,          // 类型全称路径
    origin: Option<VarId>,// 借用来源
    is_copy: bool,        // 是否 Copy
    meta: Option<String>, // 元数据
}
```

#### State
```rust
pub struct State {
    places: IndexMap<TypeKey, Vec<Token>>,  // Place multiset
    owner_status: IndexMap<VarId, BorrowFlag>, // 借用状态
    loan_stack: Option<Vec<LoanFrame>>,     // 可选 LIFO 栈
    next_var_id: VarId,                     // 下一个变量 ID
}
```

#### BorrowFlag
```rust
pub enum BorrowFlag {
    Free,           // 无借用
    ShrCount(usize),// n 个共享借用
    MutActive,      // 一个可变借用
}
```

### api_extract.rs

#### ApiSignature
```rust
pub struct ApiSignature {
    full_path: String,          // 函数全路径
    is_method: bool,            // 是否方法
    self_mode: Option<ParamMode>,// self 参数
    params: Vec<ParamMode>,     // 其他参数
    return_mode: ReturnMode,    // 返回值
    is_unsafe: bool,            // 是否 unsafe
}
```

#### ParamMode
```rust
pub enum ParamMode {
    ByValue(TypeKey, bool),  // (type, is_copy)
    SharedRef(TypeKey),      // &T
    MutRef(TypeKey),         // &mut T
}
```

### transition.rs

#### Transition
```rust
pub enum Transition {
    ApiCall(ApiCallTransition),
    Structural(StructuralTransition),
}
```

#### AdaptationStrategy
```rust
pub enum AdaptationStrategy {
    Direct,          // 直接使用
    OwnedToShared,   // own → &
    OwnedToMut,      // own → &mut
    MutToShared,     // mut → &*
}
```

## 关键算法

### 1. 类型归一化 (type_norm.rs)

```
normalize_type(Type) → (TypeKey, Capability, is_copy)

- ResolvedPath → (path, Own, is_copy)
- Primitive → (name, Own, true)
- BorrowedRef { mutable, inner } →
    (normalize(inner).key, Shr|Mut, true|false)
- Tuple([]) → ("()", Own, true)
- Generic(T) → ("$T", Own, false)
```

### 2. 参数绑定枚举 (transition.rs)

```
enumerate_bindings(state, params, idx, current, results):
    if idx >= len(params):
        results.push(current)
        return
    
    candidates = find_candidate_tokens(state, params[idx])
    for candidate in candidates.take(MAX_CANDIDATES):
        enumerate_bindings(state, params, idx+1, 
                          current + [candidate], results)
```

### 3. 候选 Token 查找 (transition.rs)

```
find_candidate_tokens(state, param):
    match param:
        ByValue(T, is_copy):
            - 查找 own(T) tokens
            - 检查可移动（is_copy 或 can_borrow_mut）
        
        SharedRef(T):
            - 直接：shr(T) tokens
            - 适配：own(T) tokens (can_borrow_shr)
            - 重借用：mut(T) tokens
        
        MutRef(T):
            - 直接：mut(T) tokens
            - 适配：own(T) tokens (can_borrow_mut)
```

### 4. 借用规则检查 (model.rs)

```
can_borrow_shr(owner_id):
    flag = owner_status[owner_id]
    return flag != MutActive

can_borrow_mut(owner_id):
    flag = owner_status[owner_id]
    return flag == Free
```

### 5. 状态规范化 (canonicalize.rs)

```
canonicalize(state):
    renaming = {}
    next_id = 0
    
    # 1. 按 loan_stack 顺序（如果启用）
    for frame in reversed(loan_stack):
        if frame.owner not in renaming:
            renaming[frame.owner] = next_id++
        if frame.reference not in renaming:
            renaming[frame.reference] = next_id++
    
    # 2. 按 tokens 排序
    sorted_tokens = sort(all_tokens, by=(cap, ty, origin, id))
    for token in sorted_tokens:
        if token.id not in renaming:
            renaming[token.id] = next_id++
        if token.origin not in renaming:
            renaming[token.origin] = next_id++
    
    return apply_renaming(state, renaming)
```

### 6. 可达性搜索 (search.rs)

```
search(apis, type_ctx, config):
    initial = State::new()
    queue = [(initial, None, None)]
    visited = {hash(initial): (None, None)}
    
    while queue not empty:
        (state, parent, trans) = queue.pop()
        
        if check_goal(state, config):
            return reconstruct_trace(visited, hash(state))
        
        if depth(state) >= max_steps:
            continue
        
        transitions = generate_enabled_transitions(state, apis)
        
        for trans in transitions:
            next = apply_transition(state, trans)
            
            if not check_bounds(next, config):
                continue
            
            h = hash(next)
            if h not in visited:
                visited[h] = (hash(state), trans)
                queue.push((next, hash(state), trans))
    
    return None
```

### 7. 代码生成 (emit.rs)

```
emit_code(trace):
    output = "fn generated_witness() {\n"
    var_names = {}
    
    for (step, trans) in enumerate(trace):
        match trans:
            ApiCall(call):
                args = [format_arg(b, var_names) for b in call.bindings]
                call_expr = f"{call.api.path}({', '.join(args)})"
                
                if has_return:
                    var = alloc_var_name(return_id, is_ref)
                    output += f"    let {var} = {call_expr};\n"
                else:
                    output += f"    {call_expr};\n"
            
            Structural(Drop { id }):
                output += f"    drop({var_names[id]});\n"
            
            Structural(BorrowShr { owner, ref }):
                output += f"    let {ref} = &{owner};\n"
    
    output += "}\n"
    return output
```

## 工程化权衡

### 简化 vs 完整性

| 方面 | 简化策略 | 完整实现 |
|------|---------|---------|
| 泛型 | 不展开，只用 base path | 单态化，展开所有实例 |
| Copy trait | 近似判断 | 完整 trait 求解 |
| 生命周期 | 不写，依赖 NLL | 显式标注 |
| 返回引用 origin | 使用第一个参数 | 完整生命周期分析 |
| 参数绑定 | 限制前 3 个候选 | 完整枚举 |

### Soundness vs Completeness

- **Soundness 优先**：宁可拒绝不确定的情况，也不生成错误代码
- **Completeness 次要**：可能漏掉某些可行轨迹，但不会生成非法代码

示例：
- 返回引用但无法推断 origin → 拒绝此 transition
- 违反 LIFO 借用 → 拒绝此 transition
- 超过 token 数量限制 → 停止扩展

### 性能优化

1. **状态去重**：通过 α-重命名减少状态空间
2. **候选限制**：参数绑定最多 3 个候选
3. **Bounds 检查**：及早剪枝不可行分支
4. **迭代限制**：最多 10000 次迭代

## 扩展点

### 1. 新增 Structural Transition

在 `transition.rs` 中添加：

```rust
pub enum StructuralTransition {
    // 现有
    DropOwned { token_id: VarId },
    BorrowShr { owner_id: VarId, ref_id: VarId },
    BorrowMut { owner_id: VarId, ref_id: VarId },
    EndBorrow { ref_id: VarId, owner_id: VarId },
    
    // 新增
    DerefCopy { ref_id: VarId, new_id: VarId },  // *r (Copy)
    ReborrowMut { mut_id: VarId, new_mut_id: VarId }, // &mut *r
    Clone { token_id: VarId, new_id: VarId },    // x.clone()
}
```

### 2. 更精确的 Trait 分析

在 `type_norm.rs` 中增强：

```rust
impl TypeContext {
    pub fn implements_trait(&self, ty: &TypeKey, trait_name: &str) -> bool {
        // 查询 rustdoc 的 trait impls
        // 或使用 chalk/rustc trait solver
    }
}
```

### 3. 泛型单态化

在 `type_norm.rs` 中添加：

```rust
pub struct MonomorphizedType {
    base: TypeKey,
    args: Vec<TypeKey>,
}

impl TypeContext {
    pub fn monomorphize(&self, ty: &Type) -> MonomorphizedType {
        // 展开泛型参数
    }
}
```

### 4. 自定义目标谓词

在 `search.rs` 中支持：

```rust
pub trait GoalPredicate {
    fn check(&self, state: &State) -> bool;
}

pub struct CustomGoal {
    // 用户定义的目标条件
}
```

## 测试策略

### 单元测试

- `model.rs`: Token/State 操作
- `canonicalize.rs`: α-重命名正确性
- `transition.rs`: 借用规则检查

### 集成测试

- 简单 crate (examples/simple_counter)
- 复杂 API (std 子集)
- 边界情况（空 crate、无公开 API）

### Golden Tests

保存已知 crate 的输出，回归测试：

```rust
#[test]
fn test_simple_counter_golden() {
    let output = run_sypetype("examples/simple_counter/...");
    assert_eq!(output, include_str!("golden/simple_counter.rs"));
}
```

## 未来方向

1. **并行搜索**：多线程 BFS
2. **增量分析**：缓存中间结果
3. **交互式调试**：逐步执行 transition
4. **图形化可视化**：状态空间图
5. **IDE 集成**：LSP 支持
6. **Fuzzing 集成**：生成 fuzz 测试用例

---

**设计原则**：
- Soundness > Completeness
- 简单 > 复杂
- 可扩展 > 一次性完美
- 工程可用 > 理论完备

