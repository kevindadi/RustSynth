use RustSynth::apigraph::*;
use RustSynth::pcpn::{Pcpn, TransitionKind};
use RustSynth::simulator::TraceFiring;
use RustSynth::type_model::{PassingMode, TypeKey};

/// Minimal API: Only one non-Copy type S
/// 极简 API: 仅一个非 Copy 类型 S
#[allow(dead_code)]
pub fn build_minimal_api_graph() -> ApiGraph {
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

/// Borrow semantics API
/// 借用语义 API
pub fn build_borrow_api_graph() -> ApiGraph {
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

/// Extract pure API call names from trace (ConstProducer + ApiCall)
/// 从 trace 中提取纯 API 调用名（ConstProducer + ApiCall）
pub fn collect_api_call_names(trace: &[TraceFiring]) -> Vec<String> {
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

pub fn api_call_names(trace: &[TraceFiring]) -> Vec<String> {
    collect_api_call_names(trace)
}

/// Validate trace legality: Each step's token place_id is valid and consistent with PCPN
/// 验证 trace 的合法性：每步 token 的 place_id 有效，且与 PCPN 一致
pub fn assert_trace_valid(pcpn: &Pcpn, trace: &[TraceFiring], context: &str) {
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

/// Extract all transition names from trace
/// 从 trace 中提取所有 transition 名称
pub fn all_transition_names(trace: &[TraceFiring]) -> Vec<String> {
    trace.iter().map(|f| f.name.clone()).collect()
}
