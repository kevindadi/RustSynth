/// 基本类型定义
///
/// 这些是 Rust 的内置类型,不需要从 rustdoc JSON 中解析

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
