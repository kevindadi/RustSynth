pub mod impl_filter;
/// 支持类型和过滤逻辑模块
///
/// 该模块提供:
/// - 方法黑名单(过滤无意义的方法)
/// - Trait 黑名单(过滤无意义的 Trait)
/// - 基本类型定义(原始类型列表)
/// - Impl 块过滤(过滤 Auto Trait 和 Blanket Implementations)
pub mod method_blacklist;
pub mod primitives;
pub mod trait_blacklist;

// 重新导出常用函数和常量
pub use impl_filter::should_filter_impl;
pub use method_blacklist::is_blacklisted_method;
pub use primitives::is_primitive_type;
pub use trait_blacklist::{TRAIT_BLACKLIST, is_blacklisted_trait, is_blacklisted_trait_path};
