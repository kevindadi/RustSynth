use std::{fmt::Write, sync::Arc};

use petgraph::{
    Direction, 
    graph::NodeIndex, 
    stable_graph::StableGraph, 
    visit::EdgeRef,
    algo::{dijkstra, is_cyclic_directed},
};
use rustdoc_types::{Id, Variant};
use serde::{Deserialize, Serialize};

use super::type_repr::{BorrowKind, TypeDescriptor};

/// 类型化 Petri 网中，token 包含类型信息和借用信息
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Token {
    /// 令牌的类型描述符（规范化后的类型，不考虑借用）
    pub descriptor: TypeDescriptor,
    /// 令牌的借用类型（Owned、SharedRef、MutRef 等）
    pub borrow_kind: BorrowKind,
}

impl Token {
    pub fn new(descriptor: TypeDescriptor, borrow_kind: BorrowKind) -> Self {
        Self {
            descriptor,
            borrow_kind,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlaceId(pub(crate) NodeIndex);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransitionId(pub(crate) NodeIndex);

pub type ArcWeight = u32;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Node {
    Place(Place),
    Transition(Transition),
}

/// 泛型参数需要保存所属类型的 PlaceId 和描述符
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct GenericParameter {
    /// 泛型参数名称（如 "T", "E", "W"）
    pub name: Arc<str>,
    /// 该泛型参数需要的 trait 约束
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trait_bounds: Vec<Arc<str>>,
    /// 所属类型的 PlaceId
    pub owner_place_id: PlaceId,
    /// 所属类型的描述符
    pub owner_descriptor: TypeDescriptor,
}

/// Struct、Enum、Union 类型的共同信息
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompositeTypeInfo {
    /// 该类型实现的 trait 列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implemented_traits: Vec<Arc<str>>,
    /// 该类型定义的泛型参数列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generic_parameters: Vec<GenericParameter>,
    /// Enum 的 Variant 列表（仅当类型为 Enum 时使用）
    /// 直接使用 rustdoc-types 中的 Variant 定义
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<Variant>,
}

/// 库所（Place）代表一个类型定义
/// 使用枚举区分不同类型的 Place，每种类型有对应的数据结构
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Place {
    /// 基本类型（Primitive）
    Primitive {
        descriptor: TypeDescriptor,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        implemented_traits: Vec<Arc<str>>,
    },
    /// Struct、Enum、Union 类型
    Composite {
        descriptor: TypeDescriptor,
        kind: CompositeTypeKind,
        info: CompositeTypeInfo,
    },
    /// 泛型参数类型
    Generic {
        descriptor: TypeDescriptor,
        generic_param: GenericParameter,
    },
}

/// Struct、Enum、Union 的类型种类
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CompositeTypeKind {
    Struct,
    Enum,
    Union,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParameterSummary {
    pub name: Option<Arc<str>>,
    pub descriptor: TypeDescriptor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArcKind {
    #[default]
    Normal,
    Inhibitor,
    Reset,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionContext {
    FreeFunction,
    InherentMethod {
        receiver: TypeDescriptor,
    },
    TraitImplementation {
        receiver: TypeDescriptor,
        trait_path: Arc<str>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionSummary {
    pub item_id: Id,
    pub name: Arc<str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualified_path: Option<Arc<str>>,
    pub signature: Arc<str>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generics: Vec<Arc<str>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub where_clauses: Vec<Arc<str>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trait_bounds: Vec<Arc<str>>,
    pub context: FunctionContext,
    pub inputs: Vec<ParameterSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<TypeDescriptor>,
}

impl FunctionSummary {
    /// 返回函数的显示名称（优先使用 qualified_path，否则使用 name）
    pub fn display_name(&self) -> &str {
        self.qualified_path.as_deref().unwrap_or(self.name.as_ref())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArcData {
    #[serde(default = "default_weight")]
    pub weight: ArcWeight,
    #[serde(default)]
    pub kind: ArcKind,
    // 对于输入弧（Place -> Transition），可能包含参数信息
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter: Option<ParameterSummary>,
    // 对于输出弧（Transition -> Place），存储类型描述符
    #[serde(skip_serializing_if = "Option::is_none")]
    pub descriptor: Option<TypeDescriptor>,
    // 边上需要的借用类型约束（对于输入弧）或提供的借用类型（对于输出弧）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub borrow_kind: Option<super::type_repr::BorrowKind>,
}

fn default_weight() -> ArcWeight {
    1
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transition {
    pub summary: FunctionSummary,
}

/// 用于查找 Place 的 key
/// 对于普通类型，只用 TypeDescriptor
/// 对于泛型参数，需要同时指定所有者 ID 和类型名称
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// Place 查找键
/// 只使用类型描述符，因为泛型参数不再单独创建库所
enum PlaceLookupKey {
    /// 类型描述符（规范化后的类型名，去掉泛型参数部分）
    Type(TypeDescriptor),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PetriNet {
    #[serde(with = "petgraph_serde")]
    graph: StableGraph<Node, ArcData>,
    #[serde(skip)]
    place_lookup: indexmap::IndexMap<PlaceLookupKey, PlaceId>,
}

mod petgraph_serde {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(
        graph: &StableGraph<Node, ArcData>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let nodes: Vec<_> = graph
            .node_indices()
            .map(|idx| (idx.index(), graph[idx].clone()))
            .collect();
        let edges: Vec<_> = graph
            .edge_indices()
            .map(|idx| {
                let (source, target) = graph.edge_endpoints(idx).unwrap();
                (source.index(), target.index(), graph[idx].clone())
            })
            .collect();
        (nodes, edges).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<StableGraph<Node, ArcData>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let (nodes, edges): (Vec<(usize, Node)>, Vec<(usize, usize, ArcData)>) =
            Deserialize::deserialize(deserializer)?;

        let mut graph = StableGraph::new();
        let mut index_map = std::collections::HashMap::new();

        for (old_idx, node) in nodes {
            let new_idx = graph.add_node(node);
            index_map.insert(old_idx, new_idx);
        }

        for (source_old, target_old, data) in edges {
            let source = index_map[&source_old];
            let target = index_map[&target_old];
            graph.add_edge(source, target, data);
        }

        Ok(graph)
    }
}

impl Default for PetriNet {
    fn default() -> Self {
        Self {
            graph: StableGraph::new(),
            place_lookup: indexmap::IndexMap::new(),
        }
    }
}

impl Place {
    pub fn descriptor(&self) -> &TypeDescriptor {
        match self {
            Place::Primitive { descriptor, .. } => descriptor,
            Place::Composite { descriptor, .. } => descriptor,
            Place::Generic { descriptor, .. } => descriptor,
        }
    }

    /// 获取实现的 trait 列表
    pub fn implemented_traits(&self) -> &[Arc<str>] {
        match self {
            Place::Primitive { implemented_traits, .. } => implemented_traits,
            Place::Composite { info, .. } => &info.implemented_traits,
            Place::Generic { .. } => &[],
        }
    }

    pub fn generic_parameters(&self) -> &[GenericParameter] {
        match self {
            Place::Primitive { .. } => &[],
            Place::Composite { info, .. } => &info.generic_parameters,
            Place::Generic { generic_param, .. } => std::slice::from_ref(generic_param),
        }
    }
}

impl PetriNet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn to_json(&self) -> super::schema::JsonPetriNet {
        super::schema::JsonPetriNet::from(self)
    }

    pub fn from_json(json: super::schema::JsonPetriNet) -> Result<Self, String> {
        json.to_petri_net()
    }

    pub fn add_place(&mut self, descriptor: TypeDescriptor) -> PlaceId {
        // 默认创建为 Primitive 类型
        self.add_primitive_place(descriptor, Vec::new())
    }

    /// 添加基本类型库所，并记录其实现的 trait
    pub fn add_primitive_place(
        &mut self,
        descriptor: TypeDescriptor,
        implemented_traits: Vec<Arc<str>>,
    ) -> PlaceId {
        let normalized = descriptor.normalized();
        let lookup_key = PlaceLookupKey::Type(normalized.clone());
        
        if let Some(&id) = self.place_lookup.get(&lookup_key) {
            // 如果已存在，更新实现的 trait
            if let Some(place) = self.graph.node_weight_mut(id.0) {
                if let Node::Place(Place::Primitive { implemented_traits: traits, .. }) = place {
                    let mut existing_traits: std::collections::HashSet<_> =
                        traits.iter().cloned().collect();
                    for trait_ in implemented_traits {
                        existing_traits.insert(trait_);
                    }
                    *traits = existing_traits.into_iter().collect();
                    traits.sort();
                }
            }
            return id;
        }

        let mut traits = implemented_traits;
        traits.sort();
        let place = Place::Primitive {
            descriptor: normalized.clone(),
            implemented_traits: traits,
        };
        let node_idx = self.graph.add_node(Node::Place(place));
        let id = PlaceId(node_idx);

        self.place_lookup.insert(lookup_key, id);
        id
    }

    /// 添加 Composite 类型（Struct、Enum、Union）的 Place
    pub fn add_composite_place(
        &mut self,
        descriptor: TypeDescriptor,
        kind: CompositeTypeKind,
        implemented_traits: Vec<Arc<str>>,
    ) -> PlaceId {
        let normalized = descriptor.normalized();
        let lookup_key = PlaceLookupKey::Type(normalized.clone());
        
        if let Some(&id) = self.place_lookup.get(&lookup_key) {
            // 如果已存在，更新实现的 trait
            if let Some(place) = self.graph.node_weight_mut(id.0) {
                if let Node::Place(Place::Composite { info, .. }) = place {
                    let mut existing_traits: std::collections::HashSet<_> =
                        info.implemented_traits.iter().cloned().collect();
                    for trait_ in implemented_traits {
                        existing_traits.insert(trait_);
                    }
                    info.implemented_traits = existing_traits.into_iter().collect();
                    info.implemented_traits.sort();
                }
            }
            return id;
        }

        let mut traits = implemented_traits;
        traits.sort();
        let place = Place::Composite {
            descriptor: normalized.clone(),
            kind,
            info: CompositeTypeInfo {
                implemented_traits: traits,
                generic_parameters: Vec::new(),
                variants: Vec::new(),
            },
        };
        let node_idx = self.graph.add_node(Node::Place(place));
        let id = PlaceId(node_idx);

        self.place_lookup.insert(lookup_key, id);
        id
    }

    /// 为 Enum Place 添加 Variant
    /// Variant 的所有字段映射到 Enum 的 PlaceId
    pub fn add_variant_to_enum(
        &mut self,
        enum_place_id: PlaceId,
        variant: Variant,
    ) {
        if let Some(place) = self.graph.node_weight_mut(enum_place_id.0) {
            if let Node::Place(Place::Composite { kind, info, .. }) = place {
                if matches!(kind, CompositeTypeKind::Enum) {
                    info.variants.push(variant);
                }
            }
        }
    }

    /// 为已有的 Place 注册一个别名，使多个类型描述符指向同一个 Place
    pub fn alias_place(&mut self, descriptor: TypeDescriptor, place_id: PlaceId) {
        let normalized = descriptor.normalized();
        let lookup_key = PlaceLookupKey::Type(normalized);
        self.place_lookup.insert(lookup_key, place_id);
    }

    /// 为类型 Place 添加泛型参数
    /// 
    /// 泛型参数作为类型定义的属性，存储在 Composite Place 的 generic_parameters 字段中
    /// 
    /// # 参数
    /// 
    /// * `place_id` - 类型 Place 的 ID（必须是 Composite 类型）
    /// * `generic_name` - 泛型参数名称（如 "T", "E", "W"）
    /// * `trait_bounds` - trait 约束列表
    pub fn add_generic_parameter_to_place(
        &mut self,
        place_id: PlaceId,
        generic_name: Arc<str>,
        trait_bounds: Vec<Arc<str>>,
    ) {
        if let Some(place) = self.graph.node_weight_mut(place_id.0) {
            if let Node::Place(Place::Composite { info, descriptor, .. }) = place {
                // 检查是否已存在同名泛型参数
                if let Some(existing) = info.generic_parameters.iter_mut()
                    .find(|p| p.name == generic_name) {
                    // 合并 trait bounds
                    let mut existing_bounds: std::collections::HashSet<_> =
                        existing.trait_bounds.iter().cloned().collect();
                    for bound in trait_bounds {
                        existing_bounds.insert(bound);
                    }
                    existing.trait_bounds = existing_bounds.into_iter().collect();
                    existing.trait_bounds.sort();
                } else {
                    // 添加新的泛型参数，包含所属类型的 PlaceId 和描述符
                    let mut bounds = trait_bounds;
                    bounds.sort();
                    info.generic_parameters.push(GenericParameter {
                        name: generic_name,
                        trait_bounds: bounds,
                        owner_place_id: place_id,
                        owner_descriptor: descriptor.clone(),
                    });
                }
            }
        }
    }

    /// 在基本类型和泛型参数之间添加约束变迁
    pub fn add_constraint_transition(
        &mut self,
        from_primitive: PlaceId,
        to_generic: PlaceId,
        constraints: Vec<Arc<str>>,
    ) -> TransitionId {
        // 先获取描述符，避免借用冲突
        let primitive_descriptor = {
            let primitive_place = self
                .place(from_primitive)
                .expect("primitive place should exist");
            primitive_place.descriptor().clone()
        };
        let generic_descriptor = {
            let generic_place = self.place(to_generic).expect("generic place should exist");
            generic_place.descriptor().clone()
        };

        let signature = if constraints.is_empty() {
            format!(
                "{} -> {}",
                primitive_descriptor.display(),
                generic_descriptor.display()
            )
        } else {
            format!(
                "{} -> {} [{}]",
                primitive_descriptor.display(),
                generic_descriptor.display(),
                constraints.join(", ")
            )
        };

        let summary = FunctionSummary {
            item_id: rustdoc_types::Id(0), // 使用特殊的 ID 表示约束变迁
            name: Arc::<str>::from("constraint"),
            qualified_path: None,
            signature: Arc::<str>::from(signature),
            generics: constraints.clone(),
            where_clauses: constraints.clone(),
            trait_bounds: constraints.clone(),
            context: FunctionContext::FreeFunction,
            inputs: vec![ParameterSummary {
                name: None,
                descriptor: primitive_descriptor.clone(),
            }],
            output: Some(generic_descriptor.clone()),
        };

        let transition_id = self.add_transition(summary);

        // 添加从基本类型到变迁的输入弧
        self.add_input_arc_from_place(
            from_primitive,
            transition_id,
            ArcData {
                weight: 1,
                parameter: Some(ParameterSummary {
                    name: None,
                    descriptor: primitive_descriptor.clone(),
                }),
                kind: ArcKind::Normal,
                descriptor: None,
                borrow_kind: Some(primitive_descriptor.borrow_kind()),
            },
        );

        // 添加从变迁到泛型参数的输出弧
        self.add_output_arc_to_place(
            transition_id,
            to_generic,
            ArcData {
                weight: 1,
                parameter: None,
                kind: ArcKind::Normal,
                descriptor: Some(generic_descriptor.clone()),
                borrow_kind: Some(generic_descriptor.borrow_kind()),
            },
        );

        transition_id
    }

    pub fn add_transition(&mut self, summary: FunctionSummary) -> TransitionId {
        let transition = Transition { summary };
        let node_idx = self.graph.add_node(Node::Transition(transition));
        TransitionId(node_idx)
    }

    pub fn add_input_arc_from_place(
        &mut self,
        place: PlaceId,
        transition: TransitionId,
        arc: ArcData,
    ) {
        self.graph.add_edge(place.0, transition.0, arc);
    }

    pub fn add_output_arc_to_place(
        &mut self,
        transition: TransitionId,
        place: PlaceId,
        arc: ArcData,
    ) {
        self.graph.add_edge(transition.0, place.0, arc);
    }

    pub fn places(&self) -> impl Iterator<Item = (PlaceId, &Place)> {
        self.graph.node_indices().filter_map(|idx| {
            if let Node::Place(place) = &self.graph[idx] {
                Some((PlaceId(idx), place))
            } else {
                None
            }
        })
    }

    pub fn transitions(&self) -> impl Iterator<Item = (TransitionId, &Transition)> {
        self.graph.node_indices().filter_map(|idx| {
            if let Node::Transition(transition) = &self.graph[idx] {
                Some((TransitionId(idx), transition))
            } else {
                None
            }
        })
    }

    pub fn place(&self, id: PlaceId) -> Option<&Place> {
        self.graph.node_weight(id.0).and_then(|node| {
            if let Node::Place(place) = node {
                Some(place)
            } else {
                None
            }
        })
    }

    pub fn transition(&self, id: TransitionId) -> Option<&Transition> {
        self.graph.node_weight(id.0).and_then(|node| {
            if let Node::Transition(transition) = node {
                Some(transition)
            } else {
                None
            }
        })
    }

    pub fn place_id(&self, descriptor: &TypeDescriptor) -> Option<PlaceId> {
        let normalized = descriptor.normalized();
        let lookup_key = PlaceLookupKey::Type(normalized);
        self.place_lookup.get(&lookup_key).copied()
    }
    
    pub fn place_id_with_owner(&self, _owner_id: Option<Id>, descriptor: &TypeDescriptor) -> Option<PlaceId> {
        let normalized = descriptor.normalized();
        
        // 泛型参数不再单独创建库所，直接查找类型
        let type_key = PlaceLookupKey::Type(normalized);
        self.place_lookup.get(&type_key).copied()
    }

    pub fn place_count(&self) -> usize {
        self.graph
            .node_indices()
            .filter(|idx| matches!(self.graph[*idx], Node::Place(_)))
            .count()
    }

    pub fn transition_count(&self) -> usize {
        self.graph
            .node_indices()
            .filter(|idx| matches!(self.graph[*idx], Node::Transition(_)))
            .count()
    }

    /// 获取 Transition 的所有输入边（从 Place 到 Transition）
    pub fn transition_inputs(
        &self,
        transition: TransitionId,
    ) -> impl Iterator<Item = (PlaceId, &ArcData)> {
        self.graph
            .edges_directed(transition.0, Direction::Incoming)
            .filter_map(|edge| {
                let source = edge.source();
                if let Node::Place(_) = &self.graph[source] {
                    Some((PlaceId(source), edge.weight()))
                } else {
                    None
                }
            })
    }

    /// 获取 Transition 的所有输出边（从 Transition 到 Place）
    pub fn transition_outputs(
        &self,
        transition: TransitionId,
    ) -> impl Iterator<Item = (PlaceId, &ArcData)> {
        self.graph
            .edges_directed(transition.0, Direction::Outgoing)
            .filter_map(|edge| {
                let target = edge.target();
                if let Node::Place(_) = &self.graph[target] {
                    Some((PlaceId(target), edge.weight()))
                } else {
                    None
                }
            })
    }

    pub fn to_dot(&self) -> String {
        let mut dot = String::new();
        dot.push_str("digraph PetriNet {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  node [fontname=\"Helvetica\"];\n");

        self.write_places(&mut dot);
        self.write_transitions(&mut dot);
        self.write_arcs(&mut dot);

        dot.push_str("}\n");
        dot
    }

    fn write_places(&self, dot: &mut String) {
        let mut primitive_places = Vec::new();
        let mut generic_type_places = Vec::new();
        let mut variant_places = Vec::new();
        let mut other_places = Vec::new();

        for (id, place) in self.places() {
            let type_name = place.descriptor().display();
            
            match place {
                Place::Primitive { .. } => {
                    if matches!(
                        type_name,
                        "i8" | "i16"
                            | "i32"
                            | "i64"
                            | "i128"
                            | "isize"
                            | "u8"
                            | "u16"
                            | "u32"
                            | "u64"
                            | "u128"
                            | "usize"
                            | "f32"
                            | "f64"
                            | "bool"
                            | "char"
                            | "str"
                    ) {
                        primitive_places.push((id, place));
                    } else {
                        other_places.push((id, place));
                    }
                }
                Place::Composite { kind, info, .. } => {
                    if matches!(kind, CompositeTypeKind::Enum) && !info.variants.is_empty() {
                        // Enum 类型如果有 Variant，用特殊样式
                        variant_places.push((id, place));
                    } else if !info.generic_parameters.is_empty() {
                        // 有泛型参数的类型用特殊样式
                        generic_type_places.push((id, place));
                    } else {
                        other_places.push((id, place));
                    }
                }
                Place::Generic { .. } => {
                    other_places.push((id, place));
                }
            }
        }

        for (id, place) in primitive_places {
            let label = simplify_type_name(place.descriptor().display());
            let _ = writeln!(
                dot,
                "  p{} [shape=circle,style=filled,fillcolor=lightblue,label=\"{}\"];",
                id.0.index(),
                label
            );
        }

        for (id, place) in generic_type_places {
            let base_label = simplify_type_name(place.descriptor().display());
            
            // 构建泛型参数列表：T, E: Error, W: Write
            let generic_params: Vec<String> = match place {
                Place::Composite { info, .. } => info.generic_parameters.iter()
                    .map(|param| {
                        if param.trait_bounds.is_empty() {
                            param.name.to_string()
                        } else {
                            format!("{}: {}", param.name, param.trait_bounds.join(" + "))
                        }
                    })
                    .collect(),
                _ => Vec::new(),
            };
            
            let generic_part = if generic_params.is_empty() {
                String::new()
            } else {
                format!("<{}>", generic_params.join(", "))
            };
            
            let full_label = format!("{}{}", base_label, generic_part);
            let _ = writeln!(
                dot,
                "  p{} [shape=circle,style=filled,fillcolor=orange,label=\"{}\"];",
                id.0.index(),
                full_label
            );
        }

        for (id, place) in variant_places {
            let label = simplify_type_name(place.descriptor().display());
            let _ = writeln!(
                dot,
                "  p{} [shape=circle,style=filled,fillcolor=yellow,label=\"{}\"];",
                id.0.index(),
                label
            );
        }

        for (id, place) in other_places {
            let label = simplify_type_name(place.descriptor().display());
            let _ = writeln!(
                dot,
                "  p{} [shape=circle,label=\"{}\"];",
                id.0.index(),
                label
            );
        }
    }

    fn write_transitions(&self, dot: &mut String) {
        for (id, transition) in self.transitions() {
            let summary = &transition.summary;
            
            let sig = summary.signature.as_ref();  
            let simplified_sig = simplify_signature(sig);
            
            let _ = writeln!(
                dot,
                "  t{} [shape=box,style=rounded,label=\"{}\"];",
                id.0.index(),
                simplified_sig
            );
        }
    }

    fn write_arcs(&self, dot: &mut String) {
        for (transition_id, _transition) in self.transitions() {
            // 输入弧：Place -> Transition
            for (place_id, arc_data) in self.transition_inputs(transition_id) {
                self.write_input_arc(dot, place_id, transition_id, arc_data);
            }

            // 输出弧：Transition -> Place
            for (place_id, arc_data) in self.transition_outputs(transition_id) {
                self.write_output_arc(dot, transition_id, place_id, arc_data);
            }
        }
    }

    fn write_input_arc(
        &self,
        dot: &mut String,
        place_id: PlaceId,
        transition_id: TransitionId,
        arc: &ArcData,
    ) {
        // 简化边标签：只显示借用模式
        let label = if let Some(borrow_kind) = arc.borrow_kind {
            let borrow_mark = match borrow_kind {
                BorrowKind::Owned => "",
                BorrowKind::SharedRef => "&",
                BorrowKind::MutRef => "&mut",
                BorrowKind::RawConstPtr => "*const",
                BorrowKind::RawMutPtr => "*mut",
            };
            if borrow_mark.is_empty() {
                None
            } else {
                Some(borrow_mark.to_string())
            }
        } else {
            None
        };

        let attr = edge_attr(arc.kind, label);
        let _ = writeln!(
            dot,
            "  p{} -> t{}{};",
            place_id.0.index(),
            transition_id.0.index(),
            attr
        );
    }

    fn write_output_arc(
        &self,
        dot: &mut String,
        transition_id: TransitionId,
        place_id: PlaceId,
        arc: &ArcData,
    ) {
        let label = if let Some(borrow_kind) = arc.borrow_kind {
            let borrow_mark = match borrow_kind {
                BorrowKind::Owned => "",
                BorrowKind::SharedRef => "&",
                BorrowKind::MutRef => "&mut",
                BorrowKind::RawConstPtr => "*const",
                BorrowKind::RawMutPtr => "*mut",
            };
            if borrow_mark.is_empty() {
                None
            } else {
                Some(borrow_mark.to_string())
            }
        } else {
            None
        };

        let attr = edge_attr(arc.kind, label);
        let _ = writeln!(
            dot,
            "  t{} -> p{}{};",
            transition_id.0.index(),
            place_id.0.index(),
            attr
        );
    }

    /// 检查图中是否存在环路（用于检测类型依赖循环）
    pub fn has_cycles(&self) -> bool {
        is_cyclic_directed(&self.graph)
    }

    /// 计算从一个 Place 到另一个 Place 的最短路径
    /// 返回路径上经过的 transitions 数量，如果不可达则返回 None
    pub fn shortest_path_length(&self, from: PlaceId, to: PlaceId) -> Option<usize> {
        let distances = dijkstra(
            &self.graph,
            from.0,
            Some(to.0),
            |_| 1, // 统一权重为 1
        );
        
        distances.get(&to.0).copied()
    }

    /// 查找从 source Place 可以通过一次转换到达的所有 target Places
    /// 返回 (target_place, transition, arc_data) 的列表
    pub fn reachable_in_one_step(&self, source: PlaceId) -> Vec<(PlaceId, TransitionId, &ArcData)> {
        let mut reachable = Vec::new();
        
        // 找到所有从 source place 出发的边（到 transition）
        for edge_ref in self.graph.edges_directed(source.0, Direction::Outgoing) {
            let transition_node = edge_ref.target();
            
            // 检查这个节点是否是 transition
            if let Node::Transition(_) = &self.graph[transition_node] {
                let transition_id = TransitionId(transition_node);
                
                // 找到这个 transition 的所有输出
                for output_edge in self.graph.edges_directed(transition_node, Direction::Outgoing) {
                    let target_node = output_edge.target();
                    if let Node::Place(_) = &self.graph[target_node] {
                        reachable.push((
                            PlaceId(target_node),
                            transition_id,
                            output_edge.weight(),
                        ));
                    }
                }
            }
        }
        
        reachable
    }


    pub fn statistics(&self) -> PetriNetStatistics {
        let mut stats = PetriNetStatistics {
            place_count: 0,
            transition_count: 0,
            arc_count: self.graph.edge_count(),
            has_cycles: self.has_cycles(),
            max_place_in_degree: 0,
            max_place_out_degree: 0,
            max_transition_in_degree: 0,
            max_transition_out_degree: 0,
        };

        for node_idx in self.graph.node_indices() {
            match &self.graph[node_idx] {
                Node::Place(_) => {
                    stats.place_count += 1;
                    let in_deg = self.graph.edges_directed(node_idx, Direction::Incoming).count();
                    let out_deg = self.graph.edges_directed(node_idx, Direction::Outgoing).count();
                    stats.max_place_in_degree = stats.max_place_in_degree.max(in_deg);
                    stats.max_place_out_degree = stats.max_place_out_degree.max(out_deg);
                }
                Node::Transition(_) => {
                    stats.transition_count += 1;
                    let in_deg = self.graph.edges_directed(node_idx, Direction::Incoming).count();
                    let out_deg = self.graph.edges_directed(node_idx, Direction::Outgoing).count();
                    stats.max_transition_in_degree = stats.max_transition_in_degree.max(in_deg);
                    stats.max_transition_out_degree = stats.max_transition_out_degree.max(out_deg);
                }
            }
        }

        stats
    }

    pub fn graph(&self) -> &StableGraph<Node, ArcData> {
        &self.graph
    }

    /// 查找类型转换链：从 source 类型到 target 类型的所有可能路径
    /// 返回路径列表，每条路径是一系列 transition IDs
    pub fn find_type_conversion_paths(
        &self,
        source: PlaceId,
        target: PlaceId,
        max_depth: usize,
    ) -> Vec<Vec<TransitionId>> {
        let mut paths = Vec::new();
        let mut current_path = Vec::new();
        let mut visited = std::collections::HashSet::new();
        
        self.dfs_find_paths(source, target, &mut current_path, &mut visited, &mut paths, max_depth);
        
        paths
    }

    /// 深度优先搜索辅助函数
    fn dfs_find_paths(
        &self,
        current: PlaceId,
        target: PlaceId,
        current_path: &mut Vec<TransitionId>,
        visited: &mut std::collections::HashSet<PlaceId>,
        paths: &mut Vec<Vec<TransitionId>>,
        max_depth: usize,
    ) {
        if current == target {
            paths.push(current_path.clone());
            return;
        }

        if current_path.len() >= max_depth {
            return;
        }

        visited.insert(current);

        for (next_place, transition_id, _arc) in self.reachable_in_one_step(current) {
            if !visited.contains(&next_place) {
                current_path.push(transition_id);
                self.dfs_find_paths(next_place, target, current_path, visited, paths, max_depth);
                current_path.pop();
            }
        }

        visited.remove(&current);
    }
}

/// Petri 网的统计信息
#[derive(Debug, Clone)]
pub struct PetriNetStatistics {
    pub place_count: usize,
    pub transition_count: usize,
    pub arc_count: usize,
    pub has_cycles: bool,
    pub max_place_in_degree: usize,
    pub max_place_out_degree: usize,
    pub max_transition_in_degree: usize,
    pub max_transition_out_degree: usize,
}

impl std::fmt::Display for PetriNetStatistics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Petri 网统计信息:")?;
        writeln!(f, "  Places: {}", self.place_count)?;
        writeln!(f, "  Transitions: {}", self.transition_count)?;
        writeln!(f, "  Arcs: {}", self.arc_count)?;
        writeln!(f, "  Has Cycles: {}", self.has_cycles)?;
        writeln!(f, "  Max Place In-Degree: {}", self.max_place_in_degree)?;
        writeln!(f, "  Max Place Out-Degree: {}", self.max_place_out_degree)?;
        writeln!(f, "  Max Transition In-Degree: {}", self.max_transition_in_degree)?;
        writeln!(f, "  Max Transition Out-Degree: {}", self.max_transition_out_degree)?;
        Ok(())
    }
}

fn simplify_type_name(type_name: &str) -> String {
    let simplified = if let Some(last_colon) = type_name.rfind("::") {
        &type_name[last_colon + 2..]
    } else {
        type_name
    };
    
    // 移除生命周期参数
    let mut without_lifetimes = remove_lifetimes(simplified);
    
    // 清理孤立的括号和多余的尖括号
    // 例如: "Error)>" -> "Error", "Error>" -> "Error"
    while without_lifetimes.contains(")>") && !without_lifetimes.contains("(") {
        without_lifetimes = without_lifetimes.replace(")>", ">");
    }
    
    // 清理结尾的孤立 >
    if without_lifetimes.ends_with('>') && without_lifetimes.matches('<').count() < without_lifetimes.matches('>').count() {
        // 移除多余的 >
        let open_count = without_lifetimes.matches('<').count();
        let close_count = without_lifetimes.matches('>').count();
        for _ in 0..(close_count - open_count) {
            if let Some(pos) = without_lifetimes.rfind('>') {
                without_lifetimes.remove(pos);
            }
        }
    }
    
    if without_lifetimes.len() > 60 {
        format!("{}...", &without_lifetimes[..57])
    } else {
        without_lifetimes
    }
}

/// 移除生命周期参数
/// Base64Display<'a, 'e, E> -> Base64Display<E>
/// Option<&(dyn Error + 'static)> -> Option<&(dyn Error)>
fn remove_lifetimes(type_name: &str) -> String {
    let mut result = String::with_capacity(type_name.len());
    let mut chars = type_name.chars().peekable();
    
    while let Some(ch) = chars.next() {
        if ch == '\'' {
            // 跳过生命周期名称
            while let Some(&next_ch) = chars.peek() {
                if next_ch.is_alphanumeric() || next_ch == '_' {
                    chars.next();
                } else {
                    break;
                }
            }
            
            // 跳过 'static 后的空格和 +
            while let Some(&' ') = chars.peek() {
                chars.next();
            }
            
            // 如果后面是 + 号，也要跳过（因为这是 trait bound 的一部分）
            if let Some(&'+') = chars.peek() {
                chars.next();
                // 跳过 + 后的空格
                while let Some(&' ') = chars.peek() {
                    chars.next();
                }
                
                // 如果 + 后面没有其他内容了（只有括号或 >），需要移除前面的空格和 +
                // 这个会在后面的 replace 中处理
            }
            
            // 如果是逗号，也跳过它和后续空格
            if let Some(&',') = chars.peek() {
                chars.next();
                while let Some(&' ') = chars.peek() {
                    chars.next();
                }
            }
        } else {
            result.push(ch);
        }
    }
    
    // 清理可能的多余字符
    result = result.replace("<, ", "<");
    result = result.replace(", >", ">");
    result = result.replace("< ", "<");
    result = result.replace(" >", ">");
    result = result.replace("<>", "");
    result = result.replace("  ", " ");
    result = result.replace(" +)", ")");  // 移除 trait bounds 结尾的 +
    result = result.replace("+ )", ")");
    result = result.replace("+)", ")");
    result = result.replace(" )", ")");   // 移除括号前的空格
    result = result.replace("( ", "(");   // 移除括号后的空格
    result = result.replace("dyn  ", "dyn ");
    
    result
}

/// 简化函数签名显示
/// 移除路径前缀、生命周期、简化泛型约束
/// 例如: fn encode<T: AsRef<[u8]>>(self: &Self, input: T) -> String
///   -> fn encode(self: &Self, input: T) -> String
fn simplify_signature(sig: &str) -> String {
    let sig = sig.trim();
    
    // 移除 const、unsafe 等修饰符（保留位置但简化）
    let mut result = sig.to_string();
    
    // 移除泛型约束（保留泛型参数但移除约束）
    // fn foo<T: Trait>(x: T) -> fn foo<T>(x: T)
    result = simplify_generic_bounds(&result);
    
    // 移除生命周期
    result = remove_lifetimes(&result);
    
    // 移除路径前缀 (std::string::String -> String)
    result = remove_type_paths(&result);
    
    // 限制长度
    if result.len() > 80 {
        if let Some(arrow_pos) = result[..80].rfind("->") {
            format!("{} -> ...", &result[..arrow_pos].trim())
        } else if let Some(paren_pos) = result[..80].rfind(')') {
            format!("{})", &result[..paren_pos])
        } else {
            format!("{}...", &result[..77])
        }
    } else {
        result
    }
}

/// 简化泛型约束
/// fn foo<T: Clone + Debug, U: Display>(x: T) -> fn foo<T, U>(x: T)
fn simplify_generic_bounds(sig: &str) -> String {
    let mut result = String::new();
    let mut chars = sig.chars().peekable();
    
    while let Some(ch) = chars.next() {
        match ch {
            '<' if result.ends_with("fn ") || result.chars().rev().take(10).any(|c| c.is_alphanumeric()) => {
                // 可能是泛型参数开始
                result.push(ch);
                
                // 跳过约束部分
                let mut generic_content = String::new();
                let mut bracket_level = 1;
                
                while let Some(&next_ch) = chars.peek() {
                    chars.next();
                    if next_ch == '<' {
                        bracket_level += 1;
                        generic_content.push(next_ch);
                    } else if next_ch == '>' {
                        bracket_level -= 1;
                        if bracket_level == 0 {
                            // 处理泛型内容，移除约束
                            let params: Vec<&str> = generic_content.split(',').collect();
                            let simplified_params: Vec<String> = params.iter().map(|p| {
                                // 提取参数名（在 : 之前的部分）
                                if let Some(colon_pos) = p.find(':') {
                                    p[..colon_pos].trim().to_string()
                                } else {
                                    p.trim().to_string()
                                }
                            }).collect();
                            result.push_str(&simplified_params.join(", "));
                            result.push('>');
                            break;
                        }
                        generic_content.push(next_ch);
                    } else {
                        generic_content.push(next_ch);
                    }
                }
            }
            _ => {
                result.push(ch);
            }
        }
    }
    
    result
}

/// 移除类型路径前缀
/// std::string::String -> String
/// <U as TryFrom<T>>::Error -> Error (不带尖括号)
fn remove_type_paths(sig: &str) -> String {
    let mut result = String::new();
    let mut current_word = String::new();
    let mut in_angle_brackets: i32 = 0;
    let mut bracket_start = 0;
    
    for ch in sig.chars() {
        match ch {
            '<' => {
                if in_angle_brackets == 0 {
                    bracket_start = result.len();
                }
                in_angle_brackets += 1;
                
                // 先处理当前的word
                if !current_word.is_empty() {
                    if let Some(last_colon) = current_word.rfind("::") {
                        result.push_str(&current_word[last_colon + 2..]);
                    } else {
                        result.push_str(&current_word);
                    }
                    current_word.clear();
                }
                result.push(ch);
            }
            '>' => {
                in_angle_brackets = in_angle_brackets.saturating_sub(1);
                
                // 先处理当前的word
                if !current_word.is_empty() {
                    // 检查是否是 qualified path (e.g. <T as Trait>::Type)
                    // 如果在尖括号内且有 ::，这可能是 associated type
                    if let Some(last_colon) = current_word.rfind("::") {
                        let type_name = &current_word[last_colon + 2..];
                        // 如果这是 qualified path 的最后一部分，移除前面的尖括号
                        if in_angle_brackets == 0 && result[bracket_start..].starts_with('<') {
                            // 这是类似 <T as Trait>::Error 的情况
                            // 移除整个 qualified path 的尖括号部分
                            result.truncate(bracket_start);
                            result.push_str(type_name);
                            current_word.clear();
                            // 不添加 >
                            continue;
                        } else {
                            result.push_str(type_name);
                        }
                    } else {
                        result.push_str(&current_word);
                    }
                    current_word.clear();
                }
                result.push(ch);
            }
            _ if ch.is_alphanumeric() || ch == '_' || ch == ':' => {
                current_word.push(ch);
            }
            _ => {
                if !current_word.is_empty() {
                    if let Some(last_colon) = current_word.rfind("::") {
                        result.push_str(&current_word[last_colon + 2..]);
                    } else {
                        result.push_str(&current_word);
                    }
                    current_word.clear();
                }
                result.push(ch);
            }
        }
    }
    
    if !current_word.is_empty() {
        if let Some(last_colon) = current_word.rfind("::") {
            result.push_str(&current_word[last_colon + 2..]);
        } else {
            result.push_str(&current_word);
        }
    }
    
    result
}

fn edge_attr(kind: ArcKind, label: Option<String>) -> String {
    let mut parts = Vec::new();

    if let Some(label) = label {
        parts.push(format!("label=\"{}\"", label));
    }

    match kind {
        ArcKind::Normal => {}
        ArcKind::Inhibitor => {
            parts.push("style=dashed".into());
            parts.push("arrowhead=dot".into());
        }
        ArcKind::Reset => {
            parts.push("color=\"firebrick\"".into());
            parts.push("arrowhead=tee".into());
        }
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" [{}]", parts.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustdoc_types::Type;
    use std::sync::Arc;

    fn descriptor_from_type(ty: Type) -> TypeDescriptor {
        TypeDescriptor::from_type(&ty)
    }

    #[test]
    fn dot_output_contains_nodes_and_edges() {
        let mut net = PetriNet::new();

        let parameter_desc = descriptor_from_type(Type::Primitive("u64".into()));
        let output_desc = descriptor_from_type(Type::Primitive("u32".into()));

        let place_in = net.add_place(parameter_desc.clone());
        let place_out = net.add_place(output_desc.clone());

        let summary = FunctionSummary {
            item_id: Id(0),
            name: Arc::<str>::from("example"),
            qualified_path: None,
            signature: Arc::<str>::from("fn example(value: u64) -> u32"),
            generics: Vec::new(),
            where_clauses: Vec::new(),
            trait_bounds: Vec::new(),
            context: FunctionContext::FreeFunction,
            inputs: vec![ParameterSummary {
                name: Some(Arc::<str>::from("value")),
                descriptor: parameter_desc.clone(),
            }],
            output: Some(output_desc.clone()),
        };

        let transition_id = net.add_transition(summary);

        net.add_input_arc_from_place(
            place_in,
            transition_id,
            ArcData {
                weight: 1,
                parameter: Some(ParameterSummary {
                    name: Some(Arc::<str>::from("value")),
                    descriptor: parameter_desc.clone(),
                }),
                kind: ArcKind::Normal,
                descriptor: None,
                borrow_kind: Some(parameter_desc.borrow_kind()),
            },
        );

        net.add_output_arc_to_place(
            transition_id,
            place_out,
            ArcData {
                weight: 1,
                parameter: None,
                kind: ArcKind::Normal,
                descriptor: Some(output_desc.clone()),
                borrow_kind: Some(output_desc.borrow_kind()),
            },
        );

        let dot = net.to_dot();

        assert!(dot.contains("digraph PetriNet"));
        assert!(dot.contains("p") && dot.contains("shape=circle"));
        assert!(dot.contains("t") && dot.contains("shape=box"));
        assert!(dot.contains("->"));
    }
}
