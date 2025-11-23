use rustdoc_types::{GenericParamDefKind, ItemEnum, Path, Type};

use crate::petri::structure::BorrowKind;

/// 标准库/核心库类型白名单
/// 这些类型会自动创建 Place
pub fn is_std_library_type(path: &str) -> bool {
    matches!(
        path,
        // 基本字符串类型
        "String" | "alloc::string::String" | "std::string::String"
            | "str" | "alloc::str" | "std::str"
            // 集合类型
            | "Vec" | "alloc::vec::Vec" | "std::vec::Vec"
            | "VecDeque" | "alloc::collections::VecDeque" | "std::collections::VecDeque"
            | "LinkedList" | "alloc::collections::LinkedList" | "std::collections::LinkedList"
            | "HashMap" | "std::collections::HashMap"
            | "HashSet" | "std::collections::HashSet"
            | "BTreeMap" | "alloc::collections::BTreeMap" | "std::collections::BTreeMap"
            | "BTreeSet" | "alloc::collections::BTreeSet" | "std::collections::BTreeSet"
            // 智能指针
            | "Box" | "alloc::boxed::Box" | "std::boxed::Box"
            | "Rc" | "alloc::rc::Rc" | "std::rc::Rc"
            | "Arc" | "alloc::sync::Arc" | "std::sync::Arc"
            | "Cow" | "alloc::borrow::Cow" | "std::borrow::Cow"
            // 基本数值类型(虽然是 Primitive,但可能以 Path 形式出现)
            | "bool" | "u8" | "u16" | "u32" | "u64" | "u128" | "usize"
            | "i8" | "i16" | "i32" | "i64" | "i128" | "isize"
            | "f32" | "f64" | "char"
            // IO 类型
            | "io::Error" | "std::io::Error"
            | "io::Result" | "std::io::Result"
            // 格式化类型
            | "fmt::Error" | "std::fmt::Error" | "core::fmt::Error"
            | "fmt::Result" | "std::fmt::Result" | "core::fmt::Result"
            | "fmt::Formatter" | "std::fmt::Formatter" | "core::fmt::Formatter"
            | "fmt::Arguments" | "std::fmt::Arguments" | "core::fmt::Arguments"
            // 其他常用类型
            | "PathBuf" | "std::path::PathBuf"
            | "Path" | "std::path::Path"
            | "OsString" | "std::ffi::OsString"
            | "OsStr" | "std::ffi::OsStr"
            | "CString" | "std::ffi::CString"
            | "CStr" | "std::ffi::CStr"
            // 时间类型
            | "Duration" | "std::time::Duration" | "core::time::Duration"
            | "Instant" | "std::time::Instant"
            | "SystemTime" | "std::time::SystemTime"
            // 错误处理
            | "Error" | "std::error::Error"
            // 类型相关
            | "TypeId" | "std::any::TypeId" | "core::any::TypeId"
            | "PhantomData" | "std::marker::PhantomData" | "core::marker::PhantomData"
    )
}

/// 格式化泛型参数信息
pub fn format_generics(generics: &rustdoc_types::Generics) -> String {
    if generics.params.is_empty() {
        return String::new();
    }

    let mut parts = Vec::new();
    for param in &generics.params {
        match &param.kind {
            GenericParamDefKind::Type { bounds, .. } => {
                let mut constraint_str = String::new();
                if !bounds.is_empty() {
                    let bound_strs: Vec<String> = bounds
                        .iter()
                        .filter_map(|bound| {
                            if let rustdoc_types::GenericBound::TraitBound { trait_, .. } = bound {
                                Some(trait_.path.clone())
                            } else {
                                None
                            }
                        })
                        .collect();
                    if !bound_strs.is_empty() {
                        constraint_str = format!(": {}", bound_strs.join(" + "));
                    }
                }
                parts.push(format!("{}{}", param.name, constraint_str));
            }
            GenericParamDefKind::Lifetime { .. } => {
                parts.push(format!("'{}", param.name));
            }
            GenericParamDefKind::Const { .. } => {
                parts.push(format!("const {}", param.name));
            }
        }
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!("泛型参数: <{}>", parts.join(", "))
    }
}

/// 提取类型的借用信息,返回 (借用类型, 实际类型)
///
/// 例如:
/// - `&T` -> (Borrowed, T)
/// - `&mut T` -> (BorrowedMut, T)
/// - `*const T` -> (Borrowed, T) - 原始指针视为借用(不可变)
/// - `*mut T` -> (BorrowedMut, T) - 可变原始指针
/// - `T` -> (Owned, T)
pub fn extract_borrow_info<'t>(ty: &'t Type) -> (BorrowKind, &'t Type) {
    match ty {
        Type::BorrowedRef {
            is_mutable, type_, ..
        } => {
            if *is_mutable {
                (BorrowKind::BorrowedMut, type_.as_ref())
            } else {
                (BorrowKind::Borrowed, type_.as_ref())
            }
        }
        Type::RawPointer { is_mutable, type_ } => {
            // 原始指针也表示一种借用关系
            if *is_mutable {
                (BorrowKind::BorrowedMut, type_.as_ref())
            } else {
                (BorrowKind::Borrowed, type_.as_ref())
            }
        }
        _ => (BorrowKind::Owned, ty),
    }
}

/// 解析标准 trait 的泛型类型到实际类型
///
/// # 参数
/// - `ty`: 原始类型(可能包含泛型参数)
/// - `func_name`: 函数名
/// - `receiver_id`: 接收者类型 ID
/// - `trait_path`: Trait 路径
///
/// # 返回值
/// 返回解析后的类型(将泛型参数映射到实际类型)
pub fn resolve_std_trait_type<'t>(
    ty: &'t Type,
    func_name: &str,
    _receiver_id: Option<rustdoc_types::Id>,
    trait_path: Option<&str>,
) -> &'t Type {
    // 如果不是泛型类型,直接返回
    let Type::Generic(generic_name) = ty else {
        return ty;
    };

    // 如果是 Self,不需要特殊处理
    if generic_name == "Self" {
        return ty;
    }

    let Some(trait_name) = trait_path else {
        return ty;
    };

    // 根据 trait 和方法名决定如何映射泛型
    let should_map_to_self = match (trait_name, func_name) {
        // Borrow<T> 的 borrow() 返回 &T,T 就是 Self
        (t, "borrow") if t.contains("Borrow") && !t.contains("BorrowMut") => generic_name == "T",
        // BorrowMut<T> 的 borrow_mut() 返回 &mut T,T 就是 Self
        (t, "borrow_mut") if t.contains("BorrowMut") => generic_name == "T",
        // AsRef<T> 的 as_ref() 返回 &T
        (t, "as_ref") if t.contains("AsRef") => generic_name == "T",
        // AsMut<T> 的 as_mut() 返回 &mut T
        (t, "as_mut") if t.contains("AsMut") => generic_name == "T",
        // ToOwned 的 to_owned() 返回 Self::Owned,但 T 在 clone_into 中是 Self
        (t, "to_owned" | "clone_into") if t.contains("ToOwned") => generic_name == "T",
        // Clone 的 clone() 和 clone_from() 的 T 就是 Self
        (t, "clone" | "clone_from") if t.contains("Clone") => generic_name == "T",
        // Any 的 type_id(),返回 TypeId(不需要映射)
        (t, "type_id") if t.contains("Any") => false,
        _ => false,
    };

    if should_map_to_self {
        // 对于这些情况,泛型参数 T 实际上就是 Self
        // 但我们不能修改 ty,所以返回原类型
        // 在 find_type_place_in_function 中会特殊处理
        return ty;
    }

    // From<T> 和 Into<U> 的泛型参数不是 Self,保持原样
    // 它们会在查找时失败,这是预期行为
    ty
}

/// 检查是否是标准库 trait 实现
///
/// 这些 trait 的泛型参数需要特殊处理:
/// - Borrow<T>: T 就是 Self
/// - BorrowMut<T>: T 就是 Self
/// - AsRef<T>: T 是目标类型
/// - AsMut<T>: T 是目标类型
/// - From<T>: T 是输入类型,Self 是输出
/// - Into<U>: U 是目标类型,Self 是输入
/// - TryFrom<T>: T 是输入类型
/// - TryInto<U>: U 是目标类型
/// - ToOwned: 返回类型通常是 Self::Owned
pub fn is_standard_trait(trait_path: &str, _func_name: &str) -> bool {
    // 检查常见的标准 trait
    matches!(
        trait_path,
        "Borrow"
            | "BorrowMut"
            | "AsRef"
            | "AsMut"
            | "From"
            | "Into"
            | "TryFrom"
            | "TryInto"
            | "ToOwned"
            | "Clone"
            | "Default"
            | "Any"
    ) || trait_path.ends_with("::Borrow")
        || trait_path.ends_with("::BorrowMut")
        || trait_path.ends_with("::AsRef")
        || trait_path.ends_with("::AsMut")
        || trait_path.ends_with("::From")
        || trait_path.ends_with("::Into")
        || trait_path.ends_with("::TryFrom")
        || trait_path.ends_with("::TryInto")
        || trait_path.ends_with("::ToOwned")
        || trait_path.ends_with("::Clone")
        || trait_path.ends_with("::Default")
        || trait_path.ends_with("::Any")
}

/// 检查类型是否包含指定的泛型参数
pub fn type_has_generic(item_inner: &rustdoc_types::ItemEnum, generic_name: &str) -> bool {
    let generics = match item_inner {
        ItemEnum::Struct(s) => &s.generics,
        ItemEnum::Enum(e) => &e.generics,
        ItemEnum::Union(u) => &u.generics,
        _ => return false,
    };

    generics.params.iter().any(|param| {
        param.name == generic_name && matches!(param.kind, GenericParamDefKind::Type { .. })
    })
}

/// 将 Type 转换为字符串表示
pub fn format_type(ty: &Type) -> String {
    match ty {
        Type::Primitive(name) => name.clone(),
        Type::Tuple(types) => {
            let types_str = types
                .iter()
                .map(|t| format_type(t))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({})", types_str)
        }
        Type::Slice(inner) => format!("[{}]", format_type(inner)),
        Type::Array { type_, len } => format!("[{}; {}]", format_type(type_), len),
        Type::Infer => "_".to_string(),
        Type::RawPointer { is_mutable, type_ } => {
            let mutability = if *is_mutable { "mut" } else { "const" };
            format!("*{} {}", mutability, format_type(type_))
        }
        Type::BorrowedRef {
            is_mutable,
            type_,
            lifetime,
        } => {
            let mutability = if *is_mutable { " mut" } else { "" };
            let lifetime_str = lifetime.as_deref().unwrap_or("");
            format!("&{}{} {}", lifetime_str, mutability, format_type(type_))
        }
        Type::ResolvedPath(path) => format_path(path),
        Type::Generic(name) => name.clone(),
        _ => format!("{:?}", ty),
    }
}

/// 将 Path 转换为字符串
pub fn format_path(path: &Path) -> String {
    path.path.clone()
}
