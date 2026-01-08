# API 依赖图使用指南

## 功能说明

API 依赖图分析工具可以：

1. **构建类型依赖关系**：分析哪些 API 产生/消费哪些类型
2. **提取 Trait 实现**：自动发现 `Default`, `Clone` 等 trait 的实现
3. **识别公共字段**：标记 struct 的 public 字段，可作为类型来源
4. **可视化依赖**：生成 DOT 格式图，用 Graphviz 渲染

## 使用方法

### 1. 生成 DOT 文件

```bash
sypetype --input target/doc/your_crate.json --graph api_graph.dot
```

### 2. 渲染为图片

使用 Graphviz 工具：

```bash
# 安装 Graphviz (如果未安装)
# macOS
brew install graphviz

# Ubuntu/Debian
sudo apt-get install graphviz

# 生成 PNG 图片
dot -Tpng api_graph.dot -o api_graph.png

# 生成 SVG (矢量图，可缩放)
dot -Tsvg api_graph.dot -o api_graph.svg

# 生成 PDF
dot -Tpdf api_graph.dot -o api_graph.pdf
```

### 3. 在线查看

也可以使用在线工具：
- [GraphvizOnline](https://dreampuf.github.io/GraphvizOnline/)
- [Edotor](https://edotor.net/)

直接粘贴 DOT 文件内容即可查看。

## 图例说明

### 节点类型

- **双八边形** (Entry APIs)
  - 无参数或只需 primitive 类型
  - 可以直接调用，作为入口点
  - 例如：`Counter::new()`

- **浅蓝色方框** (Normal APIs)
  - 普通函数和方法
  - 例如：`Counter::increment(&mut Counter)`

- **浅绿色方框** (Trait Impls)
  - Trait 实现的方法
  - 标记为 `[Default]`, `[Clone]` 等
  - 例如：`Counter::default() [Default]`

- **浅黄色方框** (Field Access)
  - Public 字段访问
  - 标记为 `[field]`
  - 例如：`Counter.value [field]`

### 边（箭头）

- **黑色箭头**：复合类型依赖
  - 标签显示类型名
  - 例如：`Counter` 从 `new()` 到 `increment()`

- **灰色箭头**：Primitive 类型依赖
  - 标签显示 primitive 类型
  - 例如：`i32` 从 `get()` 到 `create_counter_with_value()`

### 节点标签格式

```
方法名 [来源]
(参数类型列表) → 返回类型
```

示例：
- `new()\\n() → Counter` - 无参数，返回 Counter
- `increment [method]\\n(&mut Counter) → ()` - 接受 &mut Counter，无返回值
- `get()\\n(&Counter) → i32` - 接受 &Counter，返回 i32

## 示例：simple_counter 的 API 图

### 预期的图结构

```
入口 APIs (双八边形):
  ┌─────────────┐
  │   new()     │
  │ () → Counter│
  └──────┬──────┘
         │ Counter
         ↓
  ┌─────────────────┐     ┌──────────────┐
  │   increment()   │     │    get()     │
  │(&mut Counter)→()│     │(&Counter)→i32│
  └─────────────────┘     └──────┬───────┘
                                 │ i32
                                 ↓
                          ┌──────────────────────┐
                          │create_counter_with..│
                          │    (i32) → Counter   │
                          └──────────────────────┘

Trait Impls:
  ┌────────────────────┐
  │  default() [Default]│
  │   () → Counter      │
  └────────────────────┘

  ┌────────────────────┐
  │  clone() [Clone]   │
  │ (&Counter)→Counter │
  └────────────────────┘

Field Access:
  ┌────────────────────┐
  │Counter.value [field]│
  │  (&Counter) → i32   │
  └────────────────────┘
```

### 调用链分析

从图中可以看出几条重要的调用链：

1. **基础链**：
   ```
   new() → Counter → increment(&mut Counter)
                  ↘ get(&Counter) → i32
   ```

2. **带参数链**：
   ```
   (i32常量) → create_counter_with_value(i32) → Counter
   ```

3. **Trait 链**：
   ```
   default() → Counter
   clone(&Counter) → Counter
   ```

4. **字段访问链**：
   ```
   Counter → Counter.value → i32
   ```

## 高级用法

### 1. 过滤模块

只分析特定模块的 API：

```bash
sypetype --input target/doc/my_crate.json \
    --graph api_graph.dot \
    --module crate::database \
    --module crate::models
```

### 2. 结合搜索

先生成图，再执行搜索：

```bash
sypetype --input target/doc/my_crate.json \
    --graph api_graph.dot \
    --output witness.rs \
    --verbose
```

### 3. 分析大型 crate

对于大型 crate，图可能很复杂。建议：

```bash
# 使用模块过滤减少节点数
sypetype --input target/doc/large_crate.json \
    --graph api_graph.dot \
    --module crate::core

# 使用 fdp 布局算法（更适合大图）
fdp -Tpng api_graph.dot -o api_graph.png

# 或使用 sfdp（处理超大图）
sfdp -Tpng api_graph.dot -o api_graph.png
```

## 图的解读

### 1. 识别入口点

寻找双八边形节点，这些是：
- 无参数构造函数（`new()`, `default()`）
- 只需要 primitive 参数的函数
- 可以直接开始调用的 API

### 2. 发现调用链

跟随箭头，找到有意义的调用序列：
- 入口 API → 中间 API → 最终 API
- 注意类型匹配：箭头标签显示传递的类型

### 3. 理解类型流

每个箭头代表一个类型的"流动"：
- 从产生者（返回该类型的 API）
- 到消费者（接受该类型作为参数的 API）

### 4. 利用 Trait

绿色的 Trait impl 节点提供了额外的类型来源：
- `Default::default()` 可以无参数创建实例
- `Clone::clone()` 可以复制现有实例
- 这些是搜索时的重要起点

### 5. 字段访问

黄色的字段访问节点表示：
- 可以从父类型提取子类型
- 例如：从 `Counter` 获取 `i32` (通过 `.value` 字段)
- 这提供了类型转换的途径

## 实际应用场景

### 场景 1: API 设计审查

查看图，检查：
- 是否有孤立的 API（无法被调用或结果无法使用）
- 是否有缺失的转换路径
- 类型依赖是否合理

### 场景 2: 测试用例生成

从图中识别：
- 主要的调用链
- 边界情况（特殊的类型组合）
- 需要测试的 API 组合

### 场景 3: 文档生成

使用图：
- 展示 API 之间的关系
- 说明典型的使用模式
- 提供调用示例

### 场景 4: 重构指导

分析图：
- 发现耦合度高的 API 群
- 识别可以分离的模块
- 优化类型设计

## 故障排除

### 问题 1: 图太大无法查看

**解决方案**：
- 使用模块过滤减少节点
- 使用不同的布局算法（`fdp`, `sfdp`）
- 分别生成多个子图

### 问题 2: 节点重叠

**解决方案**：
```bash
# 增加节点间距
dot -Tpng -Gnodesep=2 -Granksep=3 api_graph.dot -o api_graph.png

# 使用更大的画布
dot -Tpng -Gsize="20,20!" api_graph.dot -o api_graph.png
```

### 问题 3: 找不到 Trait 实现

**原因**：当前只支持 `Default` 和 `Clone`

**解决方案**：扩展 `api_graph.rs` 中的 trait 识别逻辑

### 问题 4: 缺少字段访问

**原因**：只提取 public 字段

**确认**：检查 struct 定义，确保字段是 `pub`

## 下一步

1. 查看生成的图，理解 API 结构
2. 使用图指导搜索策略
3. 基于图实现启发式搜索
4. 改进代码生成质量

---

**提示**：将生成的图与代码文档结合使用，可以更好地理解 crate 的设计！
