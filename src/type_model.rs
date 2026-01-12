//! 内部类型表示：TypeKey（单态类型）
//!
//! 作为 HashMap key、图节点、PCPN place 索引；必须稳定可序列化。
//! API Graph 的类型节点不区分 own/shr/mut（borrowing 是边的属性）。
//! PCPN 内部需要显式的 ref token 类型。

use serde::{Deserialize, Serialize};
use std::fmt;

/// 单态类型键 - 用于 API Graph 和 PCPN
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TypeKey {
    /// 原始类型: "u32", "bool", "str", "()"
    Primitive(String),

    /// 路径类型: "alloc::vec::Vec<u8>", "std::option::Option<i32>"
    Path {
        crate_path: String,
        args: Vec<TypeKey>,
    },

    /// 元组类型: (A, B, C)
    Tuple(Vec<TypeKey>),

    /// 切片类型: [T]
    Slice(Box<TypeKey>),

    /// 数组类型: [T; N]
    Array { elem: Box<TypeKey>, len: usize },

    /// 共享引用类型（PCPN 内部使用）: &T
    RefShr(Box<TypeKey>),

    /// 可变引用类型（PCPN 内部使用）: &mut T
    RefMut(Box<TypeKey>),

    /// 函数指针类型: fn(A, B) -> C
    FnPtr {
        inputs: Vec<TypeKey>,
        output: Box<TypeKey>,
    },

    /// 原始指针: *const T, *mut T
    RawPtr { mutable: bool, inner: Box<TypeKey> },

    /// 未解析/未知类型（用于错误处理）
    Unknown(String),
}

impl TypeKey {
    /// 创建 unit 类型 ()
    pub fn unit() -> Self {
        TypeKey::Primitive("()".to_string())
    }

    /// 创建 primitive 类型
    pub fn primitive(name: &str) -> Self {
        TypeKey::Primitive(name.to_string())
    }

    /// 创建路径类型（无泛型参数）
    pub fn path(crate_path: &str) -> Self {
        TypeKey::Path {
            crate_path: crate_path.to_string(),
            args: vec![],
        }
    }

    /// 创建路径类型（带泛型参数）
    pub fn path_with_args(crate_path: &str, args: Vec<TypeKey>) -> Self {
        TypeKey::Path {
            crate_path: crate_path.to_string(),
            args,
        }
    }

    /// 创建共享引用类型
    pub fn ref_shr(inner: TypeKey) -> Self {
        TypeKey::RefShr(Box::new(inner))
    }

    /// 创建可变引用类型
    pub fn ref_mut(inner: TypeKey) -> Self {
        TypeKey::RefMut(Box::new(inner))
    }

    /// 获取 base 类型（去掉引用）
    pub fn base_type(&self) -> &TypeKey {
        match self {
            TypeKey::RefShr(inner) | TypeKey::RefMut(inner) => inner.base_type(),
            _ => self,
        }
    }

    /// 是否是引用类型
    pub fn is_ref(&self) -> bool {
        matches!(self, TypeKey::RefShr(_) | TypeKey::RefMut(_))
    }

    /// 是否是原始类型
    pub fn is_primitive(&self) -> bool {
        match self {
            TypeKey::Primitive(name) => matches!(
                name.as_str(),
                "bool"
                    | "char"
                    | "str"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "usize"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "isize"
                    | "f32"
                    | "f64"
                    | "()"
            ),
            _ => false,
        }
    }

    /// 是否是 Copy 类型（简化判断）
    pub fn is_copy(&self) -> bool {
        match self {
            TypeKey::Primitive(_) => true,
            TypeKey::RefShr(_) => true, // &T is always Copy
            TypeKey::RefMut(_) => false, // &mut T is not Copy
            TypeKey::Tuple(elems) => elems.iter().all(|e| e.is_copy()),
            TypeKey::Array { elem, .. } => elem.is_copy(),
            TypeKey::RawPtr { .. } => true,
            _ => false,
        }
    }

    /// 简化显示（只取最后一段路径）
    pub fn short_name(&self) -> String {
        match self {
            TypeKey::Primitive(s) => s.clone(),
            TypeKey::Path { crate_path, args } => {
                let base = crate_path.split("::").last().unwrap_or(crate_path);
                if args.is_empty() {
                    base.to_string()
                } else {
                    let args_str: Vec<_> = args.iter().map(|a| a.short_name()).collect();
                    format!("{}<{}>", base, args_str.join(", "))
                }
            }
            TypeKey::Tuple(elems) => {
                let elems_str: Vec<_> = elems.iter().map(|e| e.short_name()).collect();
                format!("({})", elems_str.join(", "))
            }
            TypeKey::Slice(inner) => format!("[{}]", inner.short_name()),
            TypeKey::Array { elem, len } => format!("[{}; {}]", elem.short_name(), len),
            TypeKey::RefShr(inner) => format!("&{}", inner.short_name()),
            TypeKey::RefMut(inner) => format!("&mut {}", inner.short_name()),
            TypeKey::FnPtr { inputs, output } => {
                let inputs_str: Vec<_> = inputs.iter().map(|i| i.short_name()).collect();
                format!("fn({}) -> {}", inputs_str.join(", "), output.short_name())
            }
            TypeKey::RawPtr { mutable, inner } => {
                if *mutable {
                    format!("*mut {}", inner.short_name())
                } else {
                    format!("*const {}", inner.short_name())
                }
            }
            TypeKey::Unknown(s) => format!("?{}", s),
        }
    }
}

impl fmt::Display for TypeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeKey::Primitive(s) => write!(f, "{}", s),
            TypeKey::Path { crate_path, args } => {
                if args.is_empty() {
                    write!(f, "{}", crate_path)
                } else {
                    let args_str: Vec<_> = args.iter().map(|a| a.to_string()).collect();
                    write!(f, "{}<{}>", crate_path, args_str.join(", "))
                }
            }
            TypeKey::Tuple(elems) => {
                let elems_str: Vec<_> = elems.iter().map(|e| e.to_string()).collect();
                write!(f, "({})", elems_str.join(", "))
            }
            TypeKey::Slice(inner) => write!(f, "[{}]", inner),
            TypeKey::Array { elem, len } => write!(f, "[{}; {}]", elem, len),
            TypeKey::RefShr(inner) => write!(f, "&{}", inner),
            TypeKey::RefMut(inner) => write!(f, "&mut {}", inner),
            TypeKey::FnPtr { inputs, output } => {
                let inputs_str: Vec<_> = inputs.iter().map(|i| i.to_string()).collect();
                write!(f, "fn({}) -> {}", inputs_str.join(", "), output)
            }
            TypeKey::RawPtr { mutable, inner } => {
                if *mutable {
                    write!(f, "*mut {}", inner)
                } else {
                    write!(f, "*const {}", inner)
                }
            }
            TypeKey::Unknown(s) => write!(f, "?{}", s),
        }
    }
}

/// 值传递模式（用于 API Graph 的边标注）
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PassingMode {
    /// 移动所有权 (T where T: !Copy)
    Move,
    /// 复制 (T where T: Copy)
    Copy,
    /// 共享借用 (&T)
    BorrowShr,
    /// 可变借用 (&mut T)
    BorrowMut,
    /// 返回所有权值
    ReturnOwned,
    /// 返回共享引用
    ReturnBorrowShr,
    /// 返回可变引用
    ReturnBorrowMut,
}

impl fmt::Display for PassingMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PassingMode::Move => write!(f, "move"),
            PassingMode::Copy => write!(f, "copy"),
            PassingMode::BorrowShr => write!(f, "&"),
            PassingMode::BorrowMut => write!(f, "&mut"),
            PassingMode::ReturnOwned => write!(f, "→"),
            PassingMode::ReturnBorrowShr => write!(f, "→&"),
            PassingMode::ReturnBorrowMut => write!(f, "→&mut"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_key_display() {
        let vec_u8 = TypeKey::path_with_args("alloc::vec::Vec", vec![TypeKey::primitive("u8")]);
        assert_eq!(vec_u8.to_string(), "alloc::vec::Vec<u8>");
        assert_eq!(vec_u8.short_name(), "Vec<u8>");

        let ref_i32 = TypeKey::ref_shr(TypeKey::primitive("i32"));
        assert_eq!(ref_i32.to_string(), "&i32");
    }

    #[test]
    fn test_is_copy() {
        assert!(TypeKey::primitive("i32").is_copy());
        assert!(TypeKey::ref_shr(TypeKey::primitive("i32")).is_copy());
        assert!(!TypeKey::ref_mut(TypeKey::primitive("i32")).is_copy());
        assert!(!TypeKey::path("String").is_copy());
    }
}
