use std::{fmt::Write, sync::Arc};

use petgraph::{
    Direction, 
    graph::NodeIndex, 
    stable_graph::StableGraph, 
    visit::EdgeRef,
    algo::{dijkstra, is_cyclic_directed},
};
use rustdoc_types::Id;
use serde::{Deserialize, Serialize};

use super::type_repr::{BorrowKind, TypeDescriptor};

/// Token 表示 Petri 网中的令牌
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

    /// 从类型描述符创建 token（使用描述符本身的借用类型）
    pub fn from_descriptor(descriptor: TypeDescriptor) -> Self {
        Self {
            borrow_kind: descriptor.borrow_kind(),
            descriptor: descriptor.normalized(),
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Place {
    pub descriptor: TypeDescriptor,
    /// 该类型实现的 trait 列表（用于基本类型）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implemented_traits: Vec<Arc<str>>,
    /// 该泛型参数需要的 trait 约束（用于泛型参数）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_trait_bounds: Vec<Arc<str>>,
    /// 是否为泛型参数类型
    #[serde(default)]
    pub is_generic_parameter: bool,
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

/// 类型被建模为 Place, 可函数被建模为 Transition.
/// 调用时会消耗参数类型的令牌, 若有返回值则产生返回类型的令牌.
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

#[derive(Debug, Serialize, Deserialize)]
pub struct PetriNet {
    #[serde(with = "petgraph_serde")]
    graph: StableGraph<Node, ArcData>,
    #[serde(skip)]
    place_lookup: indexmap::IndexMap<TypeDescriptor, PlaceId>,
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
        // 使用规范化版本进行查找和存储，这样 T、&T、&mut T 都映射到同一个库所
        let normalized = descriptor.normalized();
        if let Some(&id) = self.place_lookup.get(&normalized) {
            return id;
        }

        let place = Place {
            descriptor: normalized.clone(),
            implemented_traits: Vec::new(),
            required_trait_bounds: Vec::new(),
            is_generic_parameter: false,
        };
        let node_idx = self.graph.add_node(Node::Place(place));
        let id = PlaceId(node_idx);

        self.place_lookup.insert(normalized, id);
        id
    }

    /// 添加基本类型库所，并记录其实现的 trait
    pub fn add_primitive_place(
        &mut self,
        descriptor: TypeDescriptor,
        implemented_traits: Vec<Arc<str>>,
    ) -> PlaceId {
        let normalized = descriptor.normalized();
        if let Some(&id) = self.place_lookup.get(&normalized) {
            // 如果已存在，更新实现的 trait
            if let Some(place) = self.graph.node_weight_mut(id.0) {
                if let Node::Place(place) = place {
                    let mut existing_traits: std::collections::HashSet<_> =
                        place.implemented_traits.iter().cloned().collect();
                    for trait_ in implemented_traits {
                        existing_traits.insert(trait_);
                    }
                    place.implemented_traits = existing_traits.into_iter().collect();
                    place.implemented_traits.sort();
                }
            }
            return id;
        }

        let mut traits = implemented_traits;
        traits.sort();
        let place = Place {
            descriptor: normalized.clone(),
            implemented_traits: traits,
            required_trait_bounds: Vec::new(),
            is_generic_parameter: false,
        };
        let node_idx = self.graph.add_node(Node::Place(place));
        let id = PlaceId(node_idx);

        self.place_lookup.insert(normalized, id);
        id
    }

    /// 添加泛型参数库所，并记录其需要的 trait 约束
    pub fn add_generic_parameter_place(
        &mut self,
        descriptor: TypeDescriptor,
        required_bounds: Vec<Arc<str>>,
    ) -> PlaceId {
        let normalized = descriptor.normalized();
        if let Some(&id) = self.place_lookup.get(&normalized) {
            // 如果已存在，更新 trait 约束
            if let Some(place) = self.graph.node_weight_mut(id.0) {
                if let Node::Place(place) = place {
                    let mut existing_bounds: std::collections::HashSet<_> =
                        place.required_trait_bounds.iter().cloned().collect();
                    for bound in required_bounds {
                        existing_bounds.insert(bound);
                    }
                    place.required_trait_bounds = existing_bounds.into_iter().collect();
                    place.required_trait_bounds.sort();
                    place.is_generic_parameter = true;
                }
            }
            return id;
        }

        let mut bounds = required_bounds;
        bounds.sort();
        let place = Place {
            descriptor: normalized.clone(),
            implemented_traits: Vec::new(),
            required_trait_bounds: bounds,
            is_generic_parameter: true,
        };
        let node_idx = self.graph.add_node(Node::Place(place));
        let id = PlaceId(node_idx);

        self.place_lookup.insert(normalized, id);
        id
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
            primitive_place.descriptor.clone()
        };
        let generic_descriptor = {
            let generic_place = self.place(to_generic).expect("generic place should exist");
            generic_place.descriptor.clone()
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
        // 使用规范化版本进行查找
        let normalized = descriptor.normalized();
        self.place_lookup.get(&normalized).copied()
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
        let mut other_places = Vec::new();

        for (id, place) in self.places() {
            let type_name = place.descriptor.display();
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

        for (id, place) in primitive_places {
            let label = html_escape(place.descriptor.display());
            let _ = writeln!(
                dot,
                "  p{} [shape=circle,style=filled,fillcolor=lightblue,label=\"{}\"];",
                id.0.index(),
                label
            );
        }

        for (id, place) in other_places {
            let label = html_escape(place.descriptor.display());
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
            let name = summary.display_name();
            let sig = summary.signature.as_ref();

            // 简化签名显示（只显示函数名和主要类型）
            let short_sig = if sig.len() > 100 {
                // 截断过长的签名
                let truncated = &sig[..97];
                format!("{}...", truncated)
            } else {
                sig.to_string()
            };

            let label = format!("{}<BR/>{}", html_escape(name), html_escape(&short_sig));
            let _ = writeln!(
                dot,
                "  t{} [shape=box,style=rounded,label=<{}>];",
                id.0.index(),
                label
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
        // 构建边标签
        let mut label_parts = Vec::new();

        // 添加参数名（如果有）
        if let Some(name) = arc
            .parameter
            .as_ref()
            .and_then(|param| param.name.as_deref())
        {
            label_parts.push(html_escape(name));
        }

        // 添加借用类型标记
        if let Some(borrow_kind) = arc.borrow_kind {
            let borrow_mark = match borrow_kind {
                BorrowKind::Owned => "",
                BorrowKind::SharedRef => "&",
                BorrowKind::MutRef => "&mut ",
                BorrowKind::RawConstPtr => "*const ",
                BorrowKind::RawMutPtr => "*mut ",
            };
            if !borrow_mark.is_empty() {
                label_parts.push(borrow_mark.to_string());
            }
        }

        // 添加类型名（简化显示）
        if let Some(descriptor) = arc
            .parameter
            .as_ref()
            .map(|param| &param.descriptor)
            .or_else(|| self.place(place_id).map(|place| &place.descriptor))
        {
            let type_name = descriptor.display();
            // 简化类型名显示
            let short_name = if type_name.len() > 30 {
                let parts: Vec<&str> = type_name.split("::").collect();
                if parts.len() > 2 {
                    format!("...{}", parts.last().unwrap_or(&type_name))
                } else {
                    format!("{}...", &type_name[..27])
                }
            } else {
                type_name.to_string()
            };
            label_parts.push(html_escape(&short_name));
        }

        let label = if label_parts.is_empty() {
            None
        } else {
            Some(label_parts.join(" "))
        };

        let attr = edge_attr(
            arc.kind,
            combine_edge_parts(label, weight_suffix(arc.weight)),
        );
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
        // 构建边标签
        let mut label_parts = Vec::new();

        // 添加借用类型标记
        if let Some(borrow_kind) = arc.borrow_kind {
            let borrow_mark = match borrow_kind {
                BorrowKind::Owned => "",
                BorrowKind::SharedRef => "&",
                BorrowKind::MutRef => "&mut ",
                BorrowKind::RawConstPtr => "*const ",
                BorrowKind::RawMutPtr => "*mut ",
            };
            if !borrow_mark.is_empty() {
                label_parts.push(borrow_mark.to_string());
            }
        }

        // 添加类型名
        if let Some(descriptor) = arc
            .descriptor
            .as_ref()
            .or_else(|| self.place(place_id).map(|place| &place.descriptor))
        {
            let type_name = descriptor.display();
            // 简化类型名显示
            let short_name = if type_name.len() > 30 {
                let parts: Vec<&str> = type_name.split("::").collect();
                if parts.len() > 2 {
                    format!("...{}", parts.last().unwrap_or(&type_name))
                } else {
                    format!("{}...", &type_name[..27])
                }
            } else {
                type_name.to_string()
            };
            label_parts.push(html_escape(&short_name));
        }

        let label = if label_parts.is_empty() {
            None
        } else {
            Some(label_parts.join(" "))
        };

        let attr = edge_attr(
            arc.kind,
            combine_edge_parts(label, weight_suffix(arc.weight)),
        );
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

    /// 查找能够产生指定 Place 的所有 Transitions
    pub fn find_producers(&self, place: PlaceId) -> Vec<(TransitionId, &ArcData)> {
        self.graph
            .edges_directed(place.0, Direction::Incoming)
            .filter_map(|edge| {
                let source = edge.source();
                if let Node::Transition(_) = &self.graph[source] {
                    Some((TransitionId(source), edge.weight()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// 查找需要指定 Place 作为输入的所有 Transitions
    pub fn find_consumers(&self, place: PlaceId) -> Vec<(TransitionId, &ArcData)> {
        self.graph
            .edges_directed(place.0, Direction::Outgoing)
            .filter_map(|edge| {
                let target = edge.target();
                if let Node::Transition(_) = &self.graph[target] {
                    Some((TransitionId(target), edge.weight()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// 获取图的统计信息
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

    /// 获取底层的 petgraph StableGraph 的不可变引用
    /// 这允许用户使用 petgraph 的所有算法
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

fn html_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            '\n' => escaped.push_str("<BR ALIGN=\"LEFT\"/>"),
            '\r' => {}
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn weight_suffix(weight: ArcWeight) -> Option<String> {
    if weight == 1 {
        None
    } else {
        Some(format!("×{}", weight))
    }
}

fn combine_edge_parts(main: Option<String>, weight: Option<String>) -> Option<String> {
    match (main, weight) {
        (None, None) => None,
        (Some(label), None) => Some(label),
        (None, Some(w)) => Some(w),
        (Some(mut label), Some(w)) => {
            if !label.is_empty() {
                label.push(' ');
            }
            label.push_str(&w);
            Some(label)
        }
    }
}

fn edge_attr(kind: ArcKind, label: Option<String>) -> String {
    let mut parts = Vec::new();

    if let Some(label) = label {
        parts.push(format!("label=<{}>", html_escape(&label)));
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
