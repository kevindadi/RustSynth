# Token 和生命周期系统 - 实现进展总结

## 📅 更新时间

2026-01-12 23:23

---

## ✅ 已完成的核心功能

### 1. Token 结构扩展 ✅

**文件**: `src/pcpn.rs` (行 287-338)

```rust
pub struct Token {
    pub id: TokenId,                    // 唯一标识
    pub type_key: TypeKey,              // 类型
    pub capability: Capability,         // Own/Shr/Mut
    pub borrowed_from: Option<TokenId>, // 借用源
    pub ref_level: usize,               // 引用层级：0=T, 1=&T, 2=&&T
    pub lifetime: Option<String>,       // 生命周期标识
}
```

**关键特性**：

- ✅ **ref_level 追踪**：区分值（0）、一级引用（1）、多级引用（2+）
- ✅ **lifetime 标识**：用于生命周期绑定
- ✅ **借用关系**：通过 `borrowed_from` 追踪源 token

**Token 操作方法**：

```rust
// 创建值
Token::new_owned(id, type_key)

// 创建一级引用 &T
Token::borrow_shr(id, type_key, from_id, lifetime)
Token::borrow_mut(id, type_key, from_id, lifetime)

// 创建多级引用 &&T
token.add_ref_level(new_id)

// 解引用 &&T → &T
token.deref(new_id)
```

### 2. 生命周期栈系统 ✅

**文件**: `src/pcpn.rs` (行 182-230)

```rust
pub struct LifetimeStack {
    frames: Vec<LifetimeFrame>,
}

pub struct LifetimeFrame {
    lifetime: String,         // 生命周期标识
    borrows: Vec<TokenId>,    // 这个生命周期内的借用
    blocks: Vec<TokenId>,     // 被阻塞的源 token（不能 drop）
}
```

**关键操作**：

- ✅ `push_frame(lifetime)` - 进入新作用域（如函数调用）
- ✅ `pop_frame()` - 离开作用域，返回需要释放的借用
- ✅ `add_borrow(lifetime, borrow_id, source_id)` - 记录借用关系
- ✅ `is_blocked(token_id)` - 检查 token 是否被借用阻塞

**语义**：

- 当函数返回引用时，将借用压栈
- 返回的引用 drop 时弹栈
- 被借用的 token 在栈帧存在期间被阻塞（不能 drop 或可变操作）

### 3. SimState 重构 ✅

**文件**: `src/simulator.rs` (行 68-158)

```rust
pub struct SimState {
    marking: HashMap<PlaceId, Vec<Token>>, // 存储实际 Token 实例（不是数量）
    next_token_id: TokenId,                // 下一个可用 Token ID
    dup_count: HashMap<TypeKey, usize>,
    lifetime_stack: LifetimeStack,         // 生命周期管理
}
```

**关键方法**：

- ✅ `add_token(token, place)` - 添加 Token 实例
- ✅ `remove_token(place)` - 移除 Token 实例
- ✅ `count(place)` - 获取 token 数量
- ✅ `hash_key()` - 状态哈希（用于去重）

### 4. 解引用变迁 ✅

**文件**: `src/pcpn.rs` (行 997-1009)

```rust
// Deref: P[T, shr] → P[T, shr] (解引用：&&T → &T)
self.add_transition(
    format!("deref({})", short_name),
    TransitionKind::DerefRef { inner_type: base_type.clone() },
    vec![Arc { place_id: shr_place, consumes: true, ... }],
    vec![Arc { place_id: shr_place, consumes: false, ... }],
    vec![],
);
```

**语义**：

- 输入：从 shr place 取出 ref_level=n 的 token
- 输出：产生 ref_level=n-1 的 token
- 例如：&&Counter → &Counter

### 5. Fire 函数重写 ✅

**文件**: `src/simulator.rs` (行 465-557)

**已实现的特殊处理**：

#### BorrowMut/BorrowShr

```rust
TransitionKind::BorrowMut { base_type, .. } => {
    // 从 own place 取 source_token
    // 创建 borrow_token (ref_level=1, borrowed_from=source_id)
    // 放入 mut place
}
```

#### EndBorrowMut/EndBorrowShr

```rust
TransitionKind::EndBorrowMut { .. } => {
    // 从 mut place 取 borrow_token
    // 恢复 source_token 到 own place
    // TODO: 从生命周期栈弹出
}
```

#### DerefRef

```rust
TransitionKind::DerefRef { .. } => {
    // 从 shr place 取 ref_token
    // 调用 token.deref(new_id) 降低 ref_level
    // 放回 shr place
}
```

#### CreatePrimitive

```rust
TransitionKind::CreatePrimitive { type_key } => {
    // 创建新的 owned token (ref_level=0)
    // 放入对应 own place
}
```

### 6. 参数传递语义修正 ✅

**文件**: `src/pcpn.rs` (行 772-780)

```rust
let consumes = match capability {
    Capability::Own => true,  // 传递所有权：消耗 token
    Capability::Shr => false, // 共享引用：不消耗（可多个同时持有）
    Capability::Mut => false, // 可变借用：不消耗（独占性由 Guard 保证）
};
```

**Copy 类型自动复制** ✅:

```rust
// API 调用时，Copy 类型的 Own 参数自动添加输出弧
if param_type.is_copy() && capability == Capability::Own {
    output_arcs.push(Arc {
        place_id,
        annotation: Some(ArcAnnotation::ReturnArc),
    });
}
```

---

## 🔄 当前进展

### PCPN 生成统计

```
PCPN: 6 places (2 base types × 3 capabilities)
      19 transitions:
        - 6 API 调用
        - 11 结构性变迁 (borrow×2, end_borrow×2, deref×2, drop×2, const×1)
```

### 变迁列表

```
API 调用:
  - Counter::new
  - Counter::increment (&mut self)
  - Counter::reset (&mut self)
  - Counter::get (&self) -> i32
  - create_counter_with_value (i32)
  - print_counter (&Counter)

结构性变迁:
  - borrow_mut(Counter)     [Own → Mut]
  - end_borrow_mut(Counter) [Mut → Own]
  - borrow_shr(Counter)     [Own → Shr]
  - end_borrow_shr(Counter) [Shr → Own]
  - deref(Counter)          [&&Counter → &Counter] ✨新增
  - drop(Counter)

  - borrow_mut(i32)
  - end_borrow_mut(i32)
  - borrow_shr(i32)
  - end_borrow_shr(i32)
  - deref(i32)              ✨新增
  - drop(i32)
  - const_i32               [创建基本类型]
```

### 可达图统计

```
状态数: 20
转换数: 41
初始状态: s0 [∅]
```

---

## ⚠️ 待完成的功能

### A. API 调用时的生命周期绑定 🔜

**需求**：

- 当 API 返回引用时（如 `get(&self) -> i32`），需要：
  1. 提取返回值的生命周期（通常与 `&self` 绑定）
  2. 调用 `lifetime_stack.add_borrow(lifetime, return_token_id, self_token_id)`
  3. 压栈记录这个借用关系

**实现位置**: `simulator.rs` fire 函数中的 `TransitionKind::ApiCall`

**代码草图**:

```rust
TransitionKind::ApiCall { fn_id } => {
    // 检查是否返回引用
    if has_lifetime_in_return {
        // 提取生命周期
        let lifetime = extract_lifetime_from_signature();
        // 找到被引用的 token
        let source_token_id = find_source_from_params();
        // 压栈
        new_state.lifetime_stack.add_borrow(lifetime, return_token_id, source_token_id);
    }
}
```

### B. 生命周期作用域管理 🔜

**需求**：

- 函数调用开始：`lifetime_stack.push_frame(fn_lifetime)`
- 函数调用结束：`lifetime_stack.pop_frame()`
- 弹栈时释放所有相关借用

**挑战**：

- 当前模型是 Petri 网，没有显式的函数调用栈
- 需要设计一种机制来模拟函数作用域

**可能方案**：

1. 为每个函数添加 `enter_fn` 和 `exit_fn` 变迁
2. 使用额外的 place 来表示当前执行上下文
3. 在 API 调用时自动管理生命周期栈

### C. 阻塞检查 🔜

**需求**：

- Drop 前检查：`if lifetime_stack.is_blocked(token_id) { 禁止 drop }`
- 可变操作前检查：`if lifetime_stack.is_blocked(token_id) { 禁止可变借用 }`

**实现位置**:

1. `guard_check` 函数 - 添加新的 GuardKind
2. Fire 函数 - 执行前检查

**代码草图**:

```rust
// 在 Drop 变迁的 Guard 中
GuardKind::RequireNotBorrowed => {
    if new_state.lifetime_stack.is_blocked(token_id) {
        return false; // 被借用，不能 drop
    }
}
```

### D. 多级引用测试用例 🔜

**需要创建测试示例**：

```rust
pub fn test_double_ref(x: &Counter) -> &&Counter {
    &x  // 返回 &&Counter
}

pub fn deref_double(x: &&Counter) -> &Counter {
    *x  // 解引用
}
```

---

## 📊 实现完成度

| 功能模块             | 完成度 | 状态          |
| -------------------- | ------ | ------------- |
| **Token 结构**       | 100%   | ✅ 完成       |
| **生命周期栈**       | 100%   | ✅ 完成       |
| **SimState 重构**    | 100%   | ✅ 完成       |
| **解引用变迁**       | 100%   | ✅ 完成       |
| **Fire 基础逻辑**    | 80%    | ✅ 大部分完成 |
| **API 生命周期绑定** | 0%     | ⚠️ 待实现     |
| **作用域管理**       | 0%     | ⚠️ 待实现     |
| **阻塞检查**         | 0%     | ⚠️ 待实现     |
| **多级引用测试**     | 0%     | ⚠️ 待实现     |

**总体进度**: ~70%

---

## 🎯 核心改进总结

### 修复的 Bug

1. ✅ **Copy 类型语义**：不消耗 token，通过输出弧自动复制
2. ✅ **引用传递**：Shr/Mut 不消耗 token（不发生 drop）
3. ✅ **删除抑制弧**：所有弧都是 solid
4. ✅ **删除 mut_to_shr**：Rust 中不存在降权操作
5. ✅ **删除 dup 变迁**：Copy 通过输出弧实现
6. ✅ **Guard 检查**：强制 Rust 借用规则

### 新增功能

1. ✅ **Token 实例管理**：从数量统计升级为实例跟踪
2. ✅ **引用层级**：支持 &T, &&T, &&&T...
3. ✅ **解引用操作**：&&T → &T
4. ✅ **生命周期栈**：栈帧、压栈、弹栈、阻塞检查
5. ✅ **基本类型持续生成**：const_i32 变迁，上限 3 个

---

## 📋 详细 TODO List

### Phase 1: 核心架构 ✅ (已完成)

- [x] 扩展 Token 结构：添加 ref_level 和 lifetime
- [x] 实现生命周期栈：LifetimeStack 和 LifetimeFrame
- [x] 重构 SimState：从数量到 Token 实例
- [x] 添加解引用变迁：deref(T)
- [x] 重写 fire 函数：处理 BorrowMut/Shr, EndBorrow, Deref

### Phase 2: 生命周期绑定 ⚠️ (进行中)

- [x] 实现借用规则的 Guard 检查
- [ ] **API 调用时绑定生命周期**：返回引用压栈
- [ ] 提取函数签名中的生命周期信息
- [ ] 将返回的引用 token 与参数 token 绑定

### Phase 3: 作用域管理 ⚠️ (待实现)

- [ ] 设计函数作用域表示方法
- [ ] 添加 enter_fn 和 exit_fn 变迁（可选）
- [ ] 自动管理生命周期栈的压栈/弹栈
- [ ] 处理嵌套函数调用

### Phase 4: 阻塞检查 ⚠️ (待实现)

- [ ] 添加 GuardKind::RequireNotBorrowed
- [ ] Drop 变迁前检查 is_blocked
- [ ] BorrowMut 前检查源 token 是否被借用
- [ ] 实现 "返回引用禁用被引用的可变操作"

### Phase 5: 测试和验证 ⚠️ (待实现)

- [ ] 创建多级引用测试用例（&&T）
- [ ] 测试解引用操作
- [ ] 测试生命周期绑定
- [ ] 验证借用阻塞规则
- [ ] 生成可达图分析

---

## 🔍 关键验证

### 当前可达图行为

```
路径 1: 基本类型生成和使用
s0 [∅] → [const_i32] → s2 [i32[own]:1]
s2 → [create_counter_with_value] → s6 [i32[own]:1, Counter[own]:1]  ✅ Copy 不消耗

路径 2: 可变借用（状态不变）
s1 [Counter[own]:1] → [borrow_mut] → s4 [Counter[mut]:1]
s4 → [Counter::increment] → s4 [Counter[mut]:1]  ✅ Mut 不消耗

路径 3: 共享借用（状态不变）
s1 [Counter[own]:1] → [borrow_shr] → s5 [Counter[shr]:1]
s5 → [print_counter] → s5 [Counter[shr]:1]  ✅ Shr 不消耗
s5 → [Counter::get] → s15 [Counter[shr]:1, i32[own]:1]  ✅ 返回值

路径 4: Drop 正确消耗
s2 [i32:1] → [drop(i32)] → s0 [∅]  ✅ Own 消耗
```

### 借用规则验证

```bash
$ grep "Counter\[mut\].*Counter\[shr\]" reachability.dot
# 结果：No matches found ✅ 没有 mut 和 shr 共存
```

---

## 📁 生成文件

```
test_output/token_system/
├── apigraph.dot       ✅ API Graph
├── pcpn.dot           ✅ 19 transitions (含 deref)
├── pcpn.png           ✅ 195KB
├── reachability.dot   ✅ 20 states, 41 edges
└── reachability.png   ✅ 219KB
```

---

## 🚀 下一步优先级

### 高优先级（核心功能）

1. **实现 API 调用的生命周期绑定** 🔥

   - 解析函数签名中的生命周期
   - 返回引用时压栈
   - 绑定返回值和参数

2. **实现阻塞检查** 🔥
   - Drop 前检查 is_blocked
   - 可变操作前检查是否被借用

### 中优先级（增强功能）

3. **生命周期作用域管理**

   - 函数调用栈
   - 自动压弹栈

4. **多级引用测试**
   - 创建 &&T 测试用例
   - 验证 deref 操作

### 低优先级（优化）

5. 性能优化
6. 更详细的错误信息
7. 文档和示例

---

## 💡 技术要点

### Token 的生命周期

```
创建 → 借用 → 使用 → 归还 → Drop
  ↓      ↓             ↓      ↓
Own   Shr/Mut      不消耗   Own
```

### 引用层级示例

```
let x: Counter;           // ref_level=0
let y: &Counter = &x;     // ref_level=1, borrowed_from=x
let z: &&Counter = &y;    // ref_level=2, borrowed_from=y
let w: &Counter = *z;     // ref_level=1 (通过 deref)
```

### 生命周期栈示例

```
fn get<'a>(&'a self) -> &'a i32 { ... }

调用时：
  lifetime_stack.add_borrow("'a", return_token_id, self_token_id)
  ↓
  Frame { lifetime: "'a", borrows: [return_id], blocks: [self_id] }
  ↓
  self_token 被阻塞，不能 drop 或可变借用
```

---

## 🎉 重大突破

1. ✅ **完整的 Token 系统**：从简单计数到实例追踪
2. ✅ **生命周期基础设施**：栈、帧、借用追踪
3. ✅ **正确的 Rust 语义**：
   - Copy 自动复制
   - 引用不消耗
   - 借用规则通过 Guard
   - 多级引用支持
4. ✅ **可扩展架构**：为生命周期完整实现打下基础

**系统已经具备了实现完整生命周期管理的所有核心组件！** 🎯
