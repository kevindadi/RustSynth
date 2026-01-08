# 已修复问题清单

本文档记录在开发过程中发现并修复的所有问题。

---

## ✅ Issue 1: Self 类型未正确解析

**发现时间**：2026-01-08  
**用户反馈**："你的 Self 没有解析呀，你返回 Self 应该自动解析为 Counter 类型"

### 问题表现
- API 依赖图中显示 `$Self` 而不是具体类型 `Counter`
- `print_counter(Counter)` 无法被 `new() → $Self` 调用
- 类型依赖关系不完整

### 根本原因
1. **`impl` 块被过滤掉**：所有 `impl` 块的 `visibility` 在 rustdoc JSON 中为 `"default"` 而不是 `"public"`，导致 `is_public()` 检查失败
2. **方法重复提取**：`impl` 块中的方法在 `krate.index` 中同时作为独立 `Function` 和 `impl.items` 成员存在
3. **类型归一化实现不完整**：未直接使用 `Path.path` 字段获取类型名

### 修复方案
#### 1. 允许处理 `default` 可见性的 `impl` 块
```rust
// src/api_extract.rs: line 102-107
let should_process = is_public(item) || matches!(item.inner, ItemEnum::Impl(_));
```

#### 2. 避免方法重复提取
```rust
// src/api_extract.rs: line 89-95
let mut impl_method_ids = std::collections::HashSet::new();
for item in krate.index.values() {
    if let ItemEnum::Impl(impl_) = &item.inner {
        impl_method_ids.extend(impl_.items.iter().cloned());
    }
}
```

#### 3. 直接使用 `path.path` 字段
```rust
// src/type_norm.rs: line 127-149
Type::ResolvedPath(path) => {
    let type_key = if path.path.is_empty() {
        self.resolve_path_to_key(&path.id)?
    } else {
        // 标准化路径
        if path.path.starts_with("crate::") {
            path.path.clone()
        } else if let Some(item) = self.items.get(&path.id) {
            if item.crate_id == 0 {
                format!("crate::{}", path.path)
            } else {
                path.path.clone()
            }
        } else {
            path.path.clone()
        }
    };
    // ...
}
```

### 测试结果
**修复前**：
- 提取到 10 个 API（重复）
- 5 条依赖边
- 显示 `$Self` 类型

**修复后**：
- 提取到 6 个 API（正确）
- 9 条依赖边
- 所有类型显示为 `crate::Counter`

**详细分析**：参见 `SELF_TYPE_RESOLUTION_FIXED.md`

---

## ✅ Issue 2: rustdoc-types 版本兼容性

**发现时间**：初始实现阶段  
**错误信息**：
```
error[E0432]: unresolved import `rustdoc_types::FnDecl`
error[E0609]: no field `decl` on type `&rustdoc_types::Function`
```

### 根本原因
`rustdoc-types 0.57.0` 将 `FnDecl` 重命名为 `FunctionSignature`，字段从 `decl` 改为 `sig`。

### 修复方案
```rust
// src/api_extract.rs
- use rustdoc_types::FnDecl;
+ use rustdoc_types::FunctionSignature;

- func.decl
+ func.sig
```

---

## ✅ Issue 3: Crate 结构变更

**发现时间**：初始实现阶段  
**错误信息**：
```
error[E0609]: no field `name` on type `rustdoc_types::Crate`
error[E0609]: no field `version` on type `rustdoc_types::Crate`
```

### 根本原因
`rustdoc-types 0.57.0` 将 `Crate.name` 和 `Crate.version` 移除，改为：
- `Crate.root: Id` - 指向根模块
- `Crate.crate_version: Option<String>` - 版本号

### 修复方案
```rust
// src/main.rs
- krate.name
+ krate.index[&krate.root].name

- krate.version
+ krate.crate_version
```

---

## ✅ Issue 4: IndexMap vs HashMap

**发现时间**：初始实现阶段  
**错误信息**：
```
error[E0308]: mismatched types
expected `&IndexMap<Id, Item>`, found `&HashMap<Id, Item>`
```

### 根本原因
`rustdoc-types 0.57.0` 使用 `IndexMap` 代替 `HashMap` 来保持插入顺序。

### 修复方案
```rust
// src/type_norm.rs
- use std::collections::HashMap;
+ use indexmap::IndexMap;

- items: &HashMap<Id, Item>
+ items: &IndexMap<Id, Item>
```

---

## ✅ Issue 5: Path.name 不存在

**发现时间**：API 图实现阶段  
**错误信息**：
```
error[E0609]: no field `name` on type `&rustdoc_types::Path`
```

### 根本原因
`rustdoc_types::Path` 结构的字段名为 `path`，不是 `name`。

### 修复方案
```rust
// src/api_graph.rs
- path.name
+ path.path
```

---

## ✅ Issue 6: BorrowFlag 状态断言错误

**发现时间**：运行时测试阶段  
**错误信息**：
```
thread 'main' panicked at src/model.rs:155:9:
assertion failed: matches!(self, BorrowFlag::MutActive)
```

### 根本原因
`BorrowFlag::clear_mut()` 的断言过于严格，未考虑从 `ShrCount(n)` 状态清除的情况。

### 修复方案
```rust
// src/model.rs
pub fn clear_mut(&mut self) {
-     assert!(matches!(self, BorrowFlag::ShrCount(_)));
+     assert!(matches!(self, BorrowFlag::MutActive));
      *self = BorrowFlag::Free;
}
```

---

## ✅ Issue 7: 缺少 tempfile 依赖

**发现时间**：编译 `emit.rs` 阶段  
**错误信息**：
```
error[E0433]: failed to resolve: use of undeclared crate `tempfile`
```

### 修复方案
```toml
# Cargo.toml
[dependencies]
tempfile = "3.13"
```

---

## ✅ Issue 8: CreatePrimitive 缺少 TypeKey 导入

**发现时间**：添加 `CreatePrimitive` transition 时  
**错误信息**：
```
error[E0425]: cannot find type `TypeKey` in this scope
```

### 修复方案
```rust
// src/transition.rs
+ use crate::model::TypeKey;
```

---

## ✅ Issue 9: CreatePrimitive 使用错误的 VarId

**发现时间**：代码生成测试阶段  
**表现**：生成的代码中变量 ID 不连续（如 `v0`, `v2`，缺少 `v1`）

### 根本原因
`CreatePrimitive` 使用 `state.next_var_id` 创建 `Transition`，但 `next_var_id` 在 `apply` 时才递增，导致 ID 不一致。

### 修复方案
```rust
// src/transition.rs
pub fn apply_transition(...) {
    match trans {
        Transition::CreatePrimitive { type_key, new_id } => {
-             let new_id = state.next_var_id;
-             state.next_var_id += 1;
+             // 使用 transition 中预先分配的 new_id
              state.places.entry(type_key.clone()).or_default().push(Token {
                  id: *new_id,
                  // ...
              });
        }
    }
}
```

---

## 待修复问题

### ⚠️ Issue TODO-1: Trait 实现未检测

**优先级**：高  
**描述**：`Default::default()`, `Clone::clone()` 等 trait 方法未被识别为可用 API  
**计划修复**：在 `api_graph::extract_trait_impls` 中实现

### ⚠️ Issue TODO-2: 引用类型未区分

**优先级**：中  
**描述**：图中未标记 `&T` vs `&mut T` vs `T` 的区别  
**计划修复**：在边标签中添加 `Capability` 信息

### ⚠️ Issue TODO-3: Public 字段未提取

**优先级**：中  
**描述**：结构体的 public 字段未作为可访问的"API"  
**计划修复**：在 `api_graph::extract_public_fields` 中实现

---

## 修复统计

- **已修复**：9 个问题
- **待修复**：3 个问题
- **修改文件**：
  - `src/api_extract.rs` (3 处修复)
  - `src/type_norm.rs` (2 处修复)
  - `src/model.rs` (1 处修复)
  - `src/transition.rs` (2 处修复)
  - `src/main.rs` (1 处修复)
  - `src/api_graph.rs` (1 处修复)
  - `Cargo.toml` (1 处修复)

**最后更新**：2026-01-08
