# Self 类型解析问题修复总结

## 问题描述

原始 API 依赖图中，`Self` 类型没有被正确解析为具体类型 `Counter`，导致：
1. 节点标签显示 `$Self` 而不是 `Counter`
2. 类型依赖关系不完整（如 `print_counter` 无法接收 `new()` 的返回值）
3. `Default` 等 trait 实现未被检测到

## 根本原因

### 原因 1：`impl` 块被过滤掉

**问题**：在 `src/api_extract.rs` 的 `extract_apis` 函数中，所有 items 都经过 `is_public(item)` 检查。Rustdoc JSON 中，`impl` 块的 `visibility` 字段通常是 `"default"` 而不是 `"public"`，导致所有 `impl` 块被跳过。

**示例**（从 `simple_counter.json`）：
```json
{
  "id": "44",
  "name": null,
  "visibility": "default",   // ← 不是 "public"
  "inner": {
    "impl": {
      "for": {
        "resolved_path": {
          "path": "Counter",
          "id": 1
        }
      }
    }
  }
}
```

**修复**：修改 `extract_apis` 逻辑，允许处理 `visibility = "default"` 的 `impl` 块，但仍然检查其内部方法的可见性。

```rust
// 修复前
if !is_public(item) {
    continue;
}

// 修复后
let should_process = is_public(item) || matches!(item.inner, ItemEnum::Impl(_));
if !should_process {
    continue;
}
```

### 原因 2：方法被重复提取

**问题**：`impl` 块中的方法在 `krate.index` 中同时作为：
1. 独立的 `ItemEnum::Function` 条目
2. `impl` 块的 `items` 字段成员

导致每个方法被提取两次，一次没有 `self_type` 上下文（显示为 `$Self`），一次有（显示为 `Counter`）。

**修复**：在提取前，先收集所有 `impl` 块中的方法 ID，提取 `Function` 时跳过这些 ID。

```rust
// 首先收集所有 impl 块中的方法 ID
let mut impl_method_ids = std::collections::HashSet::new();
for item in krate.index.values() {
    if let ItemEnum::Impl(impl_) = &item.inner {
        impl_method_ids.extend(impl_.items.iter().cloned());
    }
}

// 提取 Function 时跳过
ItemEnum::Function(func) => {
    if impl_method_ids.contains(id) {
        continue;  // 让 Impl 处理
    }
    // ... 提取逻辑
}
```

### 原因 3：类型归一化实现不完整

**问题**：在 `src/type_norm.rs` 的 `normalize_type_with_context` 中：
1. `Type::ResolvedPath` 分支直接通过 `id` 查找路径，但构建的路径可能不完整
2. `Type::Generic("Self")` 分支需要 `self_type` 上下文才能替换

**关键发现**（来自用户提示）：
- Rustdoc JSON 中，`Type::ResolvedPath` 包含一个 `Path` 结构：
  ```rust
  pub struct Path {
      pub path: String,  // 例如 "Counter"，这是完整路径！
      pub id: Id,        // Item ID
      pub args: Option<Box<GenericArgs>>,
  }
  ```
- 我们应该**直接使用 `path.path` 字段**，而不是通过 `id` 查找

**修复**：
```rust
Type::ResolvedPath(path) => {
    // 直接使用 path.path 字段
    let type_key = if path.path.is_empty() {
        self.resolve_path_to_key(&path.id)?  // 降级
    } else {
        // 标准化路径：统一使用 crate:: 格式
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

## 修改的文件

### 1. `src/api_extract.rs`
- **行 87-100**：添加 `impl_method_ids` 集合，收集所有 `impl` 块中的方法 ID
- **行 102-107**：修改可见性检查，允许处理 `default` 可见性的 `impl` 块
- **行 117-122**：在 `Function` 分支中跳过 `impl` 方法
- **行 126**：添加调试输出 `"Processing impl block"`

### 2. `src/type_norm.rs`
- **行 126-151**：修改 `Type::ResolvedPath` 处理逻辑，直接使用 `path.path` 字段
- **行 182-192**：改进 `Type::Generic` 处理逻辑，正确使用 `self_type` 上下文替换 `Self`
- 添加调试输出以追踪类型归一化过程

### 3. `src/main.rs` 
- 无需修改（API 提取和图构建逻辑正确）

### 4. `src/api_graph.rs`
- 无需修改（`extract_self_type` 方法本身正确，只是没有接收到正确的 `Self` 类型）

## 测试结果

### 修复前（`api_graph.dot`）
```dot
n0 [label="get\n($Self) → i32", ...];
n2 [label="new\n() → $Self", ...];
n5 [label="increment\n($Self)", ...];
// ...
n2 -> n5 [label="$Self", color="black"];
// print_counter 不可达
```

### 修复后（`api_graph_fixed.dot`）
```dot
n0 [label="print_counter\n(Counter)", ...];
n1 [label="new\n() → Counter", ...];
n2 [label="increment\n(Counter)", ...];
// ...
n1 -> n0 [label="Counter", color="black"];
n1 -> n2 [label="Counter", color="black"];
// 所有类型都正确，依赖完整
```

**关键改进**：
- ✅ `Self` 全部解析为 `crate::Counter`
- ✅ API 数量从 10 个（重复）减少到 6 个（正确）
- ✅ 依赖边从 5 条增加到 9 条（更完整）
- ✅ `print_counter` 现在可以接收 `new()` 和 `create_counter_with_value()` 的返回值

## 关键技术点

### 1. Rustdoc JSON 中的 `impl` 块结构
```json
{
  "inner": {
    "impl": {
      "for": {
        "resolved_path": {
          "path": "Counter",  // ← 这是具体类型名！
          "id": 1
        }
      },
      "items": [2, 3, 4],  // 方法 ID 列表
      "trait": null        // 或 trait 引用
    }
  }
}
```

### 2. `Type` 枚举的两种形式
- **`Type::ResolvedPath`**：用于 `impl_.for_` 和参数类型（具体类型）
  - 直接包含 `path.path` 字段，值为类型名（如 `"Counter"`）
- **`Type::Generic`**：用于方法返回值和 `self` 类型（泛型占位符）
  - 值为 `"Self"` 字符串，需要替换为 `impl_.for_` 的类型

### 3. 类型归一化的上下文传递
```
extract_apis
  ├─ normalize_type(&impl_.for_)  →  获取 self_type
  └─ extract_function_signature(..., self_type)
       └─ normalize_type_with_context(&param_ty, self_type)  →  替换 Self
```

## 未来改进

1. **Trait 实现检测**：当前未检测 `Default`、`Clone` 等 trait 实现，需要在 `api_graph.rs` 中增强 `extract_trait_impls`
2. **Public 字段访问**：未提取结构体的 public 字段，需要实现 `extract_public_fields`
3. **引用类型标记**：图中未标记 `&T` vs `&mut T` vs `T` 的区别，应在边标签中添加 capability 信息

## 参考

- Rustdoc JSON 格式：https://doc.rust-lang.org/nightly/nightly-rustc/rustdoc_json_types/
- Rustdoc Types 源码：`~/.cargo/registry/src/.../rustdoc-types-0.57.0/src/lib.rs`
- 用户关键提示："`for` 的是这个类型，这个在 rustdoc_types里都有定义了，找到 id 即可！"
