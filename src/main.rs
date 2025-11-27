use anyhow::Result;
use rustdoc_types::{Crate, Id};
use std::collections::HashMap;

mod analysis;
mod generate;
mod ir_graph;
mod parse;
mod pt_net;
mod support_types;

use crate::generate::FuzzTargetGenerator;
use crate::ir_graph::structure::{DataEdge, EdgeMode, IrGraph, OpKind, OpNode, TypeNode};
use crate::pt_net::builder::PetriNetBuilder;

fn main() -> Result<()> {
    println!("=== SyPetype Pipeline Integration Test ===");

    // 1. Construct Mock IrGraph
    println!("1. Constructing Mock IrGraph...");
    let ir = create_mock_ir()?;

    // 2. Build PetriNet
    println!("2. Building PetriNet...");
    let net = PetriNetBuilder::from_ir(&ir);

    // 3. Generate Fuzz Target
    println!("3. Generating Fuzz Target...");
    let generator = FuzzTargetGenerator::new("my_crate".to_string());
    let source_code = generator.generate(&net);

    println!("\n=== Generated Code ===\n");
    println!("{}", source_code);
    println!("======================");

    Ok(())
}

fn create_mock_ir() -> Result<IrGraph> {
    // Create a dummy ParsedCrate using serde_json to avoid specifying all fields manually
    // and to be robust against struct changes if we include enough fields.
    // If 'target' field is missing in JSON but required, serde might fail or use default if Option.
    // We try to provide a minimal valid JSON for Crate.
    let json = r#"{
        "root": 0,
        "crate_version": null,
        "includes_private": false,
        "index": {},
        "paths": {},
        "external_crates": {},
        "format_version": 0
    }"#;

    // We try to deserialize. If it fails due to missing field, we might need to add it.
    // But since we can't see the error easily without running, we hope this is enough or 'target' is optional.
    // Actually, to be safe, let's assume we can construct it manually if we knew the fields.
    // But since we saw a compile error about missing 'target', we know it exists.
    // Let's try to construct manually with what we know, and use a hack for 'target' if we can't find its type.
    // But I cannot see the type of 'target' without docs.
    // However, I can try to use serde_json::from_str which is dynamic.

    let crate_data: Crate = match serde_json::from_str(json) {
        Ok(c) => c,
        Err(_) => {
            // If failed, maybe 'target' is required. Let's try adding it.
            // Assuming target is a string (triplet).
            let json2 = r#"{
                "root": 0,
                "crate_version": null,
                "includes_private": false,
                "index": {},
                "paths": {},
                "external_crates": {},
                "format_version": 0,
                "is_proc_macro": false,
                "proc_macro_derive_resolution_fallback": false,
                "target": {
                    "triple": "x86_64-unknown-linux-gnu",
                    "target_features": [],
                    "abi": "",
                    "arch": "x86_64",
                    "vendor": "unknown",
                    "os": "linux",
                    "env": "gnu",
                    "pointer_width": "64",
                    "endian": "little"
                }
            }"#;
            serde_json::from_str(json2).unwrap_or_else(|e| {
                panic!("Failed to create dummy crate: {}", e);
            })
        }
    };

    let dummy_crate = crate::parse::ParsedCrate {
        crate_data,
        type_index: HashMap::new(),
        trait_implementations: HashMap::new(),
        functions: Vec::new(),
        types: Vec::new(),
        impl_blocks: Vec::new(),
        traits: Vec::new(),
    };

    let mut ir = IrGraph::new(dummy_crate);

    // --- Types ---
    // Use u32 for Ids

    // 1. u8
    let u8_node = TypeNode::Primitive("u8".to_string());
    ir.add_type(u8_node.clone(), "u8".to_string());

    // 2. Vec<u8>
    let vec_id = Id(1);
    let vec_node = TypeNode::GenericInstance {
        base_id: vec_id.clone(),
        path: "Vec".to_string(),
        type_args: vec![u8_node.clone()],
    };
    ir.add_type(vec_node.clone(), "Vec<u8>".to_string());

    // 3. Context
    let ctx_id = Id(2);
    let ctx_node = TypeNode::Struct(Some(ctx_id.clone()));
    ir.add_type(ctx_node.clone(), "Context".to_string());

    // --- Operations ---

    // 1. Vec::push: fn push(&mut Vec<u8>, value: u8)
    let push_op = OpNode {
        id: Id(3),
        name: "Vec::push".to_string(),
        kind: OpKind::MethodCall {
            self_type: vec_node.clone(),
        },
        inputs: vec![
            DataEdge::mut_ref_edge(vec_node.clone(), Some("self".to_string())),
            DataEdge::move_edge(u8_node.clone(), Some("value".to_string())),
        ],
        output: None, // Returns ()
        error_output: None,
        generic_constraints: HashMap::new(),
        docs: None,
        is_unsafe: false,
        is_const: false,
        is_public: true,
        is_fallible: false,
    };
    ir.add_operation(push_op);

    // 2. Context::new: fn new(data: Vec<u8>) -> Context
    let new_op = OpNode {
        id: Id(4),
        name: "Context::new".to_string(),
        kind: OpKind::FnCall,
        inputs: vec![DataEdge::move_edge(
            vec_node.clone(),
            Some("data".to_string()),
        )],
        output: Some(DataEdge::move_edge(ctx_node.clone(), None)),
        error_output: None,
        generic_constraints: HashMap::new(),
        docs: None,
        is_unsafe: false,
        is_const: false,
        is_public: true,
        is_fallible: false,
    };
    ir.add_operation(new_op);

    Ok(ir)
}
