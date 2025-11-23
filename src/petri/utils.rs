use rustdoc_types::GenericParamDefKind;

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
                            if let rustdoc_types::GenericBound::TraitBound { trait_, .. } =
                                bound
                            {
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