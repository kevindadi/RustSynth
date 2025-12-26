//! 下推栈定义
//!
//! 栈用于跟踪借用和作用域信息

use std::fmt;
use serde::{Deserialize, Serialize};
use super::types::TypeId;
use super::marking::ValueId;

/// 栈操作
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StackOp {
    /// 无操作
    None,
    /// 压入
    Push,
    /// 弹出
    Pop,
    /// 替换栈顶（Pop + Push）
    Replace,
}

/// 栈帧类型
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StackFrame {
    /// 共享借用帧
    SharedBorrow {
        /// 被借用值的 ID
        value_id: ValueId,
        /// 被借用的类型
        type_id: TypeId,
        /// 借用计数（同一值可能被多次借用）
        count: usize,
    },

    /// 可变借用帧
    MutBorrow {
        /// 被借用值的 ID
        value_id: ValueId,
        /// 被借用的类型
        type_id: TypeId,
    },

    /// 字段投影帧
    FieldProj {
        /// 父结构体的值 ID
        parent_id: ValueId,
        /// 字段名
        field: String,
        /// 字段类型
        field_type: TypeId,
    },

    /// 函数调用帧
    FnCall {
        /// 函数名
        name: String,
        /// 返回类型
        return_type: Option<TypeId>,
    },

    /// 作用域帧（通用）
    Scope {
        /// 作用域标识
        id: String,
    },
}

impl StackFrame {
    /// 获取帧中引用的值 ID（如果有）
    pub fn value_id(&self) -> Option<ValueId> {
        match self {
            StackFrame::SharedBorrow { value_id, .. } => Some(*value_id),
            StackFrame::MutBorrow { value_id, .. } => Some(*value_id),
            StackFrame::FieldProj { parent_id, .. } => Some(*parent_id),
            _ => None,
        }
    }

    /// 是否是借用帧
    pub fn is_borrow(&self) -> bool {
        matches!(self, StackFrame::SharedBorrow { .. } | StackFrame::MutBorrow { .. })
    }

    /// 是否是可变借用帧
    pub fn is_mut_borrow(&self) -> bool {
        matches!(self, StackFrame::MutBorrow { .. })
    }
}

impl fmt::Display for StackFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StackFrame::SharedBorrow { value_id, type_id, count } => {
                write!(f, "&{}:{}({})", value_id, type_id, count)
            }
            StackFrame::MutBorrow { value_id, type_id } => {
                write!(f, "&mut {}:{}", value_id, type_id)
            }
            StackFrame::FieldProj { parent_id, field, field_type } => {
                write!(f, "{}.{}:{}", parent_id, field, field_type)
            }
            StackFrame::FnCall { name, .. } => {
                write!(f, "call({})", name)
            }
            StackFrame::Scope { id } => {
                write!(f, "scope({})", id)
            }
        }
    }
}

/// 下推栈
#[derive(Debug, Clone, Default)]
pub struct PushdownStack {
    frames: Vec<StackFrame>,
}

impl PushdownStack {
    pub fn new() -> Self {
        Self::default()
    }

    /// 压入栈帧
    pub fn push(&mut self, frame: StackFrame) {
        self.frames.push(frame);
    }

    /// 弹出栈帧
    pub fn pop(&mut self) -> Option<StackFrame> {
        self.frames.pop()
    }

    /// 获取栈顶
    pub fn top(&self) -> Option<&StackFrame> {
        self.frames.last()
    }

    /// 获取栈顶（可变）
    pub fn top_mut(&mut self) -> Option<&mut StackFrame> {
        self.frames.last_mut()
    }

    /// 栈深度
    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// 检查栈顶是否匹配期望的帧类型
    pub fn top_matches<F>(&self, predicate: F) -> bool
    where
        F: Fn(&StackFrame) -> bool,
    {
        self.top().map(&predicate).unwrap_or(false)
    }

    /// 获取所有帧
    pub fn frames(&self) -> &[StackFrame] {
        &self.frames
    }

    /// 查找指定值的借用帧
    pub fn find_borrow(&self, value_id: ValueId) -> Option<&StackFrame> {
        self.frames
            .iter()
            .rev()
            .find(|f| f.value_id() == Some(value_id) && f.is_borrow())
    }

    /// 检查是否有对指定值的可变借用
    pub fn has_mut_borrow(&self, value_id: ValueId) -> bool {
        self.frames.iter().any(|f| {
            matches!(f, StackFrame::MutBorrow { value_id: vid, .. } if *vid == value_id)
        })
    }

    /// 检查是否有对指定值的任何借用
    pub fn has_any_borrow(&self, value_id: ValueId) -> bool {
        self.frames.iter().any(|f| f.value_id() == Some(value_id) && f.is_borrow())
    }

    /// 增加共享借用计数
    pub fn increment_shared_borrow(&mut self, value_id: ValueId) -> bool {
        for frame in self.frames.iter_mut().rev() {
            if let StackFrame::SharedBorrow { value_id: vid, count, .. } = frame {
                if *vid == value_id {
                    *count += 1;
                    return true;
                }
            }
        }
        false
    }

    /// 减少共享借用计数
    pub fn decrement_shared_borrow(&mut self, value_id: ValueId) -> Option<bool> {
        for (i, frame) in self.frames.iter_mut().enumerate().rev() {
            if let StackFrame::SharedBorrow { value_id: vid, count, .. } = frame {
                if *vid == value_id {
                    *count -= 1;
                    if *count == 0 {
                        self.frames.remove(i);
                        return Some(true); // 借用结束
                    }
                    return Some(false); // 还有其他借用
                }
            }
        }
        None // 没找到
    }

    /// 计算栈状态的哈希值
    pub fn state_hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        for frame in &self.frames {
            format!("{:?}", frame).hash(&mut hasher);
        }
        hasher.finish()
    }
}

impl fmt::Display for PushdownStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.frames.is_empty() {
            write!(f, "[]")
        } else {
            let parts: Vec<String> = self.frames.iter().map(|f| format!("{}", f)).collect();
            write!(f, "[{}]", parts.join(" | "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_operations() {
        let mut stack = PushdownStack::new();

        assert!(stack.is_empty());

        let frame = StackFrame::SharedBorrow {
            value_id: ValueId::new(1),
            type_id: TypeId::new(0),
            count: 1,
        };

        stack.push(frame.clone());
        assert_eq!(stack.depth(), 1);
        assert_eq!(stack.top(), Some(&frame));

        stack.pop();
        assert!(stack.is_empty());
    }

    #[test]
    fn test_borrow_counting() {
        let mut stack = PushdownStack::new();

        let value_id = ValueId::new(1);
        let frame = StackFrame::SharedBorrow {
            value_id,
            type_id: TypeId::new(0),
            count: 1,
        };

        stack.push(frame);

        // 增加借用计数
        assert!(stack.increment_shared_borrow(value_id));

        // 减少借用计数
        assert_eq!(stack.decrement_shared_borrow(value_id), Some(false));
        assert_eq!(stack.decrement_shared_borrow(value_id), Some(true)); // 最后一个借用
        assert!(stack.is_empty());
    }
}

