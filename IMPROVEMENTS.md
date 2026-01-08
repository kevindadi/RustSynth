# 改进说明

## 已实现的改进

### 1. 最小深度要求 ✅
- 之前：找到任何有 token 的封闭状态就停止
- 现在：要求至少 3 步深度（除非指定了 target_type）
- 效果：避免过早停止，鼓励探索更长轨迹

### 2. Primitive 常量支持 ✅
- 新增 `CreatePrimitive` structural transition
- 支持：`i32`, `u32`, `i64`, `u64`, `bool`, `usize`
- 每种类型限制创建 1 个（避免组合爆炸）
- 代码生成：`let x0: i32 = 0;`

### 3. 深度信息输出 ✅
- 在日志中显示找到目标时的深度
- 帮助调试和理解搜索过程

## 当前观察到的问题

### 问题 1: 重复调用相同 API
**现象**: 生成 3 次 `new()` 调用
```rust
let x0 = new();
let x1 = new();
let x2 = new();
```

**原因**:
1. `new()` 是无参数函数，总是 enabled
2. 搜索算法是 BFS，优先探索浅层
3. 没有鼓励使用已有 token 的机制

**建议改进方案**:

#### 方案 A: 搜索启发式
- 优先级队列替代普通队列
- 使用已有 token 的 transition 优先级更高
- 创建新 token 的 transition 优先级较低

#### 方案 B: 多样性奖励
- 跟踪已调用过的 API
- 对新的 API 调用给予更高优先级
- 避免重复调用相同函数

#### 方案 C: Token 使用率
- 目标函数增加"token 使用率"指标
- 优先选择使用更多现有 token 的路径
- 鼓励 `increment(&mut x0)`, `get(&x0)` 等调用

### 问题 2: 没有生成完整的使用链
**期望轨迹**:
```rust
let counter = Counter::new();
counter.increment();
let val = counter.get();
print_counter(&counter);
```

**当前只能达到**:
```rust
let x0 = Counter::new();
let x1 = Counter::new();
let x2 = Counter::new();
```

**根本原因**:
- BFS 找到第一个满足条件的解就停止
- 没有"最优解"的概念
- 缺少对"有意义轨迹"的定义

**建议改进方案**:

#### 方案 A: 多目标搜索
- 不要在第一个解就停止
- 继续搜索，收集多个候选解
- 按某种度量（API 多样性、token 使用率）排序
- 返回"最好的"解

#### 方案 B: 目标函数改进
```rust
fn goal_quality(state: &State, trace: &[Transition]) -> f64 {
    let mut score = 0.0;
    
    // 1. API 多样性（调用不同的函数）
    let unique_apis = trace.iter().filter_map(|t| match t {
        Transition::ApiCall(call) => Some(&call.api.full_path),
        _ => None,
    }).collect::<HashSet<_>>().len();
    score += unique_apis as f64 * 10.0;
    
    // 2. Token 使用率（使用已有的 token）
    let token_reuses = count_token_reuses(trace);
    score += token_reuses as f64 * 5.0;
    
    // 3. 轨迹长度（更长更好，但有上限）
    score += (trace.len() as f64).min(10.0);
    
    // 4. 惩罚重复调用
    let repetitions = count_repetitions(trace);
    score -= repetitions as f64 * 20.0;
    
    score
}
```

#### 方案 C: API 图预分析
正如你建议的，先构建 API 图：
```
1. 分析所有 API 的输入输出类型
2. 构建类型依赖图
3. 识别"入口" API (无参数或只需 primitive)
4. 识别"链式" API (使用其他 API 的输出)
5. 搜索时优先探索有意义的调用链
```

示例 API 图：
```
Counter::new() -> Counter
    ↓
Counter::increment(&mut Counter) -> ()
    ↓
Counter::get(&Counter) -> i32
    ↓
print_counter(&Counter) -> ()
    
create_counter_with_value(i32) -> Counter
```

### 问题 3: 无法处理方法调用语法
**当前生成**:
```rust
Counter::increment(&mut x0);
```

**期望生成**:
```rust
x0.increment();
```

**解决方案**: 在 emit.rs 中检测 self 参数，生成方法调用语法

## 下一步行动计划

### 立即可做 (1-2 小时)
1. ✅ 添加 primitive 常量支持
2. ✅ 增加最小深度要求
3. 🔄 多候选解搜索（找前 N 个解，选最好的）
4. 🔄 方法调用语法生成

### 短期 (1 天)
5. 实现 API 多样性奖励
6. 实现 token 使用率度量
7. 添加重复调用惩罚
8. 改进目标函数

### 中期 (3-5 天)
9. 构建 API 依赖图
10. 实现启发式搜索（A*）
11. 支持更多 structural transitions (reborrow, deref)
12. Trait 实例化和关联类型

### 长期 (1-2 周)
13. 完整的类型图分析
14. 路径约束求解
15. 符号执行集成
16. 智能测试用例生成

## 你提出的关键见解

> "而且还可能存在一些 constant，已经被定义好的类型"
✅ 已添加 primitive 常量支持

> "复合类型内部还是有 const 方法，能够直接发生，不需要库所的"
📝 好想法！const 方法（不需要 self）可以直接调用。需要从 rustdoc 提取 const 标记。

> "你要不先构建一个 api graph？"
📝 非常好的建议！这能大大提高搜索质量：
  - 识别 API 间的类型依赖
  - 发现合理的调用链
  - 避免无意义的搜索路径

> "所有 trait 都是实例化，内部对应的关联类型也被实例化？"
📝 重要！当前实现忽略了 trait:
  - `Default::default()` 应该能创建很多类型
  - `Clone::clone()` 应该能复制 token
  - 关联类型需要展开（如 `Iterator::Item`）
  
需要添加：
1. 提取 trait impls from rustdoc
2. 为每个 impl 生成 transition
3. 处理关联类型

## 总结

你的分析非常准确！当前实现的主要问题：
1. **搜索过早停止** - ✅ 已通过最小深度要求缓解
2. **缺少 primitive 支持** - ✅ 已添加
3. **无 API 图** - ⏳ 待实现，这是关键改进
4. **忽略 trait** - ⏳ 待实现
5. **目标函数过于简单** - ⏳ 需要加入质量度量

建议优先实现：API 图 + 多候选解搜索 + 质量度量
