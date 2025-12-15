# LabeledPetriNet 定义与元素说明

## 概述

`LabeledPetriNet` 是一个带标签的 Petri 网,用于建模 Rust API 的类型系统和操作语义.它将 Rust 代码中的数据类型和函数调用转换为 Petri 网的形式化表示,便于进行程序分析和测试生成.

## 数据结构

### LabeledPetriNet

```rust
pub struct LabeledPetriNet {
    pub places: Vec<String>,                    // 库所列表(数据类型节点)
    pub transitions: Vec<String>,               // 变迁列表(操作节点)
    pub transition_attrs: Vec<TransitionAttr>, // 变迁属性
    pub arcs: Vec<Arc>,                         // 弧列表
    pub initial_marking: Vec<usize>,            // 初始标记(每个 place 的 token 数量)
    pub trans_to_node: HashMap<usize, NodeIndex>, // Transition → IrGraph NodeIndex 映射
    pub place_to_node: HashMap<usize, NodeIndex>, // Place → IrGraph NodeIndex 映射
}
```

## 核心元素

### 1. Place(库所)- 数据类型节点

**含义**：Place 表示系统中的状态或资源,在 Rust API 建模中对应**数据类型**.

#### Place 的类型映射

| Rust 类型 | NodeType | 说明 |
|-----------|----------|------|
| 结构体 | `Struct` | 用户定义的结构体类型 |
| 枚举 | `Enum` | 枚举类型 |
| 联合体 | `Union` | 联合体类型 |
| 常量 | `Constant` | 编译时常量,可能有初始标记 |
| 静态变量 | `Static` | 静态变量,可能有初始标记 |
| 基本类型 | `Primitive` | `i32`, `bool`, `f64`, `str` 等 |
| 元组 | `Tuple` | 元组类型 `(T1, T2, ...)` |
| 变体 | `Variant` | 枚举的某个变体 |
| 泛型参数 | `Generic` | 泛型类型参数 `T` |
| 类型别名 | `TypeAlias` | `type` 定义的别名 |
| Trait | `Trait` | Trait 定义(作为约束使用) |
| Result 包装 | `ResultWrapper` | `Result<T, E>` 类型 |
| Option 包装 | `OptionWrapper` | `Option<T>` 类型 |
| 单元类型 | `Unit` | `()` 类型 |

#### Place 的初始标记(Initial Marking)

- **默认值**：大多数 Place 的初始标记为 `0`
- **有初始值的类型**：
  - `Constant`：根据 `init_value` 解析为 token 数量
  - `Static`：根据 `init_value` 解析为 token 数量
  - **Shim Place**：基本类型的 shim place 初始标记为 `1`(表示类型可用)

**Token 的含义**：表示该类型在当前状态下可用的实例数量.

### 2. Transition(变迁)- 操作节点

**含义**：Transition 表示状态转换或操作,在 Rust API 建模中对应**函数/方法调用**.

#### Transition 的类型映射

| Rust 操作 | NodeType | 说明 |
|-----------|----------|------|
| 实现方法 | `ImplMethod` | 类型实现的方法 |
| Trait 方法 | `TraitMethod` | Trait 定义的方法 |
| 自由函数 | `Function` | 模块级函数 |
| 展开操作 | `UnwrapOp` | `Result::unwrap()` / `Option::unwrap()` 等 |
| 虚拟变迁 | - | 用于连接 Place → Place 的中间变迁 |

#### Transition 属性(TransitionAttr)

```rust
pub struct TransitionAttr {
    pub is_const: bool,   // 是否是 const 函数
    pub is_async: bool,   // 是否是 async 函数
    pub is_unsafe: bool,  // 是否是 unsafe 函数
}
```

这些属性用于：
- 代码生成时的正确性检查
- 分析时的约束条件
- Fuzz 测试时的调用策略

#### 虚拟变迁(Virtual Transition)

当两个 Place 之间存在直接关系时,需要创建虚拟变迁来连接它们：

1. **结构性关系**(`Include`, `Alias`, `Instance`)：
   - 例如：`Struct` → `Generic`(结构体包含泛型参数)
   - 虚拟变迁名称格式：`{source}_{mode}_{target}`

2. **数据流关系**(`Move`, `Ref`, `MutRef`)：
   - 例如：字段访问、类型转换
   - 虚拟变迁名称格式：`access_{source}_{field_name}`

### 3. Arc(弧)- 连接关系

**含义**：Arc 连接 Place 和 Transition,表示数据流或类型关系.

#### Arc 结构

```rust
pub struct Arc {
    pub from_idx: usize,        // 源索引(place 或 transition)
    pub to_idx: usize,          // 目标索引(transition 或 place)
    pub is_input_arc: bool,      // true: Place → Transition, false: Transition → Place
    pub label: EdgeMode,         // 弧的标签(数据传递模式)
    pub weight: usize,           // 弧的权重(默认为 1)
    pub name: Option<String>,    // 可选的字段/参数名称
}
```

#### 弧的类型

1. **输入弧(Input Arc)**：`Place → Transition`
   - **含义**：函数/方法**消耗**输入参数
   - **语义**：执行该操作需要消耗相应类型的 token

2. **输出弧(Output Arc)**：`Transition → Place`
   - **含义**：函数/方法**产生**返回值
   - **语义**：执行该操作会产生相应类型的 token

#### EdgeMode 标签

`EdgeMode` 定义了数据传递的模式和类型关系：

| EdgeMode | 含义 | 示例 | 消耗性 |
|----------|------|------|--------|
| `Move` | 按值移动(所有权转移) | `fn take(s: String)` | ✅ 是 |
| `Ref` | 共享引用 `&T` | `fn borrow(s: &String)` | ❌ 否(需要守卫) |
| `MutRef` | 可变引用 `&mut T` | `fn mutate(s: &mut String)` | ❌ 否(需要守卫) |
| `Ptr` | 裸指针 `*const T` | `unsafe fn raw(p: *const T)` | ❌ 否 |
| `MutPtr` | 可变裸指针 `*mut T` | `unsafe fn raw_mut(p: *mut T)` | ❌ 否 |
| `Implements` | 实现关系 | `impl Trait for Type` | - |
| `Require` | 约束关系 | `fn foo<T: Trait>()` | - |
| `Include` | 包含关系 | `Vec<T>` 包含 `T` | - |
| `Alias` | 类型别名 | `type Alias = Type` | - |
| `Instance` | 实例化关系 | `const C: Type = ...` | - |
| `UnwrapOk` | Result 成功展开 | `Result::unwrap()` → `T` | ✅ 是 |
| `UnwrapErr` | Result 失败展开 | `Result::unwrap()` → `E` | ✅ 是 |
| `UnwrapNone` | Option 失败展开 | `Option::unwrap()` → `None` | ✅ 是 |

#### 弧的权重(Weight)

- **默认值**：`1`
- **含义**：执行一次 Transition 需要消耗/产生的 token 数量
- **当前实现**：所有弧的权重均为 1

#### 弧的名称(Name)

- **用途**：存储字段名、参数名等语义信息
- **示例**：
  - 结构体字段访问：`name = Some("field_name")`
  - 函数参数：`name = Some("param_name")`

### 4. 初始标记(Initial Marking)

**含义**：每个 Place 在初始状态下的 token 数量.

- **默认值**：`0`(大多数类型)
- **有初始值的类型**：
  - `Constant`：根据 `init_value` 解析
  - `Static`：根据 `init_value` 解析
  - **Shim Place**：基本类型为 `1`

**Token 解析规则**：
- 如果 `init_value` 是数字字符串,解析为对应数字
- 如果 `init_value` 是非空字符串,返回 `1`
- 如果 `init_value` 为空或 `None`,返回 `0`

### 5. 映射关系

#### trans_to_node

- **类型**：`HashMap<usize, NodeIndex>`
- **含义**：Transition 索引 → IrGraph 节点索引的映射
- **用途**：
  - 回溯到原始 IR Graph
  - Fuzz 测试时生成正确的函数调用
  - 代码生成时获取函数详细信息

#### place_to_node

- **类型**：`HashMap<usize, NodeIndex>`
- **含义**：Place 索引 → IrGraph 节点索引的映射
- **用途**：
  - 回溯到原始 IR Graph
  - 类型信息查询
  - 代码生成时获取类型详细信息

## 转换规则(从 IrGraph)

### 节点分类规则

```
数据节点 → Place:
  - Struct, Enum, Union, Constant, Static
  - Primitive, Tuple, Variant, Generic
  - TypeAlias, Unit, Trait
  - ResultWrapper, OptionWrapper

操作节点 → Transition:
  - ImplMethod, TraitMethod, Function
  - UnwrapOp
```

### 边转换规则

| 源节点类型 | 目标节点类型 | 转换方式 |
|-----------|------------|---------|
| Place | Transition | 输入弧(Place → Transition) |
| Transition | Place | 输出弧(Transition → Place) |
| Place | Place | 创建虚拟 Transition 连接 |
| Transition | Transition | 忽略(不符合 Petri 网语义) |

### 初始标记设置

1. 遍历所有 `NodeInfo::Constant` 和 `NodeInfo::Static`
2. 解析 `init_value` 字段
3. 设置对应 Place 的初始标记

### Shim Place 添加

`add_primitive_shims()` 方法会：

1. 为常见基本类型(`i32`, `bool`, `f64`, `str` 等)创建 shim place
2. 设置初始标记为 `1`
3. 链接默认 Trait 实现(`Copy`, `Clone`, `Debug` 等)
4. 如果 IR Graph 中存在对应的 Generic 节点,添加 `Instance` 弧

## 使用场景

### 1. API 序列生成

通过 Petri 网的执行模拟,生成有效的 API 调用序列：
- 从有初始 token 的 Place 开始
- 选择可执行的 Transition(所有输入 Place 都有足够的 token)
- 执行 Transition,消耗输入 token,产生输出 token
- 重复直到达到目标状态

### 2. Fuzz 测试

- 使用 `trans_to_node` 映射生成实际的函数调用代码
- 使用 `place_to_node` 映射生成类型构造代码
- 根据 `TransitionAttr` 添加正确的函数修饰符(`const`, `async`, `unsafe`)

### 3. 类型依赖分析

- 分析 Place 之间的依赖关系
- 识别类型约束(`Implements`, `Require` 弧)
- 检测循环依赖

### 4. 程序验证

- 检查借用规则(`Ref`/`MutRef` 弧的守卫条件)
- 验证 Trait 约束(`Implements`/`Require` 弧)
- 分析所有权转移(`Move` 弧)

## 守卫逻辑(Guard Logic)

在完整的 Petri 网模拟中,某些 Transition 需要守卫条件：

1. **借用检查**：
   - `Ref` 弧：可以有多个同时存在
   - `MutRef` 弧：同时只能有一个

2. **Trait 约束检查**：
   - `Implements` 弧：类型必须实现指定的 Trait
   - `Require` 弧：泛型参数必须满足 Trait 约束

3. **Unwrap 分支选择**：
   - `UnwrapOp` 需要根据 Result/Option 的状态选择 `UnwrapOk`/`UnwrapErr`/`UnwrapNone` 分支

## 统计信息

```rust
pub struct PetriNetStats {
    pub place_count: usize,           // Place 数量
    pub transition_count: usize,      // Transition 数量
    pub input_arc_count: usize,       // 输入弧数量
    pub output_arc_count: usize,      // 输出弧数量
    pub total_initial_tokens: usize,  // 初始 token 总数
}
```

## 总结

`LabeledPetriNet` 将 Rust 的类型系统和操作语义形式化为 Petri 网,使得：

- **类型** → **Place**：表示系统中的状态/资源
- **函数/方法** → **Transition**：表示状态转换/操作
- **数据流/关系** → **Arc**：表示类型依赖和操作语义
- **初始值** → **Initial Marking**：表示系统初始状态

这种形式化表示使得我们可以使用 Petri 网的理论和工具来分析 Rust 程序,生成测试用例,并进行程序验证.

