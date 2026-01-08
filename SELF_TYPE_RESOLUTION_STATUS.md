# Self 类型解析状态报告

## 用户反馈的核心问题

你完全正确地指出了以下关键问题：

### 1. ❌ Self 类型未解析
```
当前: new() → $Self
期望: new() → Counter
```

### 2. ❌ 类型匹配不准确  
```
问题: Counter (owned) 应该能匹配 &Counter 参数
现状: new() → Counter 没有连接到 print_counter(&Counter)
```

### 3. ❌ 缺少借用信息标记
```
需要: 明确显示是值、&T、还是 &mut T
当前: 标签中没有显示借用类型
```

### 4. ❌ Default trait 未检测
```
impl Default for Counter { fn default() -> Self }
应该被检测并生成相应的 API 节点
```

## 已实现的代码修改

### 修改 1: TypeContext 添加带上下文的归一化

```rust
// src/type_norm.rs
pub fn normalize_type_with_context(
    &self,
    ty: &Type,
    self_type: Option<&TypeKey>,
) -> Result<(TypeKey, Capability, bool)> {
    match ty {
        Type::Generic(name) => {
            if name == "Self" {
                if let Some(self_ty) = self_type {
                    let is_copy = self.copy_types.contains(self_ty);
                    return Ok((self_ty.clone(), Capability::Own, is_copy));
                }
            }
            Ok((format!("${}", name), Capability::Own, false))
        }
        // ...
    }
}
```

### 修改 2: API 提取时获取 impl Self 类型

```rust
// src/api_extract.rs
ItemEnum::Impl(impl_) => {
    // 获取 impl 的 Self 类型
    let self_type = type_ctx.normalize_type(&impl_.for_)
        .ok()
        .map(|(ty, _, _)| ty);
    
    // 传递给方法提取
    extract_function_signature(
        method_item,
        &func.sig,
        type_ctx,
        true,
        self_type.clone(),  // <- 关键！
    )
}
```

### 修改 3: 参数/返回值解析使用上下文

```rust
// src/api_extract.rs
fn parse_param_mode(..., impl_self_type: Option<&TypeKey>) -> Result<ParamMode> {
    let (type_key, cap, is_copy) = 
        type_ctx.normalize_type_with_context(ty, impl_self_type)?;
    // ...
}
```

### 修改 4: 边构建考虑引用兼容性

```rust
// src/api_graph.rs
fn build_edges(&mut self) {
    // 1. 精确匹配
    // ...
    
    // 2. 引用兼容：owned T 可以传给 &T
    for consumer_node in &self.nodes {
        for input_ty in &consumer_node.inputs {
            let output_base = strip_refs(output_ty);
            let input_base = strip_refs(input_ty);
            
            if output_base == input_base && output_ty == output_base {
                // owned 传给引用！
                self.edges.push(TypeEdge {
                    from_api: producer_node.index,
                    to_api: consumer_node.index,
                    type_key: format!("{} (as ref)", output_ty),
                });
            }
        }
    }
}
```

## 为什么还是 `$Self`？

### 可能的原因

#### 原因 1: impl_.for_ 本身是 Generic
如果 JSON 中 `impl_.for_` 的类型就是 `Generic("Self")`，那么：
```rust
let self_type = type_ctx.normalize_type(&impl_.for_)  // 返回 "$Self"
    .ok()
    .map(|(ty, _, _)| ty);  // Some("$Self")
```

这样传递的还是 `$Self`，没有解决问题！

**解决方案**: 需要从 `impl` 块的上下文获取实际类型名，而不是通过 normalize `impl_.for_`。

```rust
// 正确的方法：从 Item 的 name 或 path 获取
let self_type = if let Some(path) = type_ctx.id_to_path.get(&id) {
    Some(path.clone())
} else {
    None
};
```

#### 原因 2: impl_ 是 inherent impl，不是 trait impl
对于 `impl Counter { ... }`，rustdoc 的 `impl_.for_` 应该指向 `Counter` 的 `ResolvedPath`。

需要检查：
1. `impl_.for_` 的实际类型（是 ResolvedPath 还是 Generic）
2. 如果是 ResolvedPath，是否正确解析了 ID

#### 原因 3: 类型解析路径错误
可能 `type_ctx.id_to_path` 中没有 Counter 的正确路径映射。

## 调试步骤

### 步骤 1: 检查 JSON 结构

```bash
cd examples/simple_counter
# 查找 Counter 的定义
jq '.index | to_entries | .[] | select(.value.name == "Counter")' target/doc/simple_counter.json

# 查找 impl 块
jq '.index | to_entries | .[] | select(.value.inner.impl != null)' target/doc/simple_counter.json | head -50
```

### 步骤 2: 添加详细调试输出

```rust
ItemEnum::Impl(impl_) => {
    eprintln!("=== Impl Block ===");
    eprintln!("For type: {:?}", impl_.for_);
    
    let self_type = match &impl_.for_ {
        Type::ResolvedPath(path) => {
            eprintln!("ResolvedPath id: {:?}", path.id);
            type_ctx.id_to_path.get(&path.id)
                .cloned()
                .or_else(|| Some("Counter".to_string()))
        }
        Type::Generic(name) => {
            eprintln!("Generic: {}", name);
            Some(format!("${}", name))
        }
        other => {
            eprintln!("Other type: {:?}", other);
            None
        }
    };
    
    eprintln!("Resolved self_type: {:?}", self_type);
    // ...
}
```

### 步骤 3: 直接硬编码测试

临时修复：
```rust
let self_type = if item.name.as_deref() == Some("Counter") {
    Some("crate::Counter".to_string())
} else {
    type_ctx.normalize_type(&impl_.for_)
        .ok()
        .map(|(ty, _, _)| ty)
};
```

## 完整的修复方案

基于对问题的深入理解，完整的修复需要：

### 方案 A: 改进 impl_.for_ 的解析

```rust
fn resolve_impl_self_type(
    impl_: &Impl,
    type_ctx: &TypeContext,
) -> Option<TypeKey> {
    match &impl_.for_ {
        Type::ResolvedPath(path) => {
            // 从 path.id 查找类型名
            type_ctx.id_to_path.get(&path.id).cloned()
        }
        Type::Generic(name) if name == "Self" => {
            // 对于 Generic("Self")，需要从外层上下文获取
            // 这种情况不应该出现在 inherent impl 中
            None
        }
        _ => {
            // 其他类型正常 normalize
            type_ctx.normalize_type(&impl_.for_)
                .ok()
                .map(|(ty, _, _)| ty)
        }
    }
}
```

### 方案 B: 改进类型路径构建

在 `type_norm.rs` 的 `build_item_path` 中：

```rust
fn build_item_path(item: &Item, items: &HashMap<Id, Item>) -> Result<String> {
    if let Some(name) = &item.name {
        // 尝试构建完整路径
        let full_path = if let Some(parent_id) = find_parent_module(item, items) {
            format!("{}::{}", parent_id, name)
        } else {
            format!("crate::{}", name)
        };
        return Ok(full_path);
    }
    // ...
}
```

### 方案 C: 节点标签改进

显示借用类型：

```rust
fn format_param_with_cap(param: &ParamMode) -> String {
    match param {
        ParamMode::ByValue(t, _) => t.clone(),
        ParamMode::SharedRef(t) => format!("&{}", t),
        ParamMode::MutRef(t) => format!("&mut {}", t),
    }
}

fn format_node_label(&self, node: &ApiNode) -> String {
    let inputs = node.api.all_params().iter()
        .map(format_param_with_cap)
        .join(", ");
    
    let output = match &node.api.return_mode {
        ReturnMode::OwnedValue(t, _) => t.clone(),
        ReturnMode::SharedRef(t) => format!("&{}", t),
        ReturnMode::MutRef(t) => format!("&mut {}", t),
        ReturnMode::Unit => "()".to_string(),
    };
    
    if output == "()" {
        format!("{}\\n({})", name, inputs)
    } else {
        format!("{}\\n({}) → {}", name, inputs, output)
    }
}
```

## 下一步行动

### 立即需要（紧急）

1. **调试 impl_.for_ 的实际类型**
   - 打印 JSON 中 impl 块的结构
   - 确认是 ResolvedPath 还是 Generic
   - 检查 id_to_path 映射

2. **测试 resolve 逻辑**
   - 添加详细日志
   - 验证 Self 类型传递
   - 确认 normalize_type_with_context 被调用

3. **修复并验证**
   - 根据调试结果修复
   - 重新生成图
   - 确认 Counter 显示正确

### 短期改进

4. **节点标签显示借用类型**
5. **Default trait 检测**
6. **引用兼容边的标签改进**

## 总结

核心问题是 `impl_.for_` 的类型解析。需要通过调试确定：
- `impl_.for_` 在 JSON 中的实际结构
- `id_to_path` 映射是否正确
- `normalize_type` 是否正确处理 ResolvedPath

一旦确认了这些，修复就会很直接。你的分析完全正确，这些问题必须解决才能生成有意义的依赖图！🎯
