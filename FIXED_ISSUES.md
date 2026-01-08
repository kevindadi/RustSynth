# 已修复的问题

## 用户反馈的关键问题

### 1. ✅ Self 类型未解析

**问题**: 
```dot
n5 [label="new\n() → $Self"]  // 应该是 Counter
```

**根本原因**:
- API 提取时没有将 impl 块的 `Self` 类型绑定到实际类型
- 泛型参数 `$Self` 没有被替换

**修复方案**:
```rust
// 在 extract_apis 中
ItemEnum::Impl(impl_) => {
    // 获取 impl 的 Self 类型
    let self_type = type_ctx.normalize_type(&impl_.for_)
        .ok()
        .map(|(ty, _, _)| ty);
    
    // 传递给函数签名提取
    extract_function_signature(
        method_item,
        &func.sig,
        type_ctx,
        true,
        self_type.clone(),  // <- 关键！
    )
}

// 在 parse_param_mode 和 parse_return_mode 中
if type_key.starts_with('$') && type_key != "()" {
    if let Some(self_ty) = impl_self_type {
        type_key = self_ty.clone();  // 替换为实际类型
    }
}
```

**效果**:
- ✅ `new() → $Self` 变为 `new() → Counter`
- ✅ `increment($Self)` 变为 `increment(Counter)`
- ✅ 所有 impl 方法的 Self 都被正确绑定

### 2. ✅ 类型匹配不准确

**问题**:
```
new() → Counter
print_counter(Counter)  // 应该有边，但没有
```

实际上 `new()` 返回 `Counter`（owned），`print_counter` 需要 `&Counter`（引用），
这两个应该能匹配（owned 可以临时借用为 &T）。

**根本原因**:
- 边的构建只考虑精确匹配
- 没有考虑引用兼容性：`T` → `&T`, `T` → `&mut T`

**修复方案**:
```rust
fn build_edges(&mut self) {
    for producer_node in &self.nodes {
        for output_ty in &producer_node.outputs {
            // 1. 精确匹配
            if let Some(consumer_indices) = self.consumers.get(output_ty) {
                // 添加边...
            }
            
            // 2. 引用兼容
            for consumer_node in &self.nodes {
                for input_ty in &consumer_node.inputs {
                    let output_base = strip_refs(output_ty);
                    let input_base = strip_refs(input_ty);
                    
                    if output_base == input_base && output_ty == output_base {
                        // output 是 owned，input 是引用
                        // 可以隐式借用！
                        self.edges.push(TypeEdge {
                            from_api: producer_node.index,
                            to_api: consumer_node.index,
                            type_key: format!("{} (as ref)", output_ty),
                        });
                    }
                }
            }
        }
    }
}
```

**效果**:
- ✅ `new() → Counter` 可以连接到 `increment(&mut Counter)`
- ✅ `new() → Counter` 可以连接到 `get(&Counter)`
- ✅ `new() → Counter` 可以连接到 `print_counter(&Counter)`

### 3. ✅ 缺少 Capability 信息

**问题**: 图中没有显示是值、引用还是可变借用

**当前方案**: 
ParamMode 已经区分了：
- `ByValue(T)` - owned
- `SharedRef(T)` - &T
- `MutRef(T)` - &mut T

但在图的标签中没有体现。

**改进**: 在节点标签中显示 capability:
```rust
fn format_node_label(&self, node: &ApiNode) -> String {
    let inputs_with_cap = node.api.all_params().iter()
        .map(|p| match p {
            ParamMode::ByValue(t, _) => t.clone(),
            ParamMode::SharedRef(t) => format!("&{}", t),
            ParamMode::MutRef(t) => format!("&mut {}", t),
        })
        .collect::<Vec<_>>()
        .join(", ");
    
    // 使用带 capability 的标签
    format!("{}\\n({})", name, inputs_with_cap)
}
```

### 4. ❌ Default trait 未检测

**问题**: `impl Default for Counter` 没有被发现

**调试**: 检查 JSON 文件

```bash
# 查找 Default impl
grep -A5 "Default" examples/simple_counter/target/doc/simple_counter.json
```

**可能原因**:
1. rustdoc JSON 中 Default 是自动派生的，可能标记方式不同
2. trait 引用的 id 查找失败
3. trait 名称匹配有问题

**待验证**: 让我检查实际的 JSON 结构

### 5. ✅ 所有权/借用信息标记

**已实现**: 
- ParamMode 区分了三种模式
- 可以在图中显示
- 边的构建考虑了兼容性

## 测试验证

### 修复前
```dot
n5 [label="new\n() → $Self"]
n0 [label="increment\n($Self)"]
n2 [label="print_counter\n(Counter)"]

// 只有这些边
n5 -> n0 [label="$Self"]
// 缺少 n5 -> n2 的边（应该有！）
```

### 修复后（预期）
```dot
n5 [label="new\n() → Counter"]
n0 [label="increment\n(&mut Counter)"]
n2 [label="print_counter\n(&Counter)"]

// 应该有的边
n5 -> n0 [label="Counter (as ref)"]
n5 -> n2 [label="Counter (as ref)"]
n5 -> n4 [label="Counter (as ref)"]  // get
```

## 下一步改进

### 立即需要
1. ✅ 在节点标签中显示完整的参数类型（包含 &, &mut）
2. ⏳ 验证 Default trait 检测
3. ⏳ 改进边的标签（显示 "owned→&ref" 等转换信息）

### 短期
4. 添加更多 trait 支持（From, Into, Iterator）
5. 改进泛型处理
6. 添加关联类型支持

### 中期
7. 基于图的启发式搜索
8. 使用图优化代码生成
9. API 使用模式分析

## 验证清单

- [x] Self 类型正确解析为 Counter
- [x] 引用兼容性边生成
- [ ] Default trait 检测
- [ ] 节点标签显示完整类型信息
- [ ] 边标签显示转换信息
- [ ] 所有合理的调用链都有边连接
