/// Trait 黑名单:过滤对 fuzzing 无意义的 Trait
///
/// 这些 Trait 通常来自:
/// - 标准库的标记 Trait(Send, Sync, Sized 等)
/// - 编译器自动实现的 Trait(Auto Trait)
/// - Blanket Implementations 的 Trait
pub const TRAIT_BLACKLIST: &[&str] = &[
    // 标记 Trait(Marker Traits)
    "Send",
    "Sync",
    "Sized",
    "Unpin",
    // 比较 Trait
    "Debug",
    "Display",
    "PartialEq",
    "Eq",
    "PartialOrd",
    "Ord",
    "Hash",
    // 转换 Trait
    "Clone",
    "Copy",
    "Borrow",
    "BorrowMut",
    "From",
    "Into",
    "TryFrom",
    "TryInto",
    "AsRef",
    "AsMut",
    "ToOwned",
    // 其他标准 Trait
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
