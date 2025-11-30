use std::collections::HashMap;
use std::sync::LazyLock;

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

/// 方法名到对应 Trait 名的映射
/// 用于在过滤黑名单方法时，记录类型实现了哪些 Trait
pub static METHOD_TO_TRAIT: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut map = HashMap::new();

    // Debug trait
    map.insert("fmt", "Debug");

    // PartialEq trait
    map.insert("eq", "PartialEq");
    map.insert("ne", "PartialEq");

    // Ord trait
    map.insert("cmp", "Ord");

    // PartialOrd trait
    map.insert("partial_cmp", "PartialOrd");

    // Any trait
    map.insert("type_id", "Any");

    // Borrow trait
    map.insert("borrow", "Borrow");

    // BorrowMut trait
    map.insert("borrow_mut", "BorrowMut");

    // AsRef trait
    map.insert("as_ref", "AsRef");

    // AsMut trait
    map.insert("as_mut", "AsMut");

    // Into trait
    map.insert("into", "Into");

    // From trait
    map.insert("from", "From");

    // TryFrom trait
    map.insert("try_from", "TryFrom");

    // TryInto trait
    map.insert("try_into", "TryInto");

    // Default trait
    map.insert("default", "Default");

    // Clone trait
    map.insert("clone", "Clone");
    map.insert("clone_into", "Clone");
    map.insert("clone_to_uninit", "Clone");

    // ToOwned trait
    map.insert("to_owned", "ToOwned");

    // Drop trait
    map.insert("drop", "Drop");

    // Display trait
    map.insert("write_fmt", "Display");

    // ToString trait
    map.insert("to_string", "ToString");

    // Error trait
    map.insert("source", "Error");

    map
});

/// 检查方法是否在黑名单中
pub fn is_blacklisted_method(name: &str) -> bool {
    METHOD_BLACKLIST.contains(&name)
}

/// 获取黑名单方法对应的 Trait 名称
pub fn get_trait_for_method(method_name: &str) -> Option<&'static str> {
    METHOD_TO_TRAIT.get(method_name).copied()
}
