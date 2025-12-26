//! Petri 网构建器
//!
//! 从 IR-Graph 构建下推着色 Petri 网

use std::collections::HashMap;

use super::types::{TypeId, RustType, PrimitiveKind, Constructibility, ConstructMethod};
use super::place::{PlaceId, PlaceKind};
use super::transition::{
    TransitionId, Transition, StructuralKind,
    SignatureInfo, ParamInfo, ParamPassing, SelfKind, AutoConstructMethod,
};
use super::arc::ArcKind;
use super::net::PcpnNet;

use crate::ir_graph::{IrGraph, NodeType, NodeInfo, EdgeMode};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;

/// PCPN 构建器
pub struct PcpnBuilder {
    net: PcpnNet,
    /// IrGraph NodeIndex -> PlaceId 映射
    node_to_place: HashMap<NodeIndex, PlaceId>,
    /// IrGraph NodeIndex -> TransitionId 映射
    node_to_transition: HashMap<NodeIndex, TransitionId>,
    /// IrGraph NodeIndex -> TypeId 映射
    node_to_type: HashMap<NodeIndex, TypeId>,
}

impl PcpnBuilder {
    pub fn new() -> Self {
        PcpnBuilder {
            net: PcpnNet::new(),
            node_to_place: HashMap::new(),
            node_to_transition: HashMap::new(),
            node_to_type: HashMap::new(),
        }
    }

    /// 从 IrGraph 构建 PCPN
    pub fn build_from_ir_graph(mut self, ir: &IrGraph) -> PcpnNet {
        // 阶段 1: 注册所有类型
        self.register_types(ir);

        // 阶段 2: 创建库所（数据类型节点）
        self.create_places(ir);

        // 阶段 3: 创建签名诱导变迁（操作节点）
        self.create_signature_transitions(ir);

        // 阶段 4: 创建结构变迁
        self.create_structural_transitions(ir);

        // 阶段 5: 创建自动构造变迁
        self.create_auto_construct_transitions();

        // 阶段 6: 根据边创建弧
        self.create_arcs(ir);

        // 阶段 7: 设置初始标记
        self.set_initial_marking(ir);

        self.net
    }

    /// 注册类型
    fn register_types(&mut self, ir: &IrGraph) {
        for node_idx in ir.type_graph.node_indices() {
            if let Some(node_type) = ir.node_types.get(&node_idx) {
                if is_data_node(node_type) {
                    let rust_type = self.infer_rust_type(ir, node_idx);
                    let type_id = self.net.types.register(rust_type);
                    self.node_to_type.insert(node_idx, type_id);

                    // 设置可构造性
                    self.set_constructibility(ir, node_idx, type_id);
                }
            }
        }
    }

    /// 推断 Rust 类型
    fn infer_rust_type(&self, ir: &IrGraph, node_idx: NodeIndex) -> RustType {
        let node_label = &ir.type_graph[node_idx];

        if let Some(node_info) = ir.node_infos.get(&node_idx) {
            match node_info {
                NodeInfo::Primitive(info) => {
                    if let Some(kind) = PrimitiveKind::from_str(&info.name) {
                        return RustType::Primitive(kind);
                    }
                }
                NodeInfo::Struct(info) => {
                    return RustType::Named {
                        path: info.path.full_path.clone(),
                        type_args: Vec::new(), // TODO: 处理泛型
                    };
                }
                NodeInfo::Enum(info) => {
                    return RustType::Named {
                        path: info.path.full_path.clone(),
                        type_args: Vec::new(),
                    };
                }
                NodeInfo::Generic(info) => {
                    return RustType::Generic {
                        name: info.name.clone(),
                        scope: "unknown".to_string(),
                    };
                }
                NodeInfo::Tuple(info) => {
                    // 简化处理
                    return RustType::Tuple(Vec::new());
                }
                NodeInfo::Slice(info) => {
                    return RustType::Slice(Box::new(RustType::Primitive(PrimitiveKind::U8)));
                }
                _ => {}
            }
        }

        // 回退：从标签推断
        if let Some(kind) = PrimitiveKind::from_str(node_label) {
            RustType::Primitive(kind)
        } else {
            RustType::Named {
                path: node_label.clone(),
                type_args: Vec::new(),
            }
        }
    }

    /// 设置类型的可构造性
    fn set_constructibility(&mut self, ir: &IrGraph, node_idx: NodeIndex, type_id: TypeId) {
        if let Some(node_info) = ir.node_infos.get(&node_idx) {
            let constructibility = match node_info {
                NodeInfo::Primitive(_) => Constructibility::Unlimited,
                NodeInfo::Struct(info) => {
                    // 检查是否实现 Copy/Clone/Default
                    if info.trait_impls.iter().any(|t| t.trait_name == "Copy") {
                        Constructibility::Limited {
                            budget: 10,
                            method: ConstructMethod::Copy,
                        }
                    } else if info.trait_impls.iter().any(|t| t.trait_name == "Clone") {
                        Constructibility::Limited {
                            budget: 5,
                            method: ConstructMethod::Clone,
                        }
                    } else if info.trait_impls.iter().any(|t| t.trait_name == "Default") {
                        Constructibility::Limited {
                            budget: 3,
                            method: ConstructMethod::Default,
                        }
                    } else {
                        // 检查是否有 const fn 构造函数
                        let has_const_new = info.methods.iter().any(|&m| {
                            if let Some(NodeInfo::Method(method_info)) = ir.node_infos.get(&m) {
                                method_info.is_const && method_info.name == "new"
                            } else {
                                false
                            }
                        });
                        if has_const_new {
                            Constructibility::ConstFn {
                                fn_path: format!("{}::new", info.path.full_path),
                                params: Vec::new(),
                            }
                        } else {
                            Constructibility::NotConstructible
                        }
                    }
                }
                NodeInfo::Enum(info) => {
                    if info.trait_impls.iter().any(|t| t.trait_name == "Default") {
                        Constructibility::Limited {
                            budget: 3,
                            method: ConstructMethod::Default,
                        }
                    } else {
                        Constructibility::NotConstructible
                    }
                }
                _ => Constructibility::NotConstructible,
            };

            self.net.types.set_constructibility(type_id, constructibility);
        }
    }

    /// 创建库所
    fn create_places(&mut self, ir: &IrGraph) {
        for node_idx in ir.type_graph.node_indices() {
            if let Some(node_type) = ir.node_types.get(&node_idx) {
                if is_data_node(node_type) {
                    let node_label = ir.type_graph[node_idx].clone();
                    let type_id = self.node_to_type.get(&node_idx).copied()
                        .unwrap_or(TypeId::new(0));

                    // 创建所有权库所
                    let own_place = self.net.add_place(
                        format!("own_{}", node_label),
                        PlaceKind::Own,
                        type_id,
                    );
                    self.node_to_place.insert(node_idx, own_place);

                    // 如果是可自动构造的类型，创建自动构造库所
                    if self.net.types.can_auto_construct(type_id) {
                        let auto_place = self.net.add_place(
                            format!("auto_{}", node_label),
                            PlaceKind::AutoConstruct,
                            type_id,
                        );
                        // 设置初始 token（无限供应）
                        self.net.set_initial_token(auto_place, type_id, 1);
                    }
                }
            }
        }
    }

    /// 创建签名诱导变迁
    fn create_signature_transitions(&mut self, ir: &IrGraph) {
        for node_idx in ir.type_graph.node_indices() {
            if let Some(node_type) = ir.node_types.get(&node_idx) {
                if is_operation_node(node_type) {
                    if let Some(node_info) = ir.node_infos.get(&node_idx) {
                        let transition = self.create_signature_transition(ir, node_idx, node_info);
                        if let Some(trans) = transition {
                            let trans_id = self.net.add_transition(trans);
                            self.node_to_transition.insert(node_idx, trans_id);
                        }
                    }
                }
            }
        }
    }

    /// 创建单个签名诱导变迁
    fn create_signature_transition(
        &mut self,
        ir: &IrGraph,
        node_idx: NodeIndex,
        node_info: &NodeInfo,
    ) -> Option<Transition> {
        match node_info {
            NodeInfo::Method(method_info) => {
                let trans_id = self.net.next_transition_id();

                let params: Vec<ParamInfo> = method_info.params.iter()
                    .filter(|p| !p.is_self)
                    .map(|p| {
                        let type_id = p.type_node
                            .and_then(|n| self.node_to_type.get(&n).copied())
                            .unwrap_or(TypeId::new(0));
                        let passing = match p.borrow_mode {
                            EdgeMode::Move => ParamPassing::ByValue,
                            EdgeMode::Ref => ParamPassing::ByRef,
                            EdgeMode::MutRef => ParamPassing::ByMutRef,
                            _ => ParamPassing::ByValue,
                        };
                        ParamInfo {
                            name: p.name.clone(),
                            type_id,
                            passing,
                        }
                    })
                    .collect();

                let self_param = method_info.params.iter()
                    .find(|p| p.is_self)
                    .map(|p| match p.borrow_mode {
                        EdgeMode::Move => SelfKind::Owned,
                        EdgeMode::Ref => SelfKind::Ref,
                        EdgeMode::MutRef => SelfKind::MutRef,
                        _ => SelfKind::Ref,
                    });

                let return_type = method_info.return_info.type_node
                    .and_then(|n| self.node_to_type.get(&n).copied());

                let sig_info = SignatureInfo {
                    path: method_info.owner
                        .and_then(|o| ir.node_infos.get(&o))
                        .and_then(|info| info.path())
                        .map(|p| p.full_path.clone())
                        .unwrap_or_default(),
                    name: method_info.name.clone(),
                    params,
                    return_type,
                    is_const: method_info.is_const,
                    is_async: method_info.is_async,
                    is_unsafe: method_info.is_unsafe,
                    is_method: self_param.is_some(),
                    self_param,
                };

                Some(Transition::signature(trans_id, sig_info))
            }
            NodeInfo::Function(func_info) => {
                let trans_id = self.net.next_transition_id();

                let params: Vec<ParamInfo> = func_info.params.iter()
                    .map(|p| {
                        let type_id = p.type_node
                            .and_then(|n| self.node_to_type.get(&n).copied())
                            .unwrap_or(TypeId::new(0));
                        let passing = match p.borrow_mode {
                            EdgeMode::Move => ParamPassing::ByValue,
                            EdgeMode::Ref => ParamPassing::ByRef,
                            EdgeMode::MutRef => ParamPassing::ByMutRef,
                            _ => ParamPassing::ByValue,
                        };
                        ParamInfo {
                            name: p.name.clone(),
                            type_id,
                            passing,
                        }
                    })
                    .collect();

                let return_type = func_info.return_info.type_node
                    .and_then(|n| self.node_to_type.get(&n).copied());

                let sig_info = SignatureInfo {
                    path: func_info.path.full_path.clone(),
                    name: func_info.path.name.clone(),
                    params,
                    return_type,
                    is_const: func_info.is_const,
                    is_async: func_info.is_async,
                    is_unsafe: func_info.is_unsafe,
                    is_method: false,
                    self_param: None,
                };

                Some(Transition::signature(trans_id, sig_info))
            }
            _ => None,
        }
    }

    /// 创建结构变迁
    fn create_structural_transitions(&mut self, _ir: &IrGraph) {
        // 先收集所有类型及其 Copy 属性
        let type_infos: Vec<(TypeId, bool)> = self.node_to_type
            .values()
            .map(|&type_id| (type_id, self.net.types.is_copy(type_id)))
            .collect();

        // 为每个类型创建必要的结构变迁
        for (type_id, is_copy) in type_infos {
            // Move 变迁
            self.net.add_structural_transition(StructuralKind::Move, type_id);

            // DropOwn 变迁
            self.net.add_structural_transition(StructuralKind::DropOwn, type_id);

            // 借用变迁
            self.net.add_structural_transition(StructuralKind::BorrowShrOwn, type_id);
            self.net.add_structural_transition(StructuralKind::BorrowMut, type_id);
            self.net.add_structural_transition(StructuralKind::EndMut, type_id);
            self.net.add_structural_transition(StructuralKind::EndShrLast, type_id);

            // Copy 类型的变迁
            if is_copy {
                self.net.add_structural_transition(StructuralKind::CopyUse, type_id);
                self.net.add_structural_transition(StructuralKind::DupCopy, type_id);
            }
        }
    }

    /// 创建自动构造变迁
    fn create_auto_construct_transitions(&mut self) {
        let constructible_types: Vec<_> = self.node_to_type.values()
            .filter(|&&type_id| self.net.types.can_auto_construct(type_id))
            .copied()
            .collect();

        for type_id in constructible_types {
            let method = match self.net.types.get_constructibility(type_id) {
                Constructibility::Unlimited => AutoConstructMethod::Literal,
                Constructibility::Limited { method, .. } => match method {
                    ConstructMethod::Copy => AutoConstructMethod::Copy,
                    ConstructMethod::Clone => AutoConstructMethod::Clone,
                    ConstructMethod::Default => AutoConstructMethod::Default,
                    ConstructMethod::Literal => AutoConstructMethod::Literal,
                },
                Constructibility::ConstFn { fn_path, .. } => {
                    AutoConstructMethod::ConstFn { path: fn_path.clone() }
                }
                Constructibility::NotConstructible => continue,
            };

            let trans_id = self.net.next_transition_id();
            let transition = Transition::auto_construct(trans_id, type_id, method);
            self.net.add_transition(transition);
        }
    }

    /// 创建弧
    fn create_arcs(&mut self, ir: &IrGraph) {
        for edge_ref in ir.type_graph.edge_references() {
            let source_idx = edge_ref.source();
            let target_idx = edge_ref.target();
            let relation = edge_ref.weight();

            let source_place = self.node_to_place.get(&source_idx).copied();
            let source_trans = self.node_to_transition.get(&source_idx).copied();
            let target_place = self.node_to_place.get(&target_idx).copied();
            let target_trans = self.node_to_transition.get(&target_idx).copied();

            match (source_place, source_trans, target_place, target_trans) {
                // 库所 -> 变迁: 输入弧
                (Some(place), None, None, Some(trans)) => {
                    let color = self.node_to_type.get(&source_idx).copied();
                    let arc_kind = match relation.mode {
                        EdgeMode::Move => ArcKind::Normal,
                        EdgeMode::Ref => ArcKind::Read,
                        EdgeMode::MutRef => ArcKind::Normal,
                        _ => ArcKind::Normal,
                    };
                    self.net.add_input_arc(place, trans, arc_kind, 1, color);
                }
                // 变迁 -> 库所: 输出弧
                (None, Some(trans), Some(place), None) => {
                    let color = self.node_to_type.get(&target_idx).copied();
                    self.net.add_output_arc(trans, place, ArcKind::Normal, 1, color);
                }
                _ => {
                    // 其他情况暂时忽略
                }
            }
        }
    }

    /// 设置初始标记
    fn set_initial_marking(&mut self, ir: &IrGraph) {
        for (&node_idx, &place_id) in &self.node_to_place {
            if let Some(node_info) = ir.node_infos.get(&node_idx) {
                let tokens = match node_info {
                    NodeInfo::Constant(info) => {
                        info.init_value.as_ref()
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(1)
                    }
                    NodeInfo::Static(info) => {
                        info.init_value.as_ref()
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(1)
                    }
                    _ => 0,
                };

                if tokens > 0 {
                    if let Some(&type_id) = self.node_to_type.get(&node_idx) {
                        self.net.set_initial_token(place_id, type_id, tokens);
                    }
                }
            }
        }
    }
}

impl Default for PcpnBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// 判断节点是否是数据节点（映射为库所）
fn is_data_node(node_type: &NodeType) -> bool {
    matches!(
        node_type,
        NodeType::Struct
            | NodeType::Enum
            | NodeType::Union
            | NodeType::Primitive
            | NodeType::Tuple
            | NodeType::Generic
            | NodeType::TypeAlias
            | NodeType::Constant
            | NodeType::Static
            | NodeType::Variant
            | NodeType::Trait
            | NodeType::ResultWrapper
            | NodeType::OptionWrapper
    )
}

/// 判断节点是否是操作节点（映射为变迁）
fn is_operation_node(node_type: &NodeType) -> bool {
    matches!(
        node_type,
        NodeType::ImplMethod
            | NodeType::TraitMethod
            | NodeType::Function
            | NodeType::UnwrapOp
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_new() {
        let builder = PcpnBuilder::new();
        let net = builder.net;
        assert_eq!(net.places().count(), 0);
    }
}

