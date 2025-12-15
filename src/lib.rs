//! Rustdoc Petri Net Builder Library
//!
//! 这个库提供了从 rustdoc JSON 构建 Petri 网的功能

pub mod config;
pub mod ir_graph;
pub mod parse;
pub mod petri_net_traits;
pub mod pipeline;
pub mod pushdown_colored_pt_net;
pub mod support_types;
pub mod llm_client;

pub use ir_graph::*;
pub use parse::ParsedCrate;
