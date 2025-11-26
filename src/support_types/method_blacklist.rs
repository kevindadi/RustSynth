/// 方法黑名单:过滤对 fuzzing 无意义的方法
///
/// 这些方法通常来自:
/// - 标准库 Trait 的默认实现(Debug, Clone, Default 等)
/// - 编译器自动生成的 Trait 实现(Auto Trait Implementations)
/// - Blanket Implementations(如 `impl<T> Clone for T where T: Clone`)
pub const METHOD_BLACKLIST: &[&str] = &[
    // Debug trait
    "fmt",
    // PartialEq / Eq traits
    "eq",
    "ne",
    "cmp",
    "partial_cmp",
    // Any trait
    "type_id",
    // Borrow / BorrowMut traits
    "borrow",
    "borrow_mut",
    // AsRef / AsMut traits
    "as_ref",
    "as_mut",
    // Into / From traits
    "into",
    "from",
    "try_from",
    "try_into",
    // Default trait
    "default",
    // Clone trait
    "clone",
    "clone_into",
    "clone_to_uninit", // Clone trait 内部方法
    // ToOwned trait
    "to_owned",
    // Drop trait
    "drop",
    // Display / ToString traits
    "write_fmt",
    "to_string",
    // Error trait
    "source",
];

/// 检查方法是否在黑名单中
pub fn is_blacklisted_method(name: &str) -> bool {
    METHOD_BLACKLIST.contains(&name)
}
