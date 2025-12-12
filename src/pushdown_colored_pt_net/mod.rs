//! 下推着色 Petri 网 (Pushdown Colored Petri Net, PCPN) 表示及从 IrGraph 的转换
//!
//! # 设计说明
//!
//! ## 下推着色 Petri 网基本概念
//!
//! 下推着色 Petri 网结合了两种扩展：
//!
//! 1. **着色 Petri 网 (Colored Petri Net)**:
//!    - Token 有颜色（类型），不同类型的 token 可以区分
//!    - 例如：`u8` 类型的 token 和 `String` 类型的 token 是不同的
//!    - 弧可以指定需要的 token 颜色
//!
//! 2. **下推自动机 (Pushdown Automaton)**:
//!    - 每个变迁可以操作一个栈（push/pop）
//!    - 用于模拟上下文相关的操作，如：
//!      - 作用域嵌套（函数调用栈）
//!      - 泛型参数实例化
//!      - 生命周期作用域
//!
//! ## 从 IrGraph 的映射
//!
//! - **Place (库所)**: 数据类型节点，每个 place 可以存储多种颜色的 token
//! - **Transition (变迁)**: 操作节点，可以：
//!   - 消耗特定颜色的 token
//!   - 产生特定颜色的 token
//!   - 操作栈（push/pop）
//! - **Arc (弧)**: 带有颜色约束的边
//! - **Stack (栈)**: 用于跟踪上下文信息
//!
//! ## 在 Rust 代码分析中的应用
//!
//! 1. **类型系统**: 不同类型的 token 表示不同的 Rust 类型
//! 2. **作用域**: 栈用于模拟函数调用和作用域嵌套
//! 3. **泛型**: 栈用于跟踪泛型参数的实例化
//! 4. **生命周期**: 栈用于跟踪生命周期作用域

pub mod net;
pub mod export;
pub mod analysis;
pub mod unfolding;
pub mod unfolding_fuzz;
#[cfg(test)]
mod fuzz_example;

pub use net::{PushdownColoredPetriNet, TokenColor, StackOperation, PcpnStats};
pub use analysis::{PcpnAnalysis, FuzzEntryInfo};
pub use unfolding::{UnfoldedPetriNet, UnfoldingConfig, UnfoldingStats, unfold_petri_net};
pub use unfolding_fuzz::UnfoldingBasedFuzzer;
