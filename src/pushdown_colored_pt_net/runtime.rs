//! PCPN 运行时组件
//!
//! 实现多集、标记、栈帧、配置、变迁 trait 和触发逻辑

use std::collections::HashMap;
use crate::pushdown_colored_pt_net::types::{Color, TypeExpr, ValueId};
use crate::pushdown_colored_pt_net::env::Env;
use std::fmt;

/// 多重集合 (MultiSet)
/// 
/// 用于表示每个库所中的 token 集合,支持同一颜色的多个 token
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiSet {
    /// 颜色 -> 数量 的映射
    tokens: HashMap<Color, usize>,
}

impl MultiSet {
    /// 创建空的多重集合
    pub fn new() -> Self {
        MultiSet {
            tokens: HashMap::new(),
        }
    }

    /// 添加一个 token
    pub fn add(&mut self, color: Color) {
        *self.tokens.entry(color).or_insert(0) += 1;
    }

    /// 添加多个相同颜色的 token
    pub fn add_count(&mut self, color: Color, count: usize) {
        *self.tokens.entry(color).or_insert(0) += count;
    }

    /// 移除一个 token (如果存在)
    pub fn remove(&mut self, color: &Color) -> bool {
        if let Some(count) = self.tokens.get_mut(color) {
            if *count > 0 {
                *count -= 1;
                if *count == 0 {
                    self.tokens.remove(color);
                }
                return true;
            }
        }
        false
    }

    /// 移除指定数量的 token
    pub fn remove_count(&mut self, color: &Color, count: usize) -> usize {
        if let Some(existing_count) = self.tokens.get_mut(color) {
            let to_remove = count.min(*existing_count);
            *existing_count -= to_remove;
            if *existing_count == 0 {
                self.tokens.remove(color);
            }
            to_remove
        } else {
            0
        }
    }

    /// 获取指定颜色的 token 数量
    pub fn count(&self, color: &Color) -> usize {
        self.tokens.get(color).copied().unwrap_or(0)
    }

    /// 获取所有颜色及其数量
    pub fn iter(&self) -> impl Iterator<Item = (&Color, usize)> {
        self.tokens.iter().map(|(k, v)| (k, *v))
    }

    /// 检查是否包含指定颜色的 token
    pub fn contains(&self, color: &Color) -> bool {
        self.count(color) > 0
    }

    /// 检查多重集合是否为空
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// 获取总 token 数量
    pub fn total_count(&self) -> usize {
        self.tokens.values().sum()
    }

    /// 检查是否包含至少 `count` 个指定颜色的 token
    pub fn has_at_least(&self, color: &Color, count: usize) -> bool {
        self.count(color) >= count
    }

    /// 获取所有颜色的集合
    pub fn colors(&self) -> Vec<Color> {
        self.tokens.keys().cloned().collect()
    }
}

impl Default for MultiSet {
    fn default() -> Self {
        Self::new()
    }
}

/// 库所 ID
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Place(pub usize);

impl Place {
    pub fn new(id: usize) -> Self {
        Place(id)
    }
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "p{}", self.0)
    }
}

/// 标记 (Marking)
/// 
/// 表示 Petri 网的当前状态: 每个库所中的 token 集合
#[derive(Debug, Clone)]
pub struct Marking {
    /// 库所 -> 多重集合 的映射
    places: HashMap<Place, MultiSet>,
}

impl Marking {
    /// 创建空的标记
    pub fn new() -> Self {
        Marking {
            places: HashMap::new(),
        }
    }

    /// 在指定库所添加 token
    pub fn add_token(&mut self, place: Place, color: Color) {
        self.places
            .entry(place)
            .or_insert_with(MultiSet::new)
            .add(color);
    }

    /// 在指定库所添加多个 token
    pub fn add_tokens(&mut self, place: Place, color: Color, count: usize) {
        self.places
            .entry(place)
            .or_insert_with(MultiSet::new)
            .add_count(color, count);
    }

    /// 从指定库所移除 token
    pub fn remove_token(&mut self, place: Place, color: &Color) -> bool {
        if let Some(multiset) = self.places.get_mut(&place) {
            multiset.remove(color)
        } else {
            false
        }
    }

    /// 从指定库所移除多个 token
    pub fn remove_tokens(&mut self, place: Place, color: &Color, count: usize) -> usize {
        if let Some(multiset) = self.places.get_mut(&place) {
            multiset.remove_count(color, count)
        } else {
            0
        }
    }

    /// 获取指定库所的 token 数量
    pub fn token_count(&self, place: Place, color: &Color) -> usize {
        self.places
            .get(&place)
            .map(|ms| ms.count(color))
            .unwrap_or(0)
    }

    /// 获取指定库所的多重集合
    pub fn get_multiset(&self, place: Place) -> MultiSet {
        self.places.get(&place).cloned().unwrap_or_else(MultiSet::new)
    }

    /// 获取指定库所的多重集合 (可变引用)
    pub fn get_multiset_mut(&mut self, place: Place) -> &mut MultiSet {
        self.places.entry(place).or_insert_with(MultiSet::new)
    }

    /// 检查标记中是否包含指定库所和颜色的 token
    pub fn contains(&self, place: Place, color: &Color) -> bool {
        self.token_count(place, color) > 0
    }

    /// 克隆标记
    pub fn clone(&self) -> Self {
        Marking {
            places: self.places.clone(),
        }
    }
}

impl Default for Marking {
    fn default() -> Self {
        Self::new()
    }
}

/// 栈帧
/// 
/// 表示下推栈中的一个帧,用于跟踪借用和作用域信息
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StackFrame {
    /// 共享借用帧: (值 ID, 类型)
    SharedBorrow { value_id: ValueId, ty: TypeExpr },
    /// 可变借用帧: (值 ID, 类型)
    MutBorrow { value_id: ValueId, ty: TypeExpr },
    /// 字段投影帧: (父值 ID, 字段名, 类型)
    FieldProj { parent_id: ValueId, field: String, ty: TypeExpr },
    /// 作用域帧: (作用域 ID)
    Scope { id: String },
}

impl StackFrame {
    /// 获取帧中引用的值 ID
    pub fn value_id(&self) -> Option<ValueId> {
        match self {
            StackFrame::SharedBorrow { value_id, .. } => Some(*value_id),
            StackFrame::MutBorrow { value_id, .. } => Some(*value_id),
            StackFrame::FieldProj { parent_id, .. } => Some(*parent_id),
            StackFrame::Scope { .. } => None,
        }
    }
}

/// 下推栈
#[derive(Debug, Clone)]
pub struct Stack {
    frames: Vec<StackFrame>,
}

impl Stack {
    /// 创建空栈
    pub fn new() -> Self {
        Stack {
            frames: Vec::new(),
        }
    }

    /// 压入栈帧
    pub fn push(&mut self, frame: StackFrame) {
        self.frames.push(frame);
    }

    /// 弹出栈帧
    pub fn pop(&mut self) -> Option<StackFrame> {
        self.frames.pop()
    }

    /// 获取栈顶元素 (不弹出)
    pub fn top(&self) -> Option<&StackFrame> {
        self.frames.last()
    }

    /// 检查栈是否为空
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// 获取栈深度
    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// 获取栈的所有帧
    pub fn frames(&self) -> &[StackFrame] {
        &self.frames
    }
}

impl Default for Stack {
    fn default() -> Self {
        Self::new()
    }
}

/// 配置 (Configuration)
/// 
/// 表示 PCPN 的完整状态: 标记 + 栈
#[derive(Debug, Clone)]
pub struct Config {
    /// 当前标记
    pub marking: Marking,
    /// 下推栈
    pub stack: Stack,
}

impl Config {
    /// 创建新配置
    pub fn new() -> Self {
        Config {
            marking: Marking::new(),
            stack: Stack::new(),
        }
    }

    /// 检查配置是否有效 (例如栈深度是否在限制内)
    pub fn is_valid(&self, max_stack_depth: usize) -> bool {
        self.stack.depth() <= max_stack_depth
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

/// 变迁 ID
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct TransitionId(pub usize);

impl TransitionId {
    pub fn new(id: usize) -> Self {
        TransitionId(id)
    }
}

/// 弧权重
/// 
/// 表示从库所到变迁 (或从变迁到库所) 的弧权重
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArcWeight(pub usize);

impl ArcWeight {
    pub fn new(weight: usize) -> Self {
        ArcWeight(weight)
    }

    pub fn value(&self) -> usize {
        self.0
    }
}

/// 输入弧
#[derive(Debug, Clone)]
pub struct InputArc {
    /// 源库所
    pub place: Place,
    /// 弧权重
    pub weight: ArcWeight,
    /// 颜色约束 (None 表示接受任何颜色)
    pub color_constraint: Option<TypeExpr>,
}

/// 输出弧
pub struct OutputArc {
    /// 目标库所
    pub place: Place,
    /// 弧权重
    pub weight: ArcWeight,
    /// 颜色生成函数 (返回新生成的颜色)
    pub color_gen: Option<ColorGenerator>,
}

impl std::fmt::Debug for OutputArc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutputArc")
            .field("place", &self.place)
            .field("weight", &self.weight)
            .field("color_gen", &if self.color_gen.is_some() { "Some(...)" } else { "None" })
            .finish()
    }
}

/// 颜色生成器
/// 
/// 用于在变迁触发时生成新的颜色
pub type ColorGenerator = Box<dyn Fn(&Marking, &Stack, &mut ValueIdGenerator) -> Color>;

/// 值 ID 生成器
/// 
/// 用于生成唯一的值 ID
#[derive(Debug, Clone)]
pub struct ValueIdGenerator {
    next_id: u64,
}

impl ValueIdGenerator {
    pub fn new() -> Self {
        ValueIdGenerator { next_id: 0 }
    }

    pub fn next(&mut self) -> ValueId {
        let id = ValueId::new(self.next_id);
        self.next_id += 1;
        id
    }
}

impl Default for ValueIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// 选择 (Choice)
/// 
/// 表示从输入库所中选择的具体 token 集合
#[derive(Debug, Clone)]
pub struct Choice {
    /// 库所 -> 选择的颜色列表
    pub selections: HashMap<Place, Vec<Color>>,
}

impl Choice {
    pub fn new() -> Self {
        Choice {
            selections: HashMap::new(),
        }
    }

    /// 为指定库所添加选择的颜色
    pub fn add_selection(&mut self, place: Place, color: Color) {
        self.selections
            .entry(place)
            .or_insert_with(Vec::new)
            .push(color);
    }
}

/// 变迁 trait
/// 
/// 所有变迁类型必须实现这个 trait
pub trait Transition: std::fmt::Debug {
    /// 获取变迁 ID
    fn id(&self) -> TransitionId;

    /// 获取变迁名称
    fn name(&self) -> &str;

    /// 获取输入弧列表
    fn input_arcs(&self) -> &[InputArc];

    /// 获取输出弧列表
    fn output_arcs(&self) -> &[OutputArc];

    /// 检查变迁在当前配置下是否可触发
    /// 
    /// 参数:
    /// - config: 当前配置
    /// - env: 环境 (用于类型检查和谓词)
    /// - value_gen: 值 ID 生成器
    /// 
    /// 返回:
    /// - Some(Choice): 如果可以触发,返回选择的具体 token
    /// - None: 如果不可触发
    fn is_enabled(
        &self,
        config: &Config,
        env: &dyn Env,
        value_gen: &mut ValueIdGenerator,
    ) -> Option<Choice>;

    /// 触发变迁
    /// 
    /// 参数:
    /// - config: 当前配置 (将被修改)
    /// - choice: 选择的具体 token
    /// - env: 环境
    /// - value_gen: 值 ID 生成器
    /// 
    /// 返回:
    /// - true: 触发成功
    /// - false: 触发失败 (不应该发生,如果 is_enabled 返回 Some)
    fn fire(
        &self,
        config: &mut Config,
        choice: &Choice,
        env: &dyn Env,
        value_gen: &mut ValueIdGenerator,
    ) -> bool;

    /// 获取栈操作
    fn stack_operation(&self) -> StackOp;
}

/// 栈操作
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackOp {
    /// 无操作
    None,
    /// Push 栈帧
    Push(StackFrame),
    /// Pop 栈帧 (检查栈顶是否匹配)
    Pop(Option<StackFrame>),
    /// 替换栈顶 (先 pop 再 push)
    Replace(StackFrame),
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::pushdown_colored_pt_net::types::TypeExpr;

    #[test]
    fn test_multiset() {
        let mut ms = MultiSet::new();
        let color = Color::new(TypeExpr::Primitive("u8".to_string()), ValueId::new(1));

        assert_eq!(ms.count(&color), 0);
        ms.add(color.clone());
        assert_eq!(ms.count(&color), 1);
        ms.add(color.clone());
        assert_eq!(ms.count(&color), 2);
        assert!(ms.remove(&color));
        assert_eq!(ms.count(&color), 1);
    }

    #[test]
    fn test_marking() {
        let mut marking = Marking::new();
        let place = Place::new(0);
        let color = Color::new(TypeExpr::Primitive("u8".to_string()), ValueId::new(1));

        marking.add_token(place, color.clone());
        assert_eq!(marking.token_count(place, &color), 1);
    }

    #[test]
    fn test_stack() {
        let mut stack = Stack::new();
        let frame = StackFrame::SharedBorrow {
            value_id: ValueId::new(1),
            ty: TypeExpr::Primitive("u8".to_string()),
        };

        assert!(stack.is_empty());
        stack.push(frame.clone());
        assert_eq!(stack.depth(), 1);
        assert_eq!(stack.top(), Some(&frame));
        assert_eq!(stack.pop(), Some(frame));
        assert!(stack.is_empty());
    }
}
