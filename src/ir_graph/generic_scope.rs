/// 泛型作用域管理器
///
/// 用于在解析过程中跟踪泛型参数的作用域
/// 例如：
/// ```rust
/// struct Container<T> {       // Push scope: T -> TypeNode_123
///     value: T,                // Resolve: T -> TypeNode_123
/// }
///
/// impl<U: Clone> Container<U> { // Push scope: U -> TypeNode_456
///     fn get(&self) -> U { }     // Resolve: U -> TypeNode_456
/// }                              // Pop scope
/// ```
use rustdoc_types::Id;
use std::collections::HashMap;

use super::structure::TypeNode;

/// 泛型作用域栈帧
#[derive(Debug, Clone)]
struct ScopeFrame {
    /// 作用域所有者（Struct/Enum/Fn/Impl 的 Id）
    owner_id: Id,
    /// 泛型参数映射：名称 -> TypeNode
    generics: HashMap<String, crate::ir_graph::TypeNode>,
    /// Self 类型的具体 Id（用于 impl 块）
    ///
    /// 例如：impl MyTrait for MyStruct { ... }
    /// 在这个作用域中，self_type = Some(MyStruct_Id)
    self_type: Option<Id>,
}

/// 泛型作用域管理器
#[derive(Debug)]
pub struct GenericScope {
    /// 作用域栈（从外到内）
    stack: Vec<ScopeFrame>,
}

impl GenericScope {
    /// 创建新的作用域管理器
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// 进入新作用域
    ///
    /// 参数：
    /// - owner_id: 作用域所有者 ID
    /// - generics: 泛型参数映射（名称 -> TypeNode）
    pub fn push_scope(&mut self, owner_id: Id, generics: HashMap<String, TypeNode>) {
        self.stack.push(ScopeFrame {
            owner_id,
            generics,
            self_type: None,
        });
    }

    /// 进入新作用域（带 Self 类型）
    ///
    /// 用于 impl 块
    pub fn push_scope_with_self(
        &mut self,
        owner_id: Id,
        generics: HashMap<String, TypeNode>,
        self_type: Id,
    ) {
        self.stack.push(ScopeFrame {
            owner_id,
            generics,
            self_type: Some(self_type),
        });
    }

    /// 退出当前作用域
    pub fn pop_scope(&mut self) {
        self.stack.pop();
    }

    /// 解析泛型参数
    ///
    /// 从栈顶向下查找泛型参数名，返回对应的 TypeNode
    ///
    /// 参数：
    /// - name: 泛型参数名（如 "T"）
    ///
    /// 返回：
    /// - Some(TypeNode): 找到对应的泛型节点
    /// - None: 未找到（可能是未定义的泛型）
    pub fn resolve(&self, name: &str) -> Option<TypeNode> {
        // 从栈顶向下查找（最内层作用域优先）
        for frame in self.stack.iter().rev() {
            if let Some(node) = frame.generics.get(name) {
                return Some(node.clone());
            }
        }
        None
    }

    /// 获取当前作用域的所有者 ID
    pub fn current_owner(&self) -> Option<Id> {
        self.stack.last().map(|frame| frame.owner_id)
    }

    /// 解析 Self 类型
    ///
    /// 从栈顶向下查找，返回最近的 self_type
    pub fn resolve_self(&self) -> Option<Id> {
        for frame in self.stack.iter().rev() {
            if let Some(self_type) = frame.self_type {
                return Some(self_type);
            }
        }
        None
    }

    /// 检查是否在任何作用域中
    pub fn has_scope(&self) -> bool {
        !self.stack.is_empty()
    }

    /// 获取作用域深度
    pub fn depth(&self) -> usize {
        self.stack.len()
    }
}

impl Default for GenericScope {
    fn default() -> Self {
        Self::new()
    }
}
