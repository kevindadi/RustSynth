//! RustSynth - Pushdown CPN Safe Rust Synthesizer
//!
//! 从 rustdoc JSON 提取 API 签名，构建 Pushdown Colored Petri Net，
//! 通过有界可达性搜索生成可编译的 Safe Rust 代码片段。

pub mod apigraph;
pub mod config;
pub mod emitter;
pub mod extract;
pub mod lifetime_analyzer;
pub mod pcpn;
pub mod rustdoc_loader;
pub mod simulator;
pub mod type_model;
pub mod types;
pub mod unify;
