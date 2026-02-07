//! 内部类型表示:TypeKey(单态类型)
//!
//! 类型宇宙(已单态化):
//! - Ty ::= T | RefShr(T) | RefMut(T)
//!
//! API Graph 的类型节点不区分 own/shr/mut(借用是边的属性).
//! PCPN 内部需要显式的 ref token 类型.

use serde::{Deserialize, Serialize};
use std::fmt;

/// 单态类型键 - 用于 API Graph 和 PCPN
///
/// 类型宇宙:Ty ::= T | RefShr(T) | RefMut(T)
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

    /// 共享引用类型: &T → RefShr(T)
    RefShr(Box<TypeKey>),

    /// 可变引用类型: &mut T → RefMut(T)
    RefMut(Box<TypeKey>),

    /// 函数指针类型: fn(A, B) -> C
    FnPtr {
        inputs: Vec<TypeKey>,
        output: Box<TypeKey>,
    },

    /// 原始指针: *const T, *mut T
    RawPtr { mutable: bool, inner: Box<TypeKey> },

    /// 关联类型: <T as Trait>::Item
    AssociatedType(String),

    /// 泛型参数(占位符)
    /// - context: 所属上下文(如 "Wrapper", "pair", "Container::push")
    /// - name: 参数名(如 "T", "A", "B")
    /// - bounds: Trait bounds(如 ["Default", "Clone"])
    GenericParam {
        context: String,
        name: String,
        bounds: Vec<String>,
    },

    /// 未解析/未知类型(用于错误处理)
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

    /// 创建路径类型(无泛型参数)
    pub fn path(crate_path: &str) -> Self {
        TypeKey::Path {
            crate_path: crate_path.to_string(),
            args: vec![],
        }
    }

    /// 创建路径类型(带泛型参数)
    pub fn path_with_args(crate_path: &str, args: Vec<TypeKey>) -> Self {
        TypeKey::Path {
            crate_path: crate_path.to_string(),
            args,
        }
    }

    /// 创建共享引用类型 RefShr(T)
    pub fn ref_shr(inner: TypeKey) -> Self {
        TypeKey::RefShr(Box::new(inner))
    }

    /// 创建可变引用类型 RefMut(T)
    pub fn ref_mut(inner: TypeKey) -> Self {
        TypeKey::RefMut(Box::new(inner))
    }

    /// 获取 base 类型(去掉引用)
    pub fn base_type(&self) -> &TypeKey {
        match self {
            TypeKey::RefShr(inner) | TypeKey::RefMut(inner) => inner.base_type(),
            _ => self,
        }
    }

    /// 获取 base 类型(owned)
    pub fn into_base_type(self) -> TypeKey {
        match self {
            TypeKey::RefShr(inner) | TypeKey::RefMut(inner) => inner.into_base_type(),
            other => other,
        }
    }

    /// 是否是引用类型
    pub fn is_ref(&self) -> bool {
        matches!(self, TypeKey::RefShr(_) | TypeKey::RefMut(_))
    }

    /// 是否是共享引用
    pub fn is_ref_shr(&self) -> bool {
        matches!(self, TypeKey::RefShr(_))
    }

    /// 是否是可变引用
    pub fn is_ref_mut(&self) -> bool {
        matches!(self, TypeKey::RefMut(_))
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

    /// 是否是 Copy 类型(简化判断)
    pub fn is_copy(&self) -> bool {
        match self {
            TypeKey::Primitive(_) => true,
            TypeKey::RefShr(_) => true,  // &T is always Copy
            TypeKey::RefMut(_) => false, // &mut T is not Copy
            TypeKey::Tuple(elems) => elems.iter().all(|e| e.is_copy()),
            TypeKey::Array { elem, .. } => elem.is_copy(),
            TypeKey::RawPtr { .. } => true,
            _ => false,
        }
    }

    /// 简化显示(只取最后一段路径)
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
            TypeKey::AssociatedType(path) => path.clone(),
            TypeKey::GenericParam {
                context,
                name,
                bounds,
            } => {
                let prefix = if context.is_empty() {
                    String::new()
                } else {
                    format!("{}::", context)
                };
                if bounds.is_empty() {
                    format!("{}{}", prefix, name)
                } else {
                    format!("{}{}:{}", prefix, name, bounds.join("+"))
                }
            }
            TypeKey::Unknown(s) => format!("?{}", s),
        }
    }

    /// Rust 类型名(用于代码生成)
    pub fn rust_type_name(&self) -> String {
        match self {
            TypeKey::Primitive(s) => s.clone(),
            TypeKey::Path { crate_path, args } => {
                let base = crate_path.split("::").last().unwrap_or(crate_path);
                if args.is_empty() {
                    base.to_string()
                } else {
                    let args_str: Vec<_> = args.iter().map(|a| a.rust_type_name()).collect();
                    format!("{}<{}>", base, args_str.join(", "))
                }
            }
            TypeKey::Tuple(elems) => {
                let elems_str: Vec<_> = elems.iter().map(|e| e.rust_type_name()).collect();
                format!("({})", elems_str.join(", "))
            }
            TypeKey::Slice(inner) => format!("[{}]", inner.rust_type_name()),
            TypeKey::Array { elem, len } => format!("[{}; {}]", elem.rust_type_name(), len),
            TypeKey::RefShr(inner) => format!("&{}", inner.rust_type_name()),
            TypeKey::RefMut(inner) => format!("&mut {}", inner.rust_type_name()),
            TypeKey::FnPtr { inputs, output } => {
                let inputs_str: Vec<_> = inputs.iter().map(|i| i.rust_type_name()).collect();
                format!(
                    "fn({}) -> {}",
                    inputs_str.join(", "),
                    output.rust_type_name()
                )
            }
            TypeKey::RawPtr { mutable, inner } => {
                if *mutable {
                    format!("*mut {}", inner.rust_type_name())
                } else {
                    format!("*const {}", inner.rust_type_name())
                }
            }
            TypeKey::AssociatedType(path) => path.clone(),
            TypeKey::GenericParam { name, .. } => name.clone(),
            TypeKey::Unknown(s) => format!("/* unknown: {} */", s),
        }
    }

    /// 是否是泛型参数
    pub fn is_generic_param(&self) -> bool {
        matches!(self, TypeKey::GenericParam { .. })
    }

    /// 检查类型是否包含任何泛型参数
    pub fn contains_generic_param(&self) -> bool {
        match self {
            TypeKey::GenericParam { .. } => true,
            TypeKey::Path { args, .. } => args.iter().any(|a| a.contains_generic_param()),
            TypeKey::Tuple(elems) => elems.iter().any(|e| e.contains_generic_param()),
            TypeKey::Slice(inner) => inner.contains_generic_param(),
            TypeKey::Array { elem, .. } => elem.contains_generic_param(),
            TypeKey::RefShr(inner) | TypeKey::RefMut(inner) => inner.contains_generic_param(),
            TypeKey::FnPtr { inputs, output } => {
                inputs.iter().any(|i| i.contains_generic_param()) || output.contains_generic_param()
            }
            TypeKey::RawPtr { inner, .. } => inner.contains_generic_param(),
            _ => false,
        }
    }

    /// 收集类型中所有的泛型参数
    pub fn collect_generic_params(&self) -> Vec<(String, String, Vec<String>)> {
        let mut params = Vec::new();
        self.collect_generic_params_inner(&mut params);
        params
    }

    fn collect_generic_params_inner(&self, params: &mut Vec<(String, String, Vec<String>)>) {
        match self {
            TypeKey::GenericParam {
                context,
                name,
                bounds,
            } => {
                let key = (context.clone(), name.clone(), bounds.clone());
                if !params.contains(&key) {
                    params.push(key);
                }
            }
            TypeKey::Path { args, .. } => {
                for arg in args {
                    arg.collect_generic_params_inner(params);
                }
            }
            TypeKey::Tuple(elems) => {
                for elem in elems {
                    elem.collect_generic_params_inner(params);
                }
            }
            TypeKey::Slice(inner) => inner.collect_generic_params_inner(params),
            TypeKey::Array { elem, .. } => elem.collect_generic_params_inner(params),
            TypeKey::RefShr(inner) | TypeKey::RefMut(inner) => {
                inner.collect_generic_params_inner(params)
            }
            TypeKey::FnPtr { inputs, output } => {
                for input in inputs {
                    input.collect_generic_params_inner(params);
                }
                output.collect_generic_params_inner(params);
            }
            TypeKey::RawPtr { inner, .. } => inner.collect_generic_params_inner(params),
            _ => {}
        }
    }

    /// 用具体类型替换泛型参数
    /// substitutions: (context, name) -> concrete_type
    pub fn substitute(
        &self,
        substitutions: &std::collections::HashMap<(String, String), TypeKey>,
    ) -> TypeKey {
        match self {
            TypeKey::GenericParam { context, name, .. } => {
                let key = (context.clone(), name.clone());
                substitutions
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| self.clone())
            }
            TypeKey::Path { crate_path, args } => TypeKey::Path {
                crate_path: crate_path.clone(),
                args: args.iter().map(|a| a.substitute(substitutions)).collect(),
            },
            TypeKey::Tuple(elems) => {
                TypeKey::Tuple(elems.iter().map(|e| e.substitute(substitutions)).collect())
            }
            TypeKey::Slice(inner) => TypeKey::Slice(Box::new(inner.substitute(substitutions))),
            TypeKey::Array { elem, len } => TypeKey::Array {
                elem: Box::new(elem.substitute(substitutions)),
                len: *len,
            },
            TypeKey::RefShr(inner) => TypeKey::RefShr(Box::new(inner.substitute(substitutions))),
            TypeKey::RefMut(inner) => TypeKey::RefMut(Box::new(inner.substitute(substitutions))),
            TypeKey::FnPtr { inputs, output } => TypeKey::FnPtr {
                inputs: inputs.iter().map(|i| i.substitute(substitutions)).collect(),
                output: Box::new(output.substitute(substitutions)),
            },
            TypeKey::RawPtr { mutable, inner } => TypeKey::RawPtr {
                mutable: *mutable,
                inner: Box::new(inner.substitute(substitutions)),
            },
            _ => self.clone(),
        }
    }

    /// 获取泛型参数的 bounds
    pub fn get_bounds(&self) -> Option<&Vec<String>> {
        match self {
            TypeKey::GenericParam { bounds, .. } => Some(bounds),
            _ => None,
        }
    }

    /// 检查是否有特定的 bound
    pub fn has_bound(&self, bound: &str) -> bool {
        self.get_bounds()
            .map(|b| b.iter().any(|s| s == bound))
            .unwrap_or(false)
    }
}

impl fmt::Display for TypeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.short_name())
    }
}

/// 值传递模式(用于 API Graph 的边标注)
///
/// 参数绑定规则:
/// - T (Move/Copy) → Own(T)
/// - &T (BorrowShr) → Own(RefShr(T))
/// - &mut T (BorrowMut) → Own(RefMut(T))
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PassingMode {
    /// 移动所有权 (T where T: !Copy) → 消耗 Own(T)
    Move,
    /// 复制 (T where T: Copy) → 读取 Own(T),不消耗
    Copy,
    /// 共享借用 (&T) → 消耗 Own(RefShr(T)),返回后放回
    BorrowShr,
    /// 可变借用 (&mut T) → 消耗 Own(RefMut(T)),返回后放回
    BorrowMut,
    /// 返回所有权值 → 产生 Own(T)
    ReturnOwned,
    /// 返回共享引用 → 产生 Own(RefShr(T))
    ReturnBorrowShr,
    /// 返回可变引用 → 产生 Own(RefMut(T))
    ReturnBorrowMut,
}

impl PassingMode {
    /// 是否是借用模式
    pub fn is_borrow(&self) -> bool {
        matches!(self, PassingMode::BorrowShr | PassingMode::BorrowMut)
    }

    /// 是否消耗 token
    pub fn consumes(&self) -> bool {
        matches!(
            self,
            PassingMode::Move | PassingMode::BorrowShr | PassingMode::BorrowMut
        )
    }

    /// 是否是返回模式
    pub fn is_return(&self) -> bool {
        matches!(
            self,
            PassingMode::ReturnOwned | PassingMode::ReturnBorrowShr | PassingMode::ReturnBorrowMut
        )
    }
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
        assert_eq!(vec_u8.to_string(), "Vec<u8>");
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

    #[test]
    fn test_base_type() {
        let ref_counter = TypeKey::ref_shr(TypeKey::path("Counter"));
        assert_eq!(ref_counter.base_type(), &TypeKey::path("Counter"));

        let ref_mut_counter = TypeKey::ref_mut(TypeKey::path("Counter"));
        assert_eq!(ref_mut_counter.base_type(), &TypeKey::path("Counter"));
    }
}
