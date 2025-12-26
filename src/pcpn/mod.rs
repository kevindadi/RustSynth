//! 下推着色 Petri 网 (Pushdown Colored Petri Net, PCPN)
//!
//! 这个模块实现了完整的下推着色 Petri 网模型，用于 Rust API 序列的形式化验证。
//!
//! ## 网结构 N = (P, T, F, C, G, Stack)
//!
//! - **P**: 库所集合 (Places) - 表示类型/数据状态
//! - **T**: 变迁集合 (Transitions) - 表示 API 操作
//! - **F**: 流关系 (Flow) - 弧集合，包括输入弧和输出弧
//! - **C**: 颜色集合 (Colors) - 类型表达式
//! - **G**: 守卫函数 (Guards) - 变迁使能条件
//! - **Stack**: 下推栈 - 跟踪借用和作用域
//!
//! ## 变迁分类
//!
//! 1. **结构变迁 (S1-S16)**: 所有权和借用操作
//! 2. **签名诱导变迁**: API 函数调用
//! 3. **构造变迁**: 自动类型构造（基本类型、Copy/Clone、const fn）
//!
//! ## 发生规则
//!
//! 变迁 t 在配置 (M, σ) 下使能当且仅当:
//! 1. ∀ 输入弧 (p, t): M(p) 包含满足颜色约束的足够 token
//! 2. 守卫条件 G(t) 满足
//! 3. 栈操作前置条件满足
//!
//! 触发变迁产生新配置 (M', σ'):
//! 1. 消耗输入 token
//! 2. 产生输出 token
//! 3. 执行栈操作

pub mod types;
pub mod place;
pub mod transition;
pub mod arc;
pub mod marking;
pub mod stack;
pub mod net;
pub mod firing;
pub mod reachability;
pub mod witness;
pub mod builder;
pub mod config;
pub mod codegen;

// 重新导出核心类型
pub use types::{TypeId, RustType, TypeRegistry, Constructibility, PrimitiveKind, Mutability};
pub use place::{PlaceId, Place, PlaceKind};
pub use transition::{
    TransitionId, Transition, TransitionKind, StructuralKind,
    SignatureInfo, ParamInfo, ParamPassing, SelfKind, AutoConstructMethod,
};
pub use arc::{Arc, ArcId, ArcKind};
pub use marking::{Token, Marking, MultiSet, ValueId, ValueIdGen};
pub use stack::{StackFrame, PushdownStack, StackOp};
pub use net::{PcpnNet, NetStats};
pub use firing::{Config as PcpnState, FiringRule, EnabledBinding};
pub use reachability::{
    ReachabilityAnalyzer, ReachabilityResult, ApiSequence, ApiCall,
    SearchConfig, SearchStrategy,
};
pub use witness::{Witness, WitnessStep};
pub use builder::PcpnBuilder;
pub use config::{PcpnConfig, GenerationMode, LlmPromptTemplate};
pub use codegen::{CodeGenerator, GeneratedCode, Placeholder};

