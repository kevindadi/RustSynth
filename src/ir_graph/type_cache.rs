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
    /// 直接使用 rustdoc 的 Id（无泛型参数）
    Resolved(Id),

    /// 有 ID 的类型，带泛型参数实例化
    /// 例如：Vec<u8>, Vec<u16> 是不同的实例
    ResolvedWithArgs { id: Id, args: Vec<TypeKey> },

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

    /// 操作节点（方法）
    /// 使用 rustdoc 的 Id 作为标识
    Operation(Id),

    /// 关联类型节点
    /// 格式为 "TypeName.AssocTypeName" 或 "TraitName.AssocTypeName"
    AssociatedType {
        /// 所属类型或 Trait 的名称
        owner_name: String,
        /// 关联类型的名称
        assoc_name: String,
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
///
/// ## 统一管理的节点类型
///
/// - **类型节点** (`id_to_node`): Struct, Enum, Union, Trait, TypeAlias, Constant, Static 等
/// - **操作节点** (`op_to_node`): 方法（ImplMethod, TraitMethod）
/// - **泛型节点** (`generic_to_node`): 泛型参数，使用 "OwnerName:GenericName" 作为 key
/// - **关联类型节点** (`assoc_type_to_node`): 关联类型，使用 "OwnerName.AssocTypeName" 作为 key
/// - **基本类型节点** (`primitive_to_node`): u8, i32, bool, str, () 等
pub struct TypeCache {
    /// 核心映射：TypeKey -> NodeIndex
    type_to_node: HashMap<TypeKey, NodeIndex>,

    // ========== 快速查找索引 ==========
    // 这些是冗余映射，但可以加速查找

    /// 有 ID 的类型节点：Struct, Enum, Union, Trait, TypeAlias, Constant, Static 等
    id_to_node: HashMap<Id, NodeIndex>,

    /// 操作节点（方法）
    op_to_node: HashMap<Id, NodeIndex>,

    /// 泛型参数节点
    /// Key 格式：
    /// - 完整名: "OwnerName:GenericName" (如 "Vec:T", "Iterator:Item")
    /// - 短名: "GenericName" (如 "T", "Item") - 可能被覆盖
    generic_to_node: HashMap<String, NodeIndex>,

    /// 关联类型节点
    /// Key 格式: "OwnerName.AssocTypeName" (如 "Iterator.Item", "MyType.Output")
    assoc_type_to_node: HashMap<String, NodeIndex>,

    /// 基本类型节点
    primitive_to_node: HashMap<String, NodeIndex>,
}

impl TypeCache {
    pub fn new() -> Self {
        Self {
            type_to_node: HashMap::new(),
            id_to_node: HashMap::new(),
            op_to_node: HashMap::new(),
            generic_to_node: HashMap::new(),
            assoc_type_to_node: HashMap::new(),
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

    /// 获取 TypeCache 中的节点总数（用于调试）
    pub fn total_count(&self) -> usize {
        self.type_to_node.len()
    }

    /// 插入节点映射
    pub fn insert_node(&mut self, key: TypeKey, node: NodeIndex) {
        match &key {
            TypeKey::Resolved(id) => {
                self.id_to_node.insert(*id, node);
            }
            TypeKey::ResolvedWithArgs { id, .. } => {
                // 对于带泛型参数的类型，也更新 id_to_node（用于快速查找基础类型）
                // 但主要映射在 type_to_node 中
                self.id_to_node.insert(*id, node);
            }
            TypeKey::Primitive(name) => {
                self.primitive_to_node.insert(name.clone(), node);
            }
            TypeKey::Operation(id) => {
                self.op_to_node.insert(*id, node);
            }
            TypeKey::Generic { name, scope } => {
                // 插入完整名（OwnerName:GenericName）
                let full_key = match scope {
                    GenericScope::Type(id) | GenericScope::Trait(id) | GenericScope::Method(id) | GenericScope::Function(id) => {
                        // 这里我们只存储到 generic_to_node，完整 key 需要外部提供
                        // 因为我们没有 owner_name 信息
                    }
                    GenericScope::Global => {}
                };
                // 短名也存储（可能被覆盖）
                self.generic_to_node.insert(name.clone(), node);
            }
            TypeKey::AssociatedType { owner_name, assoc_name } => {
                let key = format!("{}.{}", owner_name, assoc_name);
                self.assoc_type_to_node.insert(key, node);
            }
            _ => {}
        }

        self.type_to_node.insert(key, node);
    }

    // ========== 类型节点（有 ID）操作 ==========

    /// 通过 Id 查找类型节点
    pub fn get_by_id(&self, id: &Id) -> Option<NodeIndex> {
        self.id_to_node.get(id).copied()
    }

    /// 插入类型节点（有 ID）
    pub fn insert_type_by_id(&mut self, id: Id, node: NodeIndex) {
        self.id_to_node.insert(id, node);
        self.type_to_node.insert(TypeKey::Resolved(id), node);
    }

    /// 检查类型节点是否存在
    pub fn contains_type_id(&self, id: &Id) -> bool {
        self.id_to_node.contains_key(id)
    }

    /// 获取所有类型节点的 ID 列表（用于调试）
    pub fn type_ids(&self) -> impl Iterator<Item = &Id> {
        self.id_to_node.keys()
    }

    // ========== 操作节点（方法）操作 ==========

    /// 通过 Id 查找操作节点
    pub fn get_op_by_id(&self, id: &Id) -> Option<NodeIndex> {
        self.op_to_node.get(id).copied()
    }

    /// 插入操作节点
    pub fn insert_op(&mut self, id: Id, node: NodeIndex) {
        self.op_to_node.insert(id, node);
        self.type_to_node.insert(TypeKey::Operation(id), node);
    }

    /// 检查操作节点是否存在
    pub fn contains_op_id(&self, id: &Id) -> bool {
        self.op_to_node.contains_key(id)
    }

    // ========== 泛型节点操作 ==========

    /// 通过完整名查找泛型节点（如 "Vec:T"）
    pub fn get_generic(&self, full_name: &str) -> Option<NodeIndex> {
        self.generic_to_node.get(full_name).copied()
    }

    /// 通过短名查找泛型节点（如 "T"）
    /// 注意：短名可能被覆盖，优先使用完整名
    pub fn get_generic_short(&self, short_name: &str) -> Option<NodeIndex> {
        self.generic_to_node.get(short_name).copied()
    }

    /// 插入泛型节点
    /// - full_name: 完整名，如 "Vec:T"
    /// - short_name: 短名，如 "T"（可选，可能被覆盖）
    pub fn insert_generic(&mut self, full_name: String, short_name: Option<String>, node: NodeIndex) {
        self.generic_to_node.insert(full_name, node);
        if let Some(short) = short_name {
            self.generic_to_node.insert(short, node);
        }
    }

    /// 插入泛型节点（使用 TypeKey）
    pub fn insert_generic_with_key(&mut self, key: TypeKey, full_name: String, short_name: Option<String>, node: NodeIndex) {
        self.generic_to_node.insert(full_name, node);
        if let Some(short) = short_name {
            self.generic_to_node.insert(short, node);
        }
        self.type_to_node.insert(key, node);
    }

    /// 检查泛型节点是否存在
    pub fn contains_generic(&self, name: &str) -> bool {
        self.generic_to_node.contains_key(name)
    }

    // ========== 关联类型节点操作 ==========

    /// 通过 key 查找关联类型节点（如 "Iterator.Item"）
    pub fn get_assoc_type_by_key(&self, key: &str) -> Option<NodeIndex> {
        self.assoc_type_to_node.get(key).copied()
    }

    /// 通过 owner_name 和 assoc_name 查找关联类型节点
    pub fn get_assoc_type(&self, owner_name: &str, assoc_name: &str) -> Option<NodeIndex> {
        let key = format!("{}.{}", owner_name, assoc_name);
        self.assoc_type_to_node.get(&key).copied()
    }

    /// 插入关联类型节点
    /// - owner_name: 所属类型或 Trait 的名称
    /// - assoc_name: 关联类型的名称
    pub fn insert_assoc_type(&mut self, owner_name: &str, assoc_name: &str, node: NodeIndex) {
        let key = format!("{}.{}", owner_name, assoc_name);
        self.assoc_type_to_node.insert(key.clone(), node);
        self.type_to_node.insert(TypeKey::AssociatedType {
            owner_name: owner_name.to_string(),
            assoc_name: assoc_name.to_string(),
        }, node);
    }

    /// 检查关联类型节点是否存在
    pub fn contains_assoc_type(&self, key: &str) -> bool {
        self.assoc_type_to_node.contains_key(key)
    }

    // ========== 基本类型节点操作 ==========

    /// 通过基本类型名查找节点
    pub fn get_primitive(&self, name: &str) -> Option<NodeIndex> {
        self.primitive_to_node.get(name).copied()
    }

    /// 插入基本类型节点
    pub fn insert_primitive(&mut self, name: String, node: NodeIndex) {
        self.primitive_to_node.insert(name.clone(), node);
        self.type_to_node.insert(TypeKey::Primitive(name), node);
    }

    /// 获取 primitive_to_node 的引用
    pub fn primitive_to_node(&self) -> &HashMap<String, NodeIndex> {
        &self.primitive_to_node
    }

    // ========== 统计信息 ==========

    /// 获取各类节点的数量统计
    pub fn stats(&self) -> TypeCacheStats {
        TypeCacheStats {
            total: self.type_to_node.len(),
            types: self.id_to_node.len(),
            ops: self.op_to_node.len(),
            generics: self.generic_to_node.len(),
            assoc_types: self.assoc_type_to_node.len(),
            primitives: self.primitive_to_node.len(),
        }
    }
}

/// TypeCache 统计信息
#[derive(Debug, Clone)]
pub struct TypeCacheStats {
    pub total: usize,
    pub types: usize,
    pub ops: usize,
    pub generics: usize,
    pub assoc_types: usize,
    pub primitives: usize,
}

impl TypeCache {
    /// 从 rustdoc Type 创建 TypeKey
    pub fn create_type_key(&self, ty: &Type, context: &TypeContext) -> Option<TypeKey> {
        match ty {
            // 1. 有 ID 的类型：检查是否有泛型参数
            Type::ResolvedPath(path) => {
                // 如果有泛型参数，需要递归处理每个参数
                if let Some(args) = &path.args {
                    if let rustdoc_types::GenericArgs::AngleBracketed { args, .. } = &**args {
                        let mut type_args = Vec::new();
                        for arg in args {
                            if let rustdoc_types::GenericArg::Type(arg_type) = arg {
                                if let Some(arg_key) = self.create_type_key(arg_type, context) {
                                    type_args.push(arg_key);
                                } else {
                                    // 如果无法解析某个参数，回退到无参数版本
                                    return Some(TypeKey::Resolved(path.id));
                                }
                            } else {
                                // 非类型参数（如 lifetime），忽略
                            }
                        }
                        if !type_args.is_empty() {
                            return Some(TypeKey::ResolvedWithArgs {
                                id: path.id,
                                args: type_args,
                            });
                        }
                    }
                }
                // 无泛型参数，使用简单版本
                Some(TypeKey::Resolved(path.id))
            }

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
