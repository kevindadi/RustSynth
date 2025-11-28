pub mod builder;
pub mod export;
pub mod generic_scope;
/// IR Graph 模块:中间表示层
///
/// 这一层将 rustdoc JSON 转换为扁平的、语义化的可调用路径图.
/// 1. 类型节点代表规范类型(不区分 &T 和 T)
/// 2. 所有权信息存储在边上(Move, &, &mut 等)
/// 3. 统一处理函数、构造器等操作
pub mod structure;

pub use builder::IrGraphBuilder;
pub use structure::{IrGraph, TypeNode};
