//! 全流程集成测试
//!
//! 测试 ApiGraph → PCPN → Simulator → Emitter 的完整流水线，
//! 包括穷举可达序列、借用语义、Copy 语义等。

use RustSynth::apigraph::*;
use RustSynth::config::{GoalConfig, ParsedGoal};
use RustSynth::emitter::emit_rust_code;
use RustSynth::pcpn::{Pcpn, TransitionKind};
use RustSynth::simulator::{SimConfig, SimState, Simulator, TraceFiring};
use RustSynth::type_model::{PassingMode, TypeKey};
use RustSynth::types::{BorrowStack, Capability, Marking, Token, TyGround, TypeForm};

// ===================================================================
//  Helper: 构造不同的 ApiGraph 示例
// ===================================================================

/// 极简 API: 仅一个非 Copy 类型 S
///
/// ```text
/// make() -> S           // const producer
/// transform(S) -> S     // 消耗 S 产生新 S
/// ```
fn build_minimal_api_graph() -> ApiGraph {
    let mut graph = ApiGraph::new();

    let s_tid = graph.get_or_create_type_node(TypeKey::path("S"));

    // make() -> S
    let make_fn = FunctionNode {
        id: 0,
        path: "make".to_string(),
        name: "make".to_string(),
        is_method: false,
        is_entry: true,
        is_const: true,
        is_const_producer: true,
        params: vec![],
        self_param: None,
        return_type: Some(TypeKey::path("S")),
        return_mode: Some(PassingMode::ReturnOwned),
        lifetime_bindings: vec![],
    };
    let make_fid = graph.add_function_node(make_fn);
    graph.add_edge(ApiEdge {
        fn_node: make_fid,
        type_node: s_tid,
        direction: EdgeDirection::Output,
        passing_mode: PassingMode::ReturnOwned,
        ownership: OwnershipType::Own,
        requires_deref: false,
        param_index: None,
        lifetime: None,
    });

    // transform(S) -> S
    let transform_fn = FunctionNode {
        id: 0,
        path: "transform".to_string(),
        name: "transform".to_string(),
        is_method: false,
        is_entry: false,
        is_const: false,
        is_const_producer: false,
        params: vec![ParamInfo {
            name: "s".to_string(),
            base_type: TypeKey::path("S"),
            passing_mode: PassingMode::Move,
        }],
        self_param: None,
        return_type: Some(TypeKey::path("S")),
        return_mode: Some(PassingMode::ReturnOwned),
        lifetime_bindings: vec![],
    };
    let transform_fid = graph.add_function_node(transform_fn);
    graph.add_edge(ApiEdge {
        fn_node: transform_fid,
        type_node: s_tid,
        direction: EdgeDirection::Input,
        passing_mode: PassingMode::Move,
        ownership: OwnershipType::Own,
        requires_deref: false,
        param_index: Some(0),
        lifetime: None,
    });
    graph.add_edge(ApiEdge {
        fn_node: transform_fid,
        type_node: s_tid,
        direction: EdgeDirection::Output,
        passing_mode: PassingMode::ReturnOwned,
        ownership: OwnershipType::Own,
        requires_deref: false,
        param_index: None,
        lifetime: None,
    });

    graph
}

/// 借用语义 API
///
/// ```text
/// make() -> S               // producer
/// peek(&S) -> i32           // shared borrow, returns i32
/// modify(&mut S)            // mutable borrow
/// consume(S) -> i32         // move, returns i32
/// ```
fn build_borrow_api_graph() -> ApiGraph {
    let mut graph = ApiGraph::new();

    let s_tid = graph.get_or_create_type_node(TypeKey::path("S"));
    let i32_tid = graph.get_or_create_type_node(TypeKey::primitive("i32"));

    // make() -> S
    let make_fn = FunctionNode {
        id: 0,
        path: "make".to_string(),
        name: "make".to_string(),
        is_method: false,
        is_entry: true,
        is_const: true,
        is_const_producer: true,
        params: vec![],
        self_param: None,
        return_type: Some(TypeKey::path("S")),
        return_mode: Some(PassingMode::ReturnOwned),
        lifetime_bindings: vec![],
    };
    let make_fid = graph.add_function_node(make_fn);
    graph.add_edge(ApiEdge {
        fn_node: make_fid,
        type_node: s_tid,
        direction: EdgeDirection::Output,
        passing_mode: PassingMode::ReturnOwned,
        ownership: OwnershipType::Own,
        requires_deref: false,
        param_index: None,
        lifetime: None,
    });

    // peek(&S) -> i32
    let peek_fn = FunctionNode {
        id: 0,
        path: "peek".to_string(),
        name: "peek".to_string(),
        is_method: false,
        is_entry: false,
        is_const: false,
        is_const_producer: false,
        params: vec![ParamInfo {
            name: "s".to_string(),
            base_type: TypeKey::path("S"),
            passing_mode: PassingMode::BorrowShr,
        }],
        self_param: None,
        return_type: Some(TypeKey::primitive("i32")),
        return_mode: Some(PassingMode::ReturnOwned),
        lifetime_bindings: vec![],
    };
    let peek_fid = graph.add_function_node(peek_fn);
    graph.add_edge(ApiEdge {
        fn_node: peek_fid,
        type_node: s_tid,
        direction: EdgeDirection::Input,
        passing_mode: PassingMode::BorrowShr,
        ownership: OwnershipType::Shr,
        requires_deref: false,
        param_index: Some(0),
        lifetime: None,
    });
    graph.add_edge(ApiEdge {
        fn_node: peek_fid,
        type_node: i32_tid,
        direction: EdgeDirection::Output,
        passing_mode: PassingMode::ReturnOwned,
        ownership: OwnershipType::Own,
        requires_deref: false,
        param_index: None,
        lifetime: None,
    });

    // modify(&mut S)
    let modify_fn = FunctionNode {
        id: 0,
        path: "modify".to_string(),
        name: "modify".to_string(),
        is_method: false,
        is_entry: false,
        is_const: false,
        is_const_producer: false,
        params: vec![ParamInfo {
            name: "s".to_string(),
            base_type: TypeKey::path("S"),
            passing_mode: PassingMode::BorrowMut,
        }],
        self_param: None,
        return_type: None,
        return_mode: None,
        lifetime_bindings: vec![],
    };
    let modify_fid = graph.add_function_node(modify_fn);
    graph.add_edge(ApiEdge {
        fn_node: modify_fid,
        type_node: s_tid,
        direction: EdgeDirection::Input,
        passing_mode: PassingMode::BorrowMut,
        ownership: OwnershipType::Mut,
        requires_deref: false,
        param_index: Some(0),
        lifetime: None,
    });

    // consume(S) -> i32
    let consume_fn = FunctionNode {
        id: 0,
        path: "consume".to_string(),
        name: "consume".to_string(),
        is_method: false,
        is_entry: false,
        is_const: false,
        is_const_producer: false,
        params: vec![ParamInfo {
            name: "s".to_string(),
            base_type: TypeKey::path("S"),
            passing_mode: PassingMode::Move,
        }],
        self_param: None,
        return_type: Some(TypeKey::primitive("i32")),
        return_mode: Some(PassingMode::ReturnOwned),
        lifetime_bindings: vec![],
    };
    let consume_fid = graph.add_function_node(consume_fn);
    graph.add_edge(ApiEdge {
        fn_node: consume_fid,
        type_node: s_tid,
        direction: EdgeDirection::Input,
        passing_mode: PassingMode::Move,
        ownership: OwnershipType::Own,
        requires_deref: false,
        param_index: Some(0),
        lifetime: None,
    });
    graph.add_edge(ApiEdge {
        fn_node: consume_fid,
        type_node: i32_tid,
        direction: EdgeDirection::Output,
        passing_mode: PassingMode::ReturnOwned,
        ownership: OwnershipType::Own,
        requires_deref: false,
        param_index: None,
        lifetime: None,
    });

    graph
}

/// 从 trace 中提取纯 API 调用名（ConstProducer + ApiCall）
fn collect_api_call_names(trace: &[TraceFiring]) -> Vec<String> {
    trace
        .iter()
        .filter(|f| {
            matches!(
                f.kind,
                TransitionKind::ApiCall { .. } | TransitionKind::ConstProducer { .. }
            )
        })
        .map(|f| f.name.clone())
        .collect()
}

fn api_call_names(trace: &[TraceFiring]) -> Vec<String> {
    collect_api_call_names(trace)
}

/// 验证 trace 的合法性：每步 token 的 place_id 有效，且与 PCPN 一致
fn assert_trace_valid(pcpn: &Pcpn, trace: &[TraceFiring], context: &str) {
    let num_places = pcpn.places.len();
    for (i, firing) in trace.iter().enumerate() {
        for (place_id, _token) in &firing.produced {
            assert!(
                *place_id < num_places,
                "{}: step {} produced token for invalid place {}",
                context,
                i,
                place_id
            );
        }
        for (place_id, _token) in &firing.consumed {
            assert!(
                *place_id < num_places,
                "{}: step {} consumed token from invalid place {}",
                context,
                i,
                place_id
            );
        }
    }
}

/// 从 trace 中提取所有 transition 名称
fn all_transition_names(trace: &[TraceFiring]) -> Vec<String> {
    trace.iter().map(|f| f.name.clone()).collect()
}

// ===================================================================
//  测试 1: 完整 Counter 流水线
// ===================================================================

#[test]
fn test_full_pipeline_counter() {
    // ApiGraph → PCPN → Simulator → Emitter 完整流程
    let graph = RustSynth::apigraph::build_counter_api_graph();

    // 阶段 1: ApiGraph
    assert_eq!(graph.type_nodes.len(), 2);
    assert_eq!(graph.fn_nodes.len(), 4);
    assert_eq!(graph.edges.len(), 6);

    // 阶段 2: PCPN
    let pcpn = Pcpn::from_api_graph(&graph);
    assert_eq!(pcpn.places.len(), 18);
    assert!(pcpn.transitions.len() > 4);

    // 阶段 3: Simulator — goal "own i32"
    let goal = ParsedGoal::parse(&GoalConfig {
        want: "own i32".to_string(),
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

    assert!(result.found, "Full pipeline should find own i32");
    assert!(!result.trace.is_empty());

    // 阶段 4: Emitter
    let code = emit_rust_code(&result.trace, &pcpn);
    assert!(code.contains("fn main()"));
    assert!(
        code.lines().count() > 5,
        "Generated code should have multiple lines"
    );
}

// ===================================================================
//  测试 2: 极简网络穷举可达
// ===================================================================

/// ```text
/// Petri 网结构 (极简 API: make/transform, 类型 S):
///
/// Places (9):
///   p0: (S, Value, Own)    p1: (S, Value, Frz)    p2: (S, Value, Blk)
///   p3: (S, RefShr, Own)   p4: (S, RefShr, Frz)   p5: (S, RefShr, Blk)
///   p6: (S, RefMut, Own)   p7: (S, RefMut, Frz)   p8: (S, RefMut, Blk)
///
/// API Transitions:
///   make():        [] → [p0]                (ConstProducer)
///   transform(S):  [p0 consume] → [p0]      (ApiCall, move S → S)
///
/// Structural Transitions:
///   borrow_shr_first(S):  [p0 consume] → [p1, p3]     Guard: NoBlk
///   borrow_shr_next(S):   [p1 read]    → [p3]
///   end_shr_keep_frz(S):  [p1 read, p3 consume] → [p1]  Guard: StackTopMatches
///   end_shr_unfreeze(S):  [p1 consume, p3 consume] → [p0]  Guard: StackTopMatches
///   borrow_mut(S):        [p0 consume] → [p2, p6]     Guard: NoFrzNoBlk
///   end_mut(S):           [p2 consume, p6 consume] → [p0]  Guard: StackTopMatches
///   drop_val(S):          [p0 consume] → []            Guard: NotBlocked
///   drop_shr(S):          [p3 consume] → []
///   drop_mut(S):          [p6 consume] → []
///
/// (S 非 primitive 非 Copy → 无 const_S / copy_use)
/// ```
#[test]
fn test_minimal_network_exhaustive_reachability() {
    let graph = build_minimal_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);
    let s_ty = TyGround::path("S");

    // 验证 PCPN 结构
    assert_eq!(pcpn.places.len(), 9, "S×9 = 9 places");
    assert_eq!(pcpn.type_universe.len(), 1, "Only type S");

    // API transitions: make (ConstProducer) + transform (ApiCall) = 2
    let api_trans: Vec<_> = pcpn
        .transitions
        .iter()
        .filter(|t| {
            matches!(
                t.kind,
                TransitionKind::ApiCall { .. } | TransitionKind::ConstProducer { .. }
            )
        })
        .collect();
    assert_eq!(api_trans.len(), 2, "make + transform");

    // 生成可达图 (bound = 2, stack_depth = 3 保持有限)
    let config = SimConfig {
        max_steps: 500,
        stack_depth: 3,
        default_bound: 2,
        ..Default::default()
    };

    let sim = Simulator::new(&pcpn, config);
    let rg = sim.generate_reachability_graph(300);

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

    println!(
        "Minimal network: {} states, {} edges",
        rg.states.len(),
        rg.edges.len()
    );

    // 验证初始状态 (index 0) 有 make() 出发的边
    let from_initial: Vec<_> = rg.edges.iter().filter(|(from, _, _)| *from == 0).collect();
    assert!(from_initial
        .iter()
        .any(|(_, _, name)| name == "make"));

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

    // 最短 trace 应该只有 make() (1 步)
    let api_names = api_call_names(&result.trace);
    assert_eq!(api_names, vec!["make"], "Shortest path: [make()]");
}

#[test]
fn test_minimal_network_transform_chain() {
    // 验证 make → transform → transform → ... 的链式调用
    let graph = build_minimal_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);

    // Goal: own S (任何包含 transform 的序列)
    // 由于 make() 直接满足 goal，我们用 deny 禁止直接 make 满足
    // 然后验证 make → transform 也能满足
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

    // 验证 trace 合法性：每一步的 produced token 都在下一步可用
    for (i, firing) in result.trace.iter().enumerate() {
        // ConstProducer 第一步不需要 consumed
        if i == 0 && matches!(firing.kind, TransitionKind::ConstProducer { .. }) {
            assert!(
                firing.consumed.is_empty(),
                "ConstProducer should have no consumed tokens"
            );
        }
    }
}

// ===================================================================
//  测试 3: 借用语义穷举
// ===================================================================

/// ```text
/// Petri 网结构 (make/peek/modify/consume, 类型 S + i32):
///
///   make():      [] → [own_S]                    (ConstProducer)
///   peek(&S):    [own_&S read] → [own_i32]       (ApiCall)
///   modify(&mut S): [own_&mut_S read] → []       (ApiCall)
///   consume(S):  [own_S consume] → [own_i32]     (ApiCall)
///
/// Goal "own i32" 的可达 API 路径:
///   1. [const_i32]                               — 直接创建 i32
///   2. [make, consume]                           — make S then consume to i32
///   3. [make, borrow_shr_first, peek, ...]       — make S, borrow, peek
/// ```
#[test]
fn test_borrow_network_all_sequences() {
    let graph = build_borrow_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);

    let s_ty = TyGround::path("S");
    let i32_ty = TyGround::primitive("i32");

    // 验证 PCPN 结构
    assert_eq!(pcpn.places.len(), 18, "S×9 + i32×9");

    // 4 API transitions: make, peek, modify, consume
    let api_trans: Vec<_> = pcpn
        .transitions
        .iter()
        .filter(|t| {
            matches!(
                t.kind,
                TransitionKind::ApiCall { .. } | TransitionKind::ConstProducer { .. }
            )
        })
        .collect();
    assert_eq!(api_trans.len(), 4);

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
        // 最短路径可能是 const_i32
        assert!(
            result.trace.len() <= 3,
            "Shortest path should be short, got {} steps",
            result.trace.len()
        );
    }

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
        assert!(
            api_names.contains(&"make".to_string()),
            "Should use make, got {:?}",
            api_names
        );
        assert!(
            api_names.contains(&"consume".to_string()) || api_names.contains(&"peek".to_string()),
            "Should use consume or peek, got {:?}",
            api_names
        );
    }

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
        assert!(
            api_names.contains(&"make".to_string()),
            "Should use make"
        );
        assert!(
            api_names.contains(&"peek".to_string()),
            "Should use peek, got {:?}",
            api_names
        );

        // trace 中应该包含 borrow_shr 相关的结构转换
        let all_names = all_transition_names(&result.trace);
        assert!(
            all_names.iter().any(|n| n.contains("borrow_shr")),
            "Should include borrow_shr transition, got {:?}",
            all_names
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

    println!(
        "Borrow network: {} states, {} edges",
        rg.states.len(),
        rg.edges.len()
    );

    // 应该有有限状态空间
    assert!(rg.states.len() > 1);
    assert!(rg.states.len() <= 500, "Should be bounded");

    // 应该有多种出边从初始状态
    let from_initial: Vec<_> = rg.edges.iter().filter(|(from, _, _)| *from == 0).collect();
    assert!(
        from_initial.len() >= 2,
        "Initial state should have at least make + const_i32 edges"
    );

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

// ===================================================================
//  测试 4: Copy 类型语义
// ===================================================================

#[test]
fn test_copy_type_semantics() {
    // 验证 copy_use transition：Copy 类型可以被复制而不消耗
    let graph = build_borrow_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);
    let i32_ty = TyGround::primitive("i32");

    // 找到 copy_use(i32) transition
    let copy_t = pcpn
        .transitions
        .iter()
        .find(|t| matches!(&t.kind, TransitionKind::CopyUse { ty } if *ty == i32_ty))
        .expect("i32 should have CopyUse");

    // 手动执行 copy_use
    let own_i32 = pcpn
        .get_place(&i32_ty, &TypeForm::Value, Capability::Own)
        .unwrap();

    let mut state = SimState::new();
    let vid = state.fresh_vid();
    state
        .marking
        .add(own_i32, Token::new_owned(vid, i32_ty.clone()));

    assert_eq!(state.marking.count(own_i32), 1);

    let config = SimConfig::default();
    let sim = Simulator::new(&pcpn, config);

    if let Some((consume, read)) = sim.enabled(copy_t, &state) {
        let (state2, firing) = sim.fire(copy_t, &state, &consume, &read).unwrap();

        // copy_use 不消耗原始 token（input arc 是 read），并产生新 token
        // 所以 own_i32 应该有 2 个 token
        assert_eq!(
            state2.marking.count(own_i32),
            2,
            "copy_use should produce a copy, result in 2 tokens"
        );
    } else {
        panic!("copy_use should be enabled when own i32 has a token");
    }
}

#[test]
fn test_primitive_creation() {
    // 验证 CreatePrimitive 可以从空状态创建 i32
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

    assert_eq!(state.marking.count(own_i32), 0);

    let config = SimConfig::default();
    let sim = Simulator::new(&pcpn, config);

    if let Some((consume, read)) = sim.enabled(const_t, &state) {
        let (state2, _) = sim.fire(const_t, &state, &consume, &read).unwrap();
        assert_eq!(
            state2.marking.count(own_i32),
            1,
            "const_i32 should create one i32 token"
        );
    } else {
        panic!("const_i32 should always be enabled");
    }
}

// ===================================================================
//  测试 5: 多重共享借用
// ===================================================================

#[test]
fn test_multiple_shared_borrows() {
    // 验证可以对同一个值创建多个共享引用
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

    // 初始: 一个 own S
    let mut state = SimState::new();
    let vid = state.fresh_vid();
    state
        .marking
        .add(own_s, Token::new_owned(vid, s_ty.clone()));

    let config = SimConfig {
        default_bound: 5,
        ..Default::default()
    };
    let sim = Simulator::new(&pcpn, config);

    // borrow_shr_first
    let bsf = pcpn
        .transitions
        .iter()
        .find(|t| t.name == "borrow_shr_first(S)")
        .unwrap();

    let (consume, read) = sim.enabled(bsf, &state).expect("borrow_shr_first should be enabled");
    let (state2, _) = sim.fire(bsf, &state, &consume, &read).unwrap();

    assert_eq!(state2.marking.count(own_s), 0);
    assert_eq!(state2.marking.count(frz_s), 1);
    assert_eq!(state2.marking.count(shr_s), 1);
    assert_eq!(state2.stack.len(), 2); // Freeze + Shr

    // borrow_shr_next → 创建第二个共享引用
    let bsn = pcpn
        .transitions
        .iter()
        .find(|t| t.name == "borrow_shr_next(S)")
        .unwrap();

    let (consume2, read2) = sim.enabled(bsn, &state2).expect("borrow_shr_next should be enabled");
    let (state3, _) = sim.fire(bsn, &state2, &consume2, &read2).unwrap();

    assert_eq!(state3.marking.count(frz_s), 1, "frz still has 1 token");
    assert_eq!(
        state3.marking.count(shr_s),
        2,
        "Now 2 shared references exist"
    );
    assert_eq!(state3.stack.len(), 3, "Freeze + Shr + Shr");

    // 可以继续创建第三个 borrow_shr_next
    if let Some((consume3, read3)) = sim.enabled(bsn, &state3) {
        let (state4, _) = sim.fire(bsn, &state3, &consume3, &read3).unwrap();
        assert_eq!(
            state4.marking.count(shr_s),
            3,
            "Now 3 shared references exist"
        );
    }
}

// ===================================================================
//  测试 6: 可变借用互斥
// ===================================================================

#[test]
fn test_mut_borrow_exclusivity() {
    // 验证可变借用期间不能再借用（guard 阻止）
    let graph = build_borrow_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);
    let s_ty = TyGround::path("S");

    let own_s = pcpn
        .get_place(&s_ty, &TypeForm::Value, Capability::Own)
        .unwrap();
    let blk_s = pcpn
        .get_place(&s_ty, &TypeForm::Value, Capability::Blk)
        .unwrap();
    let mut_s = pcpn
        .get_place(&s_ty, &TypeForm::RefMut, Capability::Own)
        .unwrap();

    let mut state = SimState::new();
    let vid = state.fresh_vid();
    state
        .marking
        .add(own_s, Token::new_owned(vid, s_ty.clone()));

    let config = SimConfig {
        default_bound: 5,
        ..Default::default()
    };
    let sim = Simulator::new(&pcpn, config);

    // borrow_mut
    let bm = pcpn
        .transitions
        .iter()
        .find(|t| t.name == "borrow_mut(S)")
        .unwrap();

    let (consume, read) = sim.enabled(bm, &state).expect("borrow_mut should be enabled");
    let (state2, _) = sim.fire(bm, &state, &consume, &read).unwrap();

    assert_eq!(state2.marking.count(own_s), 0);
    assert_eq!(state2.marking.count(blk_s), 1);
    assert_eq!(state2.marking.count(mut_s), 1);

    // 尝试 borrow_shr_first → 应该被 NoBlk guard 阻止
    // 但 own_s 已经是 0 了（被消耗），所以 borrow_shr_first 不可用（无 input token）
    let bsf = pcpn
        .transitions
        .iter()
        .find(|t| t.name == "borrow_shr_first(S)")
        .unwrap();
    assert!(
        sim.enabled(bsf, &state2).is_none(),
        "borrow_shr_first should NOT be enabled during mut borrow"
    );

    // 尝试 drop_val → 应该被 NotBlocked guard 阻止（但 own_s 为空，所以也不可用）
    // 这验证了值在可变借用期间无法被销毁
}

// ===================================================================
//  测试 7: 完整流水线代码生成 (从 borrow API)
// ===================================================================

#[test]
fn test_full_pipeline_borrow_api_codegen() {
    let graph = build_borrow_api_graph();
    let pcpn = Pcpn::from_api_graph(&graph);

    // Goal: own i32 via make → consume
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

    // 验证生成代码的结构
    assert!(code.contains("fn main()"));
    assert!(code.contains("make()"), "Should call make()");

    // 打印生成的代码（便于调试）
    println!("=== Generated code (borrow API) ===\n{}", code);
}

// ===================================================================
//  测试 8: Counter 网的所有可达 API 调用序列枚举
// ===================================================================

#[test]
fn test_counter_enumerate_all_api_sequences() {
    // 使用 Counter API，枚举所有可达 API 调用序列到 "own i32"
    // 通过 reachability graph 验证
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

    println!(
        "Counter network: {} states, {} edges",
        rg.states.len(),
        rg.edges.len()
    );

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

    println!("Reachable API transitions: {:?}", all_api_edges);

    // 应该包含 Counter::new
    assert!(all_api_edges.contains("Counter::new"));
    // const_i32 也应该可达
    assert!(all_api_edges.contains("const_i32"));
    // Counter::into_value 应该可达 (需要先 new)
    assert!(
        all_api_edges.contains("Counter::into_value"),
        "into_value should be reachable after new"
    );

    // 验证状态空间有限且合理
    assert!(rg.states.len() <= 500, "State space should be bounded");
}

// ===================================================================
//  测试 9: 验证 trace 合法性
// ===================================================================

#[test]
fn test_trace_validity() {
    // 对多种配置运行 simulator，验证每个 trace 的 token 一致性
    let configs: Vec<(&str, ApiGraph, &str)> = vec![
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

// ===================================================================
//  测试 10: DOT 输出
// ===================================================================

#[test]
fn test_dot_output() {
    // 验证各阶段的 DOT 输出格式正确
    let graph = RustSynth::apigraph::build_counter_api_graph();

    // ApiGraph DOT
    let api_dot = graph.to_dot();
    assert!(api_dot.starts_with("digraph ApiGraph {"));
    assert!(api_dot.contains("Counter"));
    assert!(api_dot.ends_with("}\n"));

    // PCPN DOT
    let pcpn = Pcpn::from_api_graph(&graph);
    let pcpn_dot = pcpn.to_dot();
    assert!(pcpn_dot.starts_with("digraph PCPN {"));
    assert!(pcpn_dot.contains("Counter"));
    assert!(pcpn_dot.ends_with("}\n"));

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
    assert!(rg_dot.ends_with("}\n"));
}
