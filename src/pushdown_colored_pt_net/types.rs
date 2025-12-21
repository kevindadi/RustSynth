//! 类型系统定义和类型谓词
//!
//! 定义了 PCPN 中使用的类型表达式和类型相关的谓词函数

use std::fmt;

/// 类型表达式
/// 
/// 表示 Rust 中的类型,包括基本类型、引用类型、泛型等
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum TypeExpr {
    /// 基本类型 (如 u8, i32, bool, str)
    Primitive(String),
    /// 复合类型 (如 Vec<T>, Option<T>)
    Composite {
        name: String,
        type_args: Vec<TypeExpr>,
    },
    /// 泛型参数 (未实例化)
    Generic {
        name: String,
        scope: String,
    },
    /// 关联类型
    AssociatedType {
        owner: String,
        assoc_name: String,
    },
    /// 元组类型
    Tuple(Vec<TypeExpr>),
    /// 共享引用类型 &T
    Reference {
        mutable: bool,
        inner: Box<TypeExpr>,
    },
    /// 切片类型 [T]
    Slice(Box<TypeExpr>),
    /// 数组类型 [T; N]
    Array {
        elem: Box<TypeExpr>,
        len: usize,
    },
    /// 函数指针类型
    FnPtr {
        params: Vec<TypeExpr>,
        ret: Box<TypeExpr>,
    },
}

impl TypeExpr {
    /// 创建共享引用类型
    pub fn shared_ref(inner: TypeExpr) -> Self {
        TypeExpr::Reference {
            mutable: false,
            inner: Box::new(inner),
        }
    }

    /// 创建可变引用类型
    pub fn mut_ref(inner: TypeExpr) -> Self {
        TypeExpr::Reference {
            mutable: true,
            inner: Box::new(inner),
        }
    }

    /// 检查是否是引用类型
    pub fn is_reference(&self) -> bool {
        matches!(self, TypeExpr::Reference { .. })
    }

    /// 检查是否是可变引用类型
    pub fn is_mut_reference(&self) -> bool {
        matches!(self, TypeExpr::Reference { mutable: true, .. })
    }

    /// 检查是否是共享引用类型
    pub fn is_shared_reference(&self) -> bool {
        matches!(self, TypeExpr::Reference { mutable: false, .. })
    }

    /// 获取引用内部类型
    pub fn as_reference_inner(&self) -> Option<&TypeExpr> {
        match self {
            TypeExpr::Reference { inner, .. } => Some(inner),
            _ => None,
        }
    }

    /// 转换为字符串表示
    pub fn to_string(&self) -> String {
        match self {
            TypeExpr::Primitive(name) => name.clone(),
            TypeExpr::Composite { name, type_args } => {
                if type_args.is_empty() {
                    name.clone()
                } else {
                    let args_str = type_args
                        .iter()
                        .map(|t| t.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{}<{}>", name, args_str)
                }
            }
            TypeExpr::Generic { name, scope } => format!("{}@{}", name, scope),
            TypeExpr::AssociatedType { owner, assoc_name } => {
                format!("{}::{}", owner, assoc_name)
            }
            TypeExpr::Tuple(types) => {
                let types_str = types
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({})", types_str)
            }
            TypeExpr::Reference { mutable, inner } => {
                let mut_str = if *mutable { "mut " } else { "" };
                format!("&{} {}", mut_str, inner.to_string())
            }
            TypeExpr::Slice(inner) => format!("[{}]", inner.to_string()),
            TypeExpr::Array { elem, len } => format!("[{}; {}]", elem.to_string(), len),
            TypeExpr::FnPtr { params, ret } => {
                let params_str = params
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("fn({}) -> {}", params_str, ret.to_string())
            }
        }
    }
}

impl fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

/// 类型谓词函数
pub mod predicates {
    use super::TypeExpr;

    /// 检查类型是否是引用类型
    pub fn is_ref_ty(ty: &TypeExpr) -> bool {
        ty.is_reference()
    }

    /// 检查类型是否是可变引用类型
    pub fn is_mut_ref_ty(ty: &TypeExpr) -> bool {
        ty.is_mut_reference()
    }

    /// 检查类型是否是共享引用类型
    pub fn is_shared_ref_ty(ty: &TypeExpr) -> bool {
        ty.is_shared_reference()
    }

    /// 检查类型是否是基本 Copy 类型
    /// 
    /// 注意: 这是一个简化的实现。在实际系统中,这应该由 Env trait 提供,
    /// 因为它依赖于类型定义和 trait 实现。
    pub fn is_copy_type(ty: &TypeExpr) -> bool {
        match ty {
            TypeExpr::Primitive(name) => {
                // 所有基本类型都是 Copy
                matches!(
                    name.as_str(),
                    "u8" | "u16" | "u32" | "u64" | "usize"
                        | "i8" | "i16" | "i32" | "i64" | "isize"
                        | "bool" | "char" | "f32" | "f64"
                )
            }
            TypeExpr::Reference { .. } => {
                // 引用类型是 Copy
                true
            }
            TypeExpr::Generic { .. } => {
                // 泛型参数默认假设是 Copy (实际应该从约束推断)
                false
            }
            _ => false,
        }
    }

    /// 检查类型是否是借用类型 (引用类型)
    pub fn is_borrow_ty(ty: &TypeExpr) -> bool {
        is_ref_ty(ty)
    }
}

/// 值 ID
/// 
/// 用于唯一标识标记中的每个值 token
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ValueId(pub u64);

impl ValueId {
    /// 创建新的值 ID
    pub fn new(id: u64) -> Self {
        ValueId(id)
    }

    /// 获取下一个值 ID (用于生成新值)
    pub fn next(&self) -> Self {
        ValueId(self.0 + 1)
    }
}

impl fmt::Display for ValueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// 颜色 (Color)
/// 
/// 在 PCPN 中,颜色表示类型和值的组合
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Color {
    /// 类型
    pub ty: TypeExpr,
    /// 值 ID (用于区分同一类型的不同值)
    pub value_id: ValueId,
}

impl Color {
    /// 创建新颜色
    pub fn new(ty: TypeExpr, value_id: ValueId) -> Self {
        Color { ty, value_id }
    }

    /// 检查是否是引用类型的颜色
    pub fn is_reference(&self) -> bool {
        self.ty.is_reference()
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.ty, self.value_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_expr() {
        let u8_ty = TypeExpr::Primitive("u8".to_string());
        assert_eq!(u8_ty.to_string(), "u8");

        let vec_ty = TypeExpr::Composite {
            name: "Vec".to_string(),
            type_args: vec![u8_ty.clone()],
        };
        assert_eq!(vec_ty.to_string(), "Vec<u8>");

        let ref_ty = TypeExpr::shared_ref(u8_ty);
        assert_eq!(ref_ty.to_string(), "& u8");
        assert!(ref_ty.is_reference());
        assert!(ref_ty.is_shared_reference());
    }

    #[test]
    fn test_color() {
        let ty = TypeExpr::Primitive("u8".to_string());
        let color = Color::new(ty, ValueId::new(1));
        assert_eq!(format!("{}", color), "u8:v1");
    }
}
