/// 基本类型定义
///
/// 这些是 Rust 的内置类型,不需要从 rustdoc JSON 中解析
use std::collections::HashMap;
use std::sync::LazyLock;

/// 原始类型列表
pub const PRIMITIVE_TYPES: &[&str] = &[
    // 整数类型
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
    // 浮点类型
    "f32", "f64", // 布尔和字符
    "bool", "char", // 字符串切片
    "str",  // Never 类型
    "!",
];

/// 检查是否是原始类型
pub fn is_primitive_type(name: &str) -> bool {
    PRIMITIVE_TYPES.contains(&name)
}

/// 基本类型的默认 Trait 实现映射
///
/// 这些是 Rust 编译器为基本类型自动实现的 Trait
pub static PRIMITIVE_DEFAULT_TRAITS: LazyLock<HashMap<&'static str, Vec<&'static str>>> =
    LazyLock::new(|| {
        let mut map = HashMap::new();

        // 所有整数类型共享的 Trait
        let integer_traits = vec![
            "Copy",
            "Clone",
            "Debug",
            "Display",
            "PartialEq",
            "Eq",
            "PartialOrd",
            "Ord",
            "Hash",
            "Default",
            "Send",
            "Sync",
            "Sized",
            // 数值运算 Trait
            "Add",
            "Sub",
            "Mul",
            "Div",
            "Rem",
            "BitAnd",
            "BitOr",
            "BitXor",
            "Shl",
            "Shr",
            "Not",
            "Neg",
            // From/Into 转换
            "From",
            "Into",
            "TryFrom",
            "TryInto",
        ];

        // 有符号整数
        for ty in ["i8", "i16", "i32", "i64", "i128", "isize"] {
            map.insert(ty, integer_traits.clone());
        }

        // 无符号整数
        for ty in ["u8", "u16", "u32", "u64", "u128", "usize"] {
            map.insert(ty, integer_traits.clone());
        }

        // 浮点类型（没有 Eq, Ord, Hash）
        let float_traits = vec![
            "Copy",
            "Clone",
            "Debug",
            "Display",
            "PartialEq",
            "PartialOrd",
            "Default",
            "Send",
            "Sync",
            "Sized",
            "Add",
            "Sub",
            "Mul",
            "Div",
            "Rem",
            "Neg",
            "From",
            "Into",
        ];
        map.insert("f32", float_traits.clone());
        map.insert("f64", float_traits);

        // bool 类型
        map.insert(
            "bool",
            vec![
                "Copy",
                "Clone",
                "Debug",
                "Display",
                "PartialEq",
                "Eq",
                "PartialOrd",
                "Ord",
                "Hash",
                "Default",
                "Send",
                "Sync",
                "Sized",
                "BitAnd",
                "BitOr",
                "BitXor",
                "Not",
                "From",
                "Into",
            ],
        );

        // char 类型
        map.insert(
            "char",
            vec![
                "Copy",
                "Clone",
                "Debug",
                "Display",
                "PartialEq",
                "Eq",
                "PartialOrd",
                "Ord",
                "Hash",
                "Default",
                "Send",
                "Sync",
                "Sized",
                "From",
                "Into",
                "TryFrom",
            ],
        );

        // str 类型（不是 Copy，是 ?Sized）
        map.insert(
            "str",
            vec![
                "Debug",
                "Display",
                "PartialEq",
                "Eq",
                "PartialOrd",
                "Ord",
                "Hash",
                "Send",
                "Sync",
                "ToOwned",
                "AsRef",
            ],
        );

        // Never 类型 (!)
        map.insert(
            "!",
            vec![
                "Copy",
                "Clone",
                "Debug",
                "Display",
                "PartialEq",
                "Eq",
                "PartialOrd",
                "Ord",
                "Hash",
                "Send",
                "Sync",
                "Sized",
            ],
        );

        // unit 类型 ()
        map.insert(
            "()",
            vec![
                "Copy",
                "Clone",
                "Debug",
                "PartialEq",
                "Eq",
                "PartialOrd",
                "Ord",
                "Hash",
                "Default",
                "Send",
                "Sync",
                "Sized",
            ],
        );

        map
    });

/// 获取基本类型的默认 Trait 实现列表
pub fn get_primitive_default_traits(name: &str) -> Vec<String> {
    PRIMITIVE_DEFAULT_TRAITS
        .get(name)
        .map(|traits| traits.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default()
}
