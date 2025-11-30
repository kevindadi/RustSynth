/// Trait 黑名单:过滤对 fuzzing 无意义的 Trait
///
/// 只过滤真正的"噪音" Trait：
/// - 标记 Trait(Send, Sync, Sized 等) - 编译器自动实现
/// - 调试/格式化 Trait(Debug, Display) - 对 API 理解无意义
/// - 比较 Trait(PartialEq, Eq 等) - 通常自动派生
/// - 克隆 Trait(Clone, Copy) - 通常自动派生
///
/// **不**过滤重要的 API Trait：
/// - AsRef, AsMut, From, Into, TryFrom, TryInto - 关键的类型转换约束
/// - Borrow, BorrowMut, ToOwned - 所有权相关的重要约束
pub const TRAIT_BLACKLIST: &[&str] = &[
    // 标记 Trait(Marker Traits) - 编译器自动实现
    "Send",
    "Sync",
    "Sized",
    "Unpin",
    // 调试/格式化 Trait - 对 API 理解无意义
    "Debug",
    "Display",
    // 比较 Trait - 通常自动派生
    "PartialEq",
    "Eq",
    "PartialOrd",
    "Ord",
    "Hash",
    // 克隆 Trait - 通常自动派生
    "Clone",
    "Copy",
    // 其他简单的标准 Trait
    "Default",
    "Drop",
    "Any",
    "Error",
];

/// 检查 Trait 是否在黑名单中
pub fn is_blacklisted_trait(trait_path: &str) -> bool {
    TRAIT_BLACKLIST.iter().any(|&blacklisted| {
        trait_path == blacklisted || trait_path.ends_with(&format!("::{}", blacklisted))
    })
}

/// 检查 rustdoc_types::Path 是否指向黑名单 Trait
pub fn is_blacklisted_trait_path(trait_path: &rustdoc_types::Path) -> bool {
    is_blacklisted_trait(&trait_path.path)
}
