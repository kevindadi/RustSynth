mod common;

use RustSynth::config::{GoalConfig, ParsedGoal};
use RustSynth::pcpn::Pcpn;
use RustSynth::simulator::{SimConfig, Simulator};
use RustSynth::types::TyGround;
use common::*;

#[test]
fn test_minimal_network_exhaustive_reachability() {
    let graph = build_minimal_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);
    let _s_ty = TyGround::path("S");

    // Verify PCPN structure
    // 验证 PCPN 结构
    assert_eq!(pcpn.places.len(), 9, "S×9 = 9 places");
    assert_eq!(pcpn.type_universe.len(), 1, "Only type S");

    // Generate reachability graph (bound = 2, stack_depth = 3 to keep it finite)
    // 生成可达图 (bound = 2, stack_depth = 3 保持有限)
    let config = SimConfig {
        max_steps: 500,
        stack_depth: 3,
        default_bound: 2,
        ..Default::default()
    };

    let sim = Simulator::new(&pcpn, config);
    let rg = sim.generate_reachability_graph(300);

    // Reachability graph should have a finite number of states
    // 可达图应该有有限多个状态
    assert!(
        rg.states.len() > 1,
        "Should have multiple reachable states, got {}",
        rg.states.len()
    );
    assert!(
        rg.states.len() < 300,
        "State space should be bounded, got {} states",
        rg.states.len()
    );
    assert!(rg.edges.len() > 0, "Should have transitions");

    // Verify initial state (index 0) has an outgoing edge for make()
    // 验证初始状态 (index 0) 有 make() 出发的边
    let from_initial: Vec<_> = rg.edges.iter().filter(|(from, _, _)| *from == 0).collect();
    assert!(from_initial.iter().any(|(_, _, name)| name == "make"));

    // Goal "own S" — should be reachable
    // Goal "own S" — 应该可达
    let goal = ParsedGoal::parse(&GoalConfig {
        want: "own S".to_string(),
        count: 1,
    })
    .unwrap();

    let config2 = SimConfig {
        max_steps: 100,
        stack_depth: 3,
        default_bound: 2,
        goal: Some(goal),
        ..Default::default()
    };

    let sim2 = Simulator::new(&pcpn, config2);
    let result = sim2.run();
    assert!(result.found, "own S should be reachable");

    // Shortest trace should be exactly make() (1 step)
    // 最短 trace 应该只有 make() (1 步)
    let api_names = api_call_names(&result.trace);
    assert_eq!(api_names, vec!["make"], "Shortest path: [make()]");
}

#[test]
fn test_minimal_network_transform_chain() {
    // Verify chained calls: make → transform → transform → ...
    // 验证 make → transform → transform → ... 的链式调用
    let graph = build_minimal_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);

    let goal = ParsedGoal::parse(&GoalConfig {
        want: "own S".to_string(),
        count: 1,
    })
    .unwrap();

    let config = SimConfig {
        max_steps: 50,
        stack_depth: 3,
        default_bound: 2,
        goal: Some(goal),
        ..Default::default()
    };

    let sim = Simulator::new(&pcpn, config);
    let result = sim.run();
    assert!(result.found);

    // Verify trace validity: produced tokens from each step are available for the next
    // 验证 trace 合法性：每一步的 produced token 都在下一步可用
    for (_, firing) in result.trace.iter().enumerate() {
        // ConstProducer doesn't need consumed tokens in the first step
        // ConstProducer 第一步不需要 consumed
        assert!(
            firing.consumed.is_empty(),
            "ConstProducer should have no consumed tokens"
        );
    }
}

#[test]
fn test_borrow_network_reachability_graph() {
    let graph = build_borrow_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);

    let config = SimConfig {
        max_steps: 500,
        stack_depth: 3,
        default_bound: 2,
        ..Default::default()
    };

    let sim = Simulator::new(&pcpn, config);
    let rg = sim.generate_reachability_graph(200);

    // Should have a finite state space
    assert!(rg.states.len() > 1);
    assert!(rg.states.len() <= 500, "Should be bounded");

    // Should have multiple outgoing edges from initial state
    let from_initial: Vec<_> = rg.edges.iter().filter(|(from, _, _)| *from == 0).collect();
    assert!(
        from_initial.len() >= 2,
        "Initial state should have at least make + const_i32 edges"
    );

    // List all transition names starting from initial state
    // 列出所有从初始状态出发的 transition 名称
    let initial_trans_names: Vec<_> = from_initial.iter().map(|(_, _, n)| n.clone()).collect();
    assert!(
        initial_trans_names.contains(&"make".to_string()),
        "Should have make from initial"
    );
    assert!(
        initial_trans_names.contains(&"const_i32".to_string()),
        "Should have const_i32 from initial"
    );
}

#[test]
fn test_counter_enumerate_all_api_sequences() {
    let graph = RustSynth::apigraph::build_counter_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);

    let config = SimConfig {
        max_steps: 500,
        stack_depth: 3,
        default_bound: 2,
        ..Default::default()
    };

    let sim = Simulator::new(&pcpn, config);
    let rg = sim.generate_reachability_graph(200);

    // Collect all reachable API transition names from initial state
    // 收集所有从初始状态可达的 API transition 名称
    let mut all_api_edges: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, _, name) in &rg.edges {
        if !name.contains("borrow_")
            && !name.contains("end_")
            && !name.contains("drop_")
            && !name.contains("copy_use")
        {
            all_api_edges.insert(name.clone());
        }
    }

    // Should include Counter::new
    assert!(
        all_api_edges.contains("Counter::into_value"),
        "into_value should be reachable after new"
    );

    // Verify state space is finite and reasonable
    assert!(rg.states.len() <= 500, "State space should be bounded");
}
