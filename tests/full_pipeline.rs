//! Full Pipeline Integration Test (Self-Check Mode)
//!
//! Test the complete pipeline: ApiGraph → PCPN → Simulator → Emitter.
//! This can be run both as an integration test and directly for a full pipeline self-check.

mod common;

use RustSynth::config::{GoalConfig, ParsedGoal};
use RustSynth::emitter::emit_rust_code;
use RustSynth::pcpn::Pcpn;
use RustSynth::simulator::{SimConfig, Simulator};
use common::*;

/// Run the full pipeline for the Counter example
fn run_counter_pipeline() -> anyhow::Result<()> {
    println!("--- Running Counter Pipeline Self-Check ---");

    // 1. ApiGraph
    let graph = RustSynth::apigraph::build_counter_api_graph();
    println!(
        "Step 1: ApiGraph built with {} nodes.",
        graph.type_nodes.len() + graph.fn_nodes.len()
    );

    // 2. PCPN
    let pcpn = Pcpn::from_api_graph(&graph);
    println!("Step 2: PCPN generated with {} places.", pcpn.places.len());

    // 3. Simulator — goal "own i32"
    let goal = ParsedGoal::parse(&GoalConfig {
        want: "own i32".to_string(),
        count: 1,
    })?;

    let config = SimConfig {
        max_steps: 80,
        stack_depth: 4,
        goal: Some(goal),
        ..Default::default()
    };

    let sim = Simulator::new(&pcpn, config);
    let result = sim.run();

    if result.found {
        println!(
            "Step 3: Goal 'own i32' found in {} steps.",
            result.trace.len()
        );
    } else {
        println!("Step 3: FAILED to find goal.");
        return Err(anyhow::anyhow!("Full pipeline failed to find goal"));
    }

    // 4. Emitter
    let code = emit_rust_code(&result.trace, &pcpn);
    println!(
        "Step 4: Rust code emitted ({} lines).",
        code.lines().count()
    );

    // Simple validation of emitted code
    assert!(code.contains("fn main()"));
    println!("Self-check PASSED.");
    Ok(())
}

#[test]
fn test_full_pipeline_counter() {
    run_counter_pipeline().expect("Full pipeline counter check failed");
}

#[test]
fn test_full_pipeline_borrow() {
    println!("--- Running Borrow Pipeline Check ---");
    let graph = build_borrow_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);
    let goal = ParsedGoal::parse(&GoalConfig {
        want: "own i32".to_string(),
        count: 1,
    })
    .unwrap();

    let config = SimConfig {
        max_steps: 100,
        stack_depth: 4,
        goal: Some(goal),
        deny_transitions: vec!["const_i32".to_string(), "copy_use".to_string()],
        ..Default::default()
    };

    let sim = Simulator::new(&pcpn, config);
    let result = sim.run();
    assert!(result.found);
    let code = emit_rust_code(&result.trace, &pcpn);
    assert!(code.contains("make()"));
    println!("Borrow pipeline check PASSED.");
}

/// If run directly (e.g., as a binary), execute the self-check
fn main() -> anyhow::Result<()> {
    run_counter_pipeline()?;
    Ok(())
}
