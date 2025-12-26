//! 统一的类型系统
//!
//! 定义 Rust 类型的表示和类型注册表

use std::collections::HashMap;
use std::fmt;
use serde::{Deserialize, Serialize};

/// 类型 ID (全局唯一)
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TypeId(pub u32);

impl TypeId {
    pub fn new(id: u32) -> Self {
        TypeId(id)
    }
}

impl fmt::Display for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "T{}", self.0)
    }
}

/// Rust 类型表示
///
/// 统一的类型表达式，合并了之前的 TokenColor 和 TypeExpr
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum RustType {
    /// 基本类型 (u8, i32, bool, char, str, ())
    Primitive(PrimitiveKind),

    /// 具名类型 (struct, enum, union)
    Named {
        /// 完整路径 (如 "std::collections::HashMap")
        path: String,
        /// 泛型参数
        type_args: Vec<RustType>,
    },

    /// 引用类型
    Reference {
        mutability: Mutability,
        lifetime: Option<String>,
        inner: Box<RustType>,
    },

    /// 裸指针
    RawPointer {
        mutability: Mutability,
        inner: Box<RustType>,
    },

    /// 切片 [T]
    Slice(Box<RustType>),

    /// 数组 [T; N]
    Array {
        elem: Box<RustType>,
        len: usize,
    },

    /// 元组 (T1, T2, ...)
    Tuple(Vec<RustType>),

    /// 函数指针 fn(T1, T2) -> R
    FnPointer {
        params: Vec<RustType>,
        ret: Box<RustType>,
        is_unsafe: bool,
        abi: Option<String>,
    },

    /// 泛型参数 (未实例化)
    Generic {
        name: String,
        /// 所属作用域 (函数/类型的 ID)
        scope: String,
    },

    /// 关联类型 <T as Trait>::Assoc
    AssociatedType {
        base: Box<RustType>,
        trait_path: String,
        assoc_name: String,
    },

    /// dyn Trait
    DynTrait {
        traits: Vec<String>,
        lifetime: Option<String>,
    },

    /// impl Trait (存在类型)
    ImplTrait(Vec<String>),

    /// Never 类型 !
    Never,
}

/// 基本类型种类
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimitiveKind {
    Bool,
    Char,
    Str,
    U8, U16, U32, U64, U128, Usize,
    I8, I16, I32, I64, I128, Isize,
    F32, F64,
    Unit,
}

impl PrimitiveKind {
    /// 从字符串解析
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "bool" => Some(Self::Bool),
            "char" => Some(Self::Char),
            "str" => Some(Self::Str),
            "()" => Some(Self::Unit),
            "u8" => Some(Self::U8),
            "u16" => Some(Self::U16),
            "u32" => Some(Self::U32),
            "u64" => Some(Self::U64),
            "u128" => Some(Self::U128),
            "usize" => Some(Self::Usize),
            "i8" => Some(Self::I8),
            "i16" => Some(Self::I16),
            "i32" => Some(Self::I32),
            "i64" => Some(Self::I64),
            "i128" => Some(Self::I128),
            "isize" => Some(Self::Isize),
            "f32" => Some(Self::F32),
            "f64" => Some(Self::F64),
            _ => None,
        }
    }

    /// 是否是 Copy 类型
    pub fn is_copy(&self) -> bool {
        // 所有基本类型都是 Copy
        true
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Char => "char",
            Self::Str => "str",
            Self::Unit => "()",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::U128 => "u128",
            Self::Usize => "usize",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::I128 => "i128",
            Self::Isize => "isize",
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }
}

/// 可变性
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mutability {
    Shared,
    Mutable,
}

impl RustType {
    /// 创建共享引用
    pub fn shared_ref(inner: RustType) -> Self {
        RustType::Reference {
            mutability: Mutability::Shared,
            lifetime: None,
            inner: Box::new(inner),
        }
    }

    /// 创建可变引用
    pub fn mut_ref(inner: RustType) -> Self {
        RustType::Reference {
            mutability: Mutability::Mutable,
            lifetime: None,
            inner: Box::new(inner),
        }
    }

    /// 是否是引用类型
    pub fn is_reference(&self) -> bool {
        matches!(self, RustType::Reference { .. })
    }

    /// 是否是可变引用
    pub fn is_mut_ref(&self) -> bool {
        matches!(self, RustType::Reference { mutability: Mutability::Mutable, .. })
    }

    /// 获取引用的内部类型
    pub fn deref(&self) -> Option<&RustType> {
        match self {
            RustType::Reference { inner, .. } => Some(inner),
            _ => None,
        }
    }

    /// 检查是否是基本类型（总是可以自动构造）
    pub fn is_primitive(&self) -> bool {
        matches!(self, RustType::Primitive(_))
    }

    /// 转换为简短的字符串表示
    pub fn short_name(&self) -> String {
        match self {
            RustType::Primitive(p) => p.as_str().to_string(),
            RustType::Named { path, type_args } => {
                let name = path.rsplit("::").next().unwrap_or(path);
                if type_args.is_empty() {
                    name.to_string()
                } else {
                    let args: Vec<_> = type_args.iter().map(|t| t.short_name()).collect();
                    format!("{}<{}>", name, args.join(", "))
                }
            }
            RustType::Reference { mutability, inner, .. } => {
                let m = match mutability {
                    Mutability::Shared => "&",
                    Mutability::Mutable => "&mut ",
                };
                format!("{}{}", m, inner.short_name())
            }
            RustType::Slice(inner) => format!("[{}]", inner.short_name()),
            RustType::Array { elem, len } => format!("[{}; {}]", elem.short_name(), len),
            RustType::Tuple(types) => {
                let parts: Vec<_> = types.iter().map(|t| t.short_name()).collect();
                format!("({})", parts.join(", "))
            }
            RustType::Generic { name, .. } => name.clone(),
            _ => format!("{:?}", self),
        }
    }
}

impl fmt::Display for RustType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.short_name())
    }
}

/// 类型的可构造性
///
/// 标记类型是否可以自动构造，以及如何构造
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Constructibility {
    /// 可以无限自动构造（基本类型）
    Unlimited,

    /// 有限次数自动构造（Copy/Clone 类型）
    Limited {
        /// 可以复制的次数
        budget: usize,
        /// 构造方式
        method: ConstructMethod,
    },

    /// 通过 const fn 构造
    ConstFn {
        /// const fn 的完整路径
        fn_path: String,
        /// 参数类型（必须也是可构造的）
        params: Vec<TypeId>,
    },

    /// 不可自动构造（需要通过 API 调用产生）
    NotConstructible,
}

/// 构造方法
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConstructMethod {
    /// 通过 Default trait
    Default,
    /// 通过 Clone trait
    Clone,
    /// 通过 Copy（隐式）
    Copy,
    /// 通过字面量
    Literal,
}

/// 类型注册表
///
/// 管理所有类型及其属性
#[derive(Debug, Clone, Default)]
pub struct TypeRegistry {
    /// 类型 ID -> 类型定义
    pub types: HashMap<TypeId, RustType>,
    /// 类型 -> 类型 ID (用于去重)
    type_to_id: HashMap<RustType, TypeId>,
    /// 类型 ID -> 可构造性
    constructibility: HashMap<TypeId, Constructibility>,
    /// 类型 ID -> 实现的 traits
    trait_impls: HashMap<TypeId, Vec<String>>,
    /// 下一个可用的类型 ID
    next_id: u32,
}

impl TypeRegistry {
    pub fn new() -> Self {
        let mut registry = Self::default();
        // 预注册所有基本类型
        registry.register_primitives();
        registry
    }

    /// 注册所有基本类型
    fn register_primitives(&mut self) {
        let primitives = [
            PrimitiveKind::Bool,
            PrimitiveKind::Char,
            PrimitiveKind::Str,
            PrimitiveKind::Unit,
            PrimitiveKind::U8, PrimitiveKind::U16, PrimitiveKind::U32,
            PrimitiveKind::U64, PrimitiveKind::U128, PrimitiveKind::Usize,
            PrimitiveKind::I8, PrimitiveKind::I16, PrimitiveKind::I32,
            PrimitiveKind::I64, PrimitiveKind::I128, PrimitiveKind::Isize,
            PrimitiveKind::F32, PrimitiveKind::F64,
        ];

        for prim in primitives {
            let ty = RustType::Primitive(prim);
            let id = self.register(ty);
            // 基本类型无限可构造
            self.constructibility.insert(id, Constructibility::Unlimited);
            // 基本类型实现的 traits
            self.trait_impls.insert(id, vec![
                "Copy".to_string(),
                "Clone".to_string(),
                "Debug".to_string(),
            ]);
        }
    }

    /// 注册类型，返回类型 ID
    pub fn register(&mut self, ty: RustType) -> TypeId {
        if let Some(&id) = self.type_to_id.get(&ty) {
            return id;
        }

        let id = TypeId::new(self.next_id);
        self.next_id += 1;
        self.types.insert(id, ty.clone());
        self.type_to_id.insert(ty, id);
        id
    }

    /// 获取类型
    pub fn get(&self, id: TypeId) -> Option<&RustType> {
        self.types.get(&id)
    }

    /// 获取类型 ID
    pub fn get_id(&self, ty: &RustType) -> Option<TypeId> {
        self.type_to_id.get(ty).copied()
    }

    /// 设置类型的可构造性
    pub fn set_constructibility(&mut self, id: TypeId, c: Constructibility) {
        self.constructibility.insert(id, c);
    }

    /// 获取类型的可构造性
    pub fn get_constructibility(&self, id: TypeId) -> &Constructibility {
        self.constructibility.get(&id).unwrap_or(&Constructibility::NotConstructible)
    }

    /// 检查类型是否是 Copy
    pub fn is_copy(&self, id: TypeId) -> bool {
        if let Some(ty) = self.get(id) {
            match ty {
                RustType::Primitive(_) => true,
                RustType::Reference { mutability: Mutability::Shared, .. } => true,
                _ => self.trait_impls.get(&id)
                    .map(|traits| traits.iter().any(|t| t == "Copy"))
                    .unwrap_or(false),
            }
        } else {
            false
        }
    }

    /// 检查类型是否实现了指定的 trait
    pub fn implements_trait(&self, id: TypeId, trait_name: &str) -> bool {
        self.trait_impls.get(&id)
            .map(|traits| traits.iter().any(|t| t == trait_name))
            .unwrap_or(false)
    }

    /// 添加 trait 实现
    pub fn add_trait_impl(&mut self, id: TypeId, trait_name: String) {
        self.trait_impls.entry(id).or_default().push(trait_name);
    }

    /// 检查类型是否可以自动构造
    pub fn can_auto_construct(&self, id: TypeId) -> bool {
        !matches!(
            self.get_constructibility(id),
            Constructibility::NotConstructible
        )
    }

    /// 类型统一检查
    pub fn unify(&self, ty1: TypeId, ty2: TypeId) -> bool {
        if ty1 == ty2 {
            return true;
        }

        match (self.get(ty1), self.get(ty2)) {
            (Some(t1), Some(t2)) => self.types_unify(t1, t2),
            _ => false,
        }
    }

    /// 类型结构统一检查
    fn types_unify(&self, ty1: &RustType, ty2: &RustType) -> bool {
        match (ty1, ty2) {
            (RustType::Primitive(p1), RustType::Primitive(p2)) => p1 == p2,
            (RustType::Generic { .. }, _) => true, // 泛型可以匹配任何类型
            (_, RustType::Generic { .. }) => true,
            (RustType::Reference { mutability: m1, inner: i1, .. },
             RustType::Reference { mutability: m2, inner: i2, .. }) => {
                m1 == m2 && self.types_unify(i1, i2)
            }
            (RustType::Named { path: p1, type_args: a1 },
             RustType::Named { path: p2, type_args: a2 }) => {
                p1 == p2 && a1.len() == a2.len()
                    && a1.iter().zip(a2.iter()).all(|(t1, t2)| self.types_unify(t1, t2))
            }
            _ => ty1 == ty2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_registry() {
        let mut registry = TypeRegistry::new();

        let u8_ty = RustType::Primitive(PrimitiveKind::U8);
        let id = registry.register(u8_ty.clone());

        assert!(registry.is_copy(id));
        assert!(registry.can_auto_construct(id));
    }

    #[test]
    fn test_reference_types() {
        let inner = RustType::Primitive(PrimitiveKind::U8);
        let shared_ref = RustType::shared_ref(inner.clone());
        let mut_ref = RustType::mut_ref(inner);

        assert!(shared_ref.is_reference());
        assert!(!shared_ref.is_mut_ref());
        assert!(mut_ref.is_mut_ref());
    }
}

