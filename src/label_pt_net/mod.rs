//! Labeled Petri Net (LPN) 表示及从 IrGraph 的转换
//!
//! # 设计说明
//!
//! ## Petri 网基本概念
//! - **Place(库所)**:表示系统状态或资源,对应数据类型节点
//! - **Transition(变迁)**:表示状态转换或操作,对应方法/函数节点
//! - **Arc(弧)**:连接 Place 和 Transition,表示数据流
//! - **Token(令牌)**:Place 中的标记,表示资源数量
//!
//! ## 从 IrGraph 的映射
//! - 数据节点 (Struct, Enum, Constant, Primitive 等) → Place
//! - 操作节点 (Method, Function, UnwrapOp) → Transition
//! - 边 (TypeRelation) → Arc,EdgeMode 作为标签
//!
//! ## 守卫逻辑(Guard)
//! 在完整的 Petri 网模拟中,某些变迁需要守卫条件:
//! - `Ref`/`MutRef` 边:借用检查(同时只能有一个 MutRef 或多个 Ref)
//! - `Implements` 边:Trait 约束检查
//! - `UnwrapOp`:Result/Option 分支选择
//!

pub mod analysis;
pub(crate) mod export;
pub(crate) mod net;
mod shims;

pub use analysis::{AnalysisResult, ApiSequence, FuzzInputParser};
