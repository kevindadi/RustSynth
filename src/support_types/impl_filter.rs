/// Impl 块过滤:识别并过滤编译器自动实现和 Blanket Implementations

/// 检查是否是编译器自动 Trait 实现(Auto Trait Implementation)
///
/// Auto Traits 包括:Send, Sync, Sized, Unpin 等
/// rustdoc 中这些实现通常标记为 `is_auto: true`
pub fn is_auto_trait_impl(impl_item: &rustdoc_types::Impl) -> bool {
    // 检查 impl 块是否标记为 auto
    // 注意:rustdoc_types 可能没有直接字段,需要通过 trait 名称判断
    if let Some(trait_ref) = &impl_item.trait_ {
        let trait_name = &trait_ref.path;
        matches!(
            trait_name.as_str(),
            "Send" | "Sync" | "Sized" | "Unpin" | "Copy"
        ) || trait_name.ends_with("::Send")
            || trait_name.ends_with("::Sync")
            || trait_name.ends_with("::Sized")
            || trait_name.ends_with("::Unpin")
            || trait_name.ends_with("::Copy")
    } else {
        false
    }
}

/// 检查是否是 Blanket Implementation
///
/// Blanket Implementations 是标准库提供的通用实现,如:
/// - `impl<T> Clone for T where T: Clone`
/// - `impl<T> From<T> for T`
///
/// 这些实现通常:
/// 1. 在标准库中定义(item.crate_id != 0)
/// 2. 有泛型参数但没有具体类型
/// 3. Trait 在黑名单中
pub fn is_blanket_impl(
    impl_item: &rustdoc_types::Impl,
    item_crate_id: u32,
    trait_blacklist: &[&str],
) -> bool {
    // 标准库的 Blanket Implementation(非当前 crate)
    if item_crate_id != 0 {
        return true;
    }

    // 检查 Trait 是否在黑名单中
    if let Some(trait_ref) = &impl_item.trait_ {
        let trait_path = &trait_ref.path;
        trait_blacklist.iter().any(|&blacklisted| {
            trait_path == blacklisted || trait_path.ends_with(&format!("::{}", blacklisted))
        })
    } else {
        false
    }
}

/// 检查 impl 块是否应该被过滤
///
/// # 参数
/// - `impl_item`: impl 块定义
/// - `item_crate_id`: Item 的 crate_id(从 `item.crate_id` 获取)
/// - `trait_blacklist`: Trait 黑名单
pub fn should_filter_impl(
    impl_item: &rustdoc_types::Impl,
    item_crate_id: u32,
    trait_blacklist: &[&str],
) -> bool {
    is_auto_trait_impl(impl_item) || is_blanket_impl(impl_item, item_crate_id, trait_blacklist)
}
