mod common;

use RustSynth::config::{GoalConfig, ParsedGoal};
use RustSynth::pcpn::{Pcpn, TransitionKind};
use RustSynth::simulator::{SimConfig, SimState, Simulator};
use RustSynth::types::{Capability, Token, TyGround, TypeForm};
use common::*;

#[test]
fn test_borrow_network_all_sequences() {
    let graph = build_borrow_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);

    // --- Path 1: const_i32 ---
    // --- 路径 1: const_i32 ---
    {
        let goal = ParsedGoal::parse(&GoalConfig {
            want: "own i32".to_string(),
            count: 1,
        })
        .unwrap();

        let config = SimConfig {
            max_steps: 50,
            stack_depth: 4,
            goal: Some(goal),
            ..Default::default()
        };

        let sim = Simulator::new(&pcpn, config);
        let result = sim.run();
        assert!(result.found, "const_i32 path should work");
        assert!(result.trace.len() <= 3);
    }

    // --- Path 2: make → consume (deny const_i32, copy_use) ---
    // --- 路径 2: make → consume (禁止 const_i32, copy_use) ---
    {
        let goal = ParsedGoal::parse(&GoalConfig {
            want: "own i32".to_string(),
            count: 1,
        })
        .unwrap();

        let config = SimConfig {
            max_steps: 50,
            stack_depth: 4,
            goal: Some(goal),
            deny_transitions: vec!["const_i32".to_string(), "copy_use".to_string()],
            ..Default::default()
        };

        let sim = Simulator::new(&pcpn, config);
        let result = sim.run();
        assert!(result.found, "make → consume path should work");

        let api_names = api_call_names(&result.trace);
        assert!(api_names.contains(&"make".to_string()));
        assert!(
            api_names.contains(&"consume".to_string()) || api_names.contains(&"peek".to_string())
        );
    }

    // --- Path 3: make → borrow → peek (deny const_i32, copy_use, consume) ---
    // --- 路径 3: make → borrow → peek (禁止 const_i32, copy_use, consume) ---
    {
        let goal = ParsedGoal::parse(&GoalConfig {
            want: "own i32".to_string(),
            count: 1,
        })
        .unwrap();

        let config = SimConfig {
            max_steps: 80,
            stack_depth: 4,
            goal: Some(goal),
            deny_transitions: vec![
                "const_i32".to_string(),
                "copy_use".to_string(),
                "consume".to_string(),
            ],
            ..Default::default()
        };

        let sim = Simulator::new(&pcpn, config);
        let result = sim.run();
        assert!(result.found, "make → borrow → peek path should work");

        let api_names = api_call_names(&result.trace);
        assert!(api_names.contains(&"make".to_string()));
        assert!(api_names.contains(&"peek".to_string()));

        let all_names = all_transition_names(&result.trace);
        assert!(all_names.iter().any(|n| n.contains("borrow_shr")));
    }
}

#[test]
fn test_copy_type_semantics() {
    let graph = build_borrow_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);
    let i32_ty = TyGround::primitive("i32");

    let copy_t = pcpn
        .transitions
        .iter()
        .find(|t| matches!(&t.kind, TransitionKind::CopyUse { ty } if *ty == i32_ty))
        .expect("i32 should have CopyUse");

    let own_i32 = pcpn
        .get_place(&i32_ty, &TypeForm::Value, Capability::Own)
        .unwrap();

    let mut state = SimState::new();
    let vid = state.fresh_vid();
    state
        .marking
        .add(own_i32, Token::new_owned(vid, i32_ty.clone()));

    let config = SimConfig::default();
    let sim = Simulator::new(&pcpn, config);

    if let Some((consume, read)) = sim.enabled(copy_t, &state) {
        let (state2, _) = sim.fire(copy_t, &state, &consume, &read).unwrap();
        assert_eq!(state2.marking.count(own_i32), 2);
    } else {
        panic!("copy_use should be enabled");
    }
}

#[test]
fn test_primitive_creation() {
    let graph = build_borrow_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);
    let i32_ty = TyGround::primitive("i32");

    let const_t = pcpn
        .transitions
        .iter()
        .find(|t| matches!(&t.kind, TransitionKind::CreatePrimitive { ty } if *ty == i32_ty))
        .expect("Should have const_i32");

    let state = SimState::new();
    let own_i32 = pcpn
        .get_place(&i32_ty, &TypeForm::Value, Capability::Own)
        .unwrap();

    let config = SimConfig::default();
    let sim = Simulator::new(&pcpn, config);

    if let Some((consume, read)) = sim.enabled(const_t, &state) {
        let (state2, _) = sim.fire(const_t, &state, &consume, &read).unwrap();
        assert_eq!(state2.marking.count(own_i32), 1);
    } else {
        panic!("const_i32 should always be enabled");
    }
}

#[test]
fn test_multiple_shared_borrows() {
    let graph = build_borrow_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);
    let s_ty = TyGround::path("S");

    let own_s = pcpn
        .get_place(&s_ty, &TypeForm::Value, Capability::Own)
        .unwrap();
    let frz_s = pcpn
        .get_place(&s_ty, &TypeForm::Value, Capability::Frz)
        .unwrap();
    let shr_s = pcpn
        .get_place(&s_ty, &TypeForm::RefShr, Capability::Own)
        .unwrap();

    let mut state = SimState::new();
    let vid = state.fresh_vid();
    state
        .marking
        .add(own_s, Token::new_owned(vid, s_ty.clone()));

    let sim = Simulator::new(&pcpn, SimConfig::default());

    let bsf = pcpn
        .transitions
        .iter()
        .find(|t| t.name == "borrow_shr_first(S)")
        .unwrap();
    let (consume, read) = sim.enabled(bsf, &state).unwrap();
    let (state2, _) = sim.fire(bsf, &state, &consume, &read).unwrap();

    assert_eq!(state2.marking.count(own_s), 0);
    assert_eq!(state2.marking.count(frz_s), 1);
    assert_eq!(state2.marking.count(shr_s), 1);

    let bsn = pcpn
        .transitions
        .iter()
        .find(|t| t.name == "borrow_shr_next(S)")
        .unwrap();
    let (consume2, read2) = sim.enabled(bsn, &state2).unwrap();
    let (state3, _) = sim.fire(bsn, &state2, &consume2, &read2).unwrap();

    assert_eq!(state3.marking.count(shr_s), 2);
}

#[test]
fn test_mut_borrow_exclusivity() {
    let graph = build_borrow_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);
    let s_ty = TyGround::path("S");

    let own_s = pcpn
        .get_place(&s_ty, &TypeForm::Value, Capability::Own)
        .unwrap();
    let mut_s = pcpn
        .get_place(&s_ty, &TypeForm::RefMut, Capability::Own)
        .unwrap();

    let mut state = SimState::new();
    let vid = state.fresh_vid();
    state
        .marking
        .add(own_s, Token::new_owned(vid, s_ty.clone()));

    let sim = Simulator::new(&pcpn, SimConfig::default());

    let bm = pcpn
        .transitions
        .iter()
        .find(|t| t.name == "borrow_mut(S)")
        .unwrap();
    let (consume, read) = sim.enabled(bm, &state).unwrap();
    let (state2, _) = sim.fire(bm, &state, &consume, &read).unwrap();

    assert_eq!(state2.marking.count(mut_s), 1);

    let bsf = pcpn
        .transitions
        .iter()
        .find(|t| t.name == "borrow_shr_first(S)")
        .unwrap();
    assert!(sim.enabled(bsf, &state2).is_none());
}

#[test]
fn test_trace_validity() {
    let configs = vec![
        ("minimal", build_minimal_api_graph(), "own S"),
        ("borrow", build_borrow_api_graph(), "own i32"),
    ];

    for (name, graph, goal_str) in configs {
        let pcpn = Pcpn::from_api_graph(&graph);
        let goal = ParsedGoal::parse(&GoalConfig {
            want: goal_str.to_string(),
            count: 1,
        })
        .unwrap();

        let config = SimConfig {
            max_steps: 80,
            stack_depth: 4,
            goal: Some(goal),
            ..Default::default()
        };

        let sim = Simulator::new(&pcpn, config);
        let result = sim.run();

        assert!(result.found, "{}: should find goal", name);
        assert_trace_valid(&pcpn, &result.trace, name);
    }
}
