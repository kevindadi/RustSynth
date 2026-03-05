//! RustSynth - Pushdown CPN Safe Rust Synthesizer
//!
//! Extract API signatures from rustdoc JSON, build a Pushdown Colored Petri Net,
//! and synthesize compilable Safe Rust code snippets via bounded reachability search.

#![allow(non_snake_case)]

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
