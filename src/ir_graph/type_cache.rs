//! 类型缓存模块
//!
//! 提供统一的类型管理，确保从 rustdoc_types::Type 到 NodeIndex 的唯一映射

use petgraph::graph::NodeIndex;
use rustdoc_types::{Id, Type};
use std::collections::HashMap;

/// 类型标识符，用于唯一标识一个类型
///
/// 这个枚举覆盖了 rustdoc_types::Type 的所有变体，
/// 并为每种类型提供可哈希的唯一标识
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub enum TypeKey {
    /// 有 ID 的类型：Struct, Enum, Union, Trait, TypeAlias 等
    /// 直接使用 rustdoc 的 Id
    Resolved(Id),

    /// 基本类型：u8, i32, bool, str 等
    /// 使用类型名作为标识
    Primitive(String),

    /// 泛型参数，需要上下文信息
    /// 例如：`Vec<T>` 中的 `T`，需要知道 T 属于哪个作用域
    Generic {
        name: String,
        /// 泛型所属的作用域（类型、Trait 或方法）
        scope: GenericScope,
    },

    /// 引用类型 &T, &mut T
    /// 递归包含内部类型
    BorrowedRef {
        is_mutable: bool,
        inner: Box<TypeKey>,
    },

    /// 裸指针 *const T, *mut T
    RawPointer {
        is_mutable: bool,
        inner: Box<TypeKey>,
    },

    /// 切片 [T]
    Slice(Box<TypeKey>),

    /// 数组 [T; N]
    Array { inner: Box<TypeKey>, len: String },

    /// 元组 (T1, T2, ...)
    Tuple(Vec<TypeKey>),

    /// 函数指针 fn(T1) -> T2
    /// 使用序列化的签名作为标识
    FunctionPointer(String),

    /// Trait object (dyn Trait)
    DynTrait(String),

    /// impl Trait
    ImplTrait(String),

    /// 关联类型 <T as Trait>::AssocType
    QualifiedPath {
        name: String,
        self_type: Box<TypeKey>,
        trait_id: Option<Id>,
    },

    /// 类型推断占位符 _
    Infer,

    /// 模式类型（实验性功能）
    Pat {
        inner: Box<TypeKey>,
        pattern: String,
    },
}

/// 泛型参数的作用域
///
/// 同名泛型可能出现在不同作用域，需要区分
/// 例如：`struct Vec<T>` 的 T 和 `fn foo<T>()` 的 T 是不同的
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub enum GenericScope {
    /// 类型的泛型参数，如 `struct Foo<T>`
    Type(Id),
    /// Trait 的泛型参数，如 `trait Bar<T>`
    Trait(Id),
    /// 方法的泛型参数，如 `fn baz<T>()`
    Method(Id),
    /// 顶层函数的泛型参数
    Function(Id),
    /// 全局/未知作用域（fallback）
    Global,
}

/// 类型解析上下文
///
/// 在解析类型时提供必要的上下文信息，
/// 特别是用于解析泛型参数和 Self 类型
#[derive(Clone, Debug)]
pub struct TypeContext {
    /// 当前所在的所有者（类型、Trait 或方法）
    pub current_owner: Option<Id>,

    /// 泛型作用域映射：泛型名 -> 作用域
    /// 例如：{"T" -> Type(id), "U" -> Method(id)}
    pub generic_scopes: HashMap<String, GenericScope>,
}

impl TypeContext {
    pub fn new() -> Self {
        Self {
            current_owner: None,
            generic_scopes: HashMap::new(),
        }
    }

    pub fn with_owner(owner_id: Id) -> Self {
        Self {
            current_owner: Some(owner_id),
            generic_scopes: HashMap::new(),
        }
    }

    /// 添加泛型参数到上下文
    pub fn add_generic(&mut self, name: String, scope: GenericScope) {
        self.generic_scopes.insert(name, scope);
    }

    /// 解析泛型参数的作用域
    pub fn resolve_generic_scope(&self, name: &str) -> Option<GenericScope> {
        // 特殊处理 Self
        if name == "Self" {
            return self.current_owner.map(GenericScope::Type);
        }

        self.generic_scopes.get(name).cloned()
    }
}

/// 类型缓存
///
/// 管理所有类型节点的创建和查找，确保同一类型只创建一次
pub struct TypeCache {
    /// 核心映射：TypeKey -> NodeIndex
    type_to_node: HashMap<TypeKey, NodeIndex>,

    /// 这些是冗余映射，但可以加速查找
    id_to_node: HashMap<Id, NodeIndex>,
    primitive_to_node: HashMap<String, NodeIndex>,
}

impl TypeCache {
    pub fn new() -> Self {
        Self {
            type_to_node: HashMap::new(),
            id_to_node: HashMap::new(),
            primitive_to_node: HashMap::new(),
        }
    }

    /// 从 Type 查找或创建对应的 NodeIndex
    ///
    /// 如果类型已存在，返回已有的 NodeIndex；
    /// 否则创建新节点并缓存
    pub fn get_or_create_node(&mut self, ty: &Type, context: &TypeContext) -> Option<TypeKey> {
        self.create_type_key(ty, context)
    }

    /// 获取已有的节点
    pub fn get_node(&self, key: &TypeKey) -> Option<NodeIndex> {
        self.type_to_node.get(key).copied()
    }

    /// 插入节点映射
    pub fn insert_node(&mut self, key: TypeKey, node: NodeIndex) {
        // 更新辅助映射
        match &key {
            TypeKey::Resolved(id) => {
                self.id_to_node.insert(*id, node);
            }
            TypeKey::Primitive(name) => {
                self.primitive_to_node.insert(name.clone(), node);
            }
            _ => {}
        }

        self.type_to_node.insert(key, node);
    }

    /// 快速查找：通过 Id 查找节点
    pub fn get_by_id(&self, id: &Id) -> Option<NodeIndex> {
        self.id_to_node.get(id).copied()
    }

    /// 快速查找：通过基本类型名查找节点
    pub fn get_primitive(&self, name: &str) -> Option<NodeIndex> {
        self.primitive_to_node.get(name).copied()
    }

    /// 从 rustdoc Type 创建 TypeKey
    ///
    /// 这是核心方法，将 rustdoc_types::Type 映射到我们的 TypeKey
    pub fn create_type_key(&self, ty: &Type, context: &TypeContext) -> Option<TypeKey> {
        match ty {
            // 1. 有 ID 的类型：直接使用 ID
            Type::ResolvedPath(path) => Some(TypeKey::Resolved(path.id)),

            // 2. 基本类型：使用类型名
            Type::Primitive(name) => Some(TypeKey::Primitive(name.clone())),

            // 3. 泛型参数：需要解析作用域
            Type::Generic(name) => {
                let scope = context
                    .resolve_generic_scope(name)
                    .unwrap_or(GenericScope::Global);
                Some(TypeKey::Generic {
                    name: name.clone(),
                    scope,
                })
            }

            // 4. 引用类型：递归处理内部类型
            Type::BorrowedRef {
                is_mutable, type_, ..
            } => {
                let inner = self.create_type_key(type_, context)?;
                Some(TypeKey::BorrowedRef {
                    is_mutable: *is_mutable,
                    inner: Box::new(inner),
                })
            }

            // 5. 裸指针：递归处理
            Type::RawPointer {
                is_mutable, type_, ..
            } => {
                let inner = self.create_type_key(type_, context)?;
                Some(TypeKey::RawPointer {
                    is_mutable: *is_mutable,
                    inner: Box::new(inner),
                })
            }

            // 6. 切片：递归处理
            Type::Slice(type_) => {
                let inner = self.create_type_key(type_, context)?;
                Some(TypeKey::Slice(Box::new(inner)))
            }

            // 7. 数组：递归处理 + 长度
            Type::Array { type_, len } => {
                let inner = self.create_type_key(type_, context)?;
                Some(TypeKey::Array {
                    inner: Box::new(inner),
                    len: len.clone(),
                })
            }

            // 8. 元组：递归处理每个元素
            Type::Tuple(elements) => {
                let keys: Option<Vec<_>> = elements
                    .iter()
                    .map(|elem| self.create_type_key(elem, context))
                    .collect();
                keys.map(TypeKey::Tuple)
            }

            // 9. 函数指针：序列化签名
            Type::FunctionPointer(fp) => {
                // 简化：使用 Debug 格式作为标识
                Some(TypeKey::FunctionPointer(format!("{:?}", fp)))
            }

            // 10. Trait object：序列化
            Type::DynTrait(dt) => Some(TypeKey::DynTrait(format!("{:?}", dt))),

            // 11. impl Trait：序列化
            Type::ImplTrait(bounds) => Some(TypeKey::ImplTrait(format!("{:?}", bounds))),

            // 12. 关联类型
            Type::QualifiedPath {
                name,
                self_type,
                trait_,
                ..
            } => {
                let self_key = self.create_type_key(self_type, context)?;
                Some(TypeKey::QualifiedPath {
                    name: name.clone(),
                    self_type: Box::new(self_key),
                    trait_id: trait_.as_ref().map(|p| p.id),
                })
            }

            // 13. 类型推断占位符
            Type::Infer => Some(TypeKey::Infer),

            // 14. 模式类型
            Type::Pat {
                type_,
                __pat_unstable_do_not_use,
            } => {
                let inner = self.create_type_key(type_, context)?;
                Some(TypeKey::Pat {
                    inner: Box::new(inner),
                    pattern: __pat_unstable_do_not_use.clone(),
                })
            }
        }
    }
}

impl Default for TypeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for TypeContext {
    fn default() -> Self {
        Self::new()
    }
}
