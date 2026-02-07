mod common;

use RustSynth::config::{GoalConfig, ParsedGoal};
use RustSynth::emitter::emit_rust_code;
use RustSynth::pcpn::Pcpn;
use RustSynth::simulator::{SimConfig, Simulator};
use common::*;

#[test]
fn test_full_pipeline_borrow_api_codegen() {
    let graph = build_borrow_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);

    let goal = ParsedGoal::parse(&GoalConfig {
        want: "own i32".to_string(),
        count: 1,
    })
    .unwrap();

    let config = SimConfig {
        max_steps: 80,
        stack_depth: 4,
        goal: Some(goal),
        deny_transitions: vec!["const_i32".to_string(), "copy_use".to_string()],
        ..Default::default()
    };

    let sim = Simulator::new(&pcpn, config);
    let result = sim.run();
    assert!(result.found);

    let code = emit_rust_code(&result.trace, &pcpn);
    assert!(code.contains("fn main()"));
    assert!(code.contains("make()"));
}

#[test]
fn test_dot_output() {
    let graph = RustSynth::apigraph::build_counter_api_graph();

    // ApiGraph DOT
    let api_dot = graph.to_dot();
    assert!(api_dot.starts_with("digraph ApiGraph {"));
    assert!(api_dot.contains("Counter"));

    // PCPN DOT
    let pcpn = Pcpn::from_api_graph(&graph);
    let pcpn_dot = pcpn.to_dot();
    assert!(pcpn_dot.starts_with("digraph PCPN {"));

    // Reachability DOT
    let config = SimConfig {
        max_steps: 100,
        stack_depth: 3,
        default_bound: 2,
        ..Default::default()
    };
    let sim = Simulator::new(&pcpn, config);
    let rg = sim.generate_reachability_graph(50);
    let rg_dot = rg.to_dot(&pcpn);
    assert!(rg_dot.starts_with("digraph Reachability {"));
}
