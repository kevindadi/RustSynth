use std::{fmt::Write, sync::Arc};

use petgraph::{Direction, graph::NodeIndex, stable_graph::StableGraph, visit::EdgeRef};
use rustdoc_types::Id;
use serde::{Deserialize, Serialize};

use super::type_repr::TypeDescriptor;

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
        for (id, place) in self.places() {
            let text = format!("p{}: {}", id.0.index(), place.descriptor.display());
            let label = html_label(&[text.as_str()]);
            let _ = writeln!(dot, "  p{} [shape=circle,label=<{}>];", id.0.index(), label);
        }
    }

    fn write_transitions(&self, dot: &mut String) {
        for (id, transition) in self.transitions() {
            let summary = &transition.summary;
            let label = html_label(&[summary.display_name(), summary.signature.as_ref()]);
            let _ = writeln!(dot, "  t{} [shape=box,label=<{}>];", id.0.index(), label);
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
        let name_ref = arc
            .parameter
            .as_ref()
            .and_then(|param| param.name.as_deref());
        let descriptor_ref = arc
            .parameter
            .as_ref()
            .map(|param| &param.descriptor)
            .or_else(|| self.place(place_id).map(|place| &place.descriptor));

        let label =
            descriptor_ref.and_then(|descriptor| edge_label_from_parameter(name_ref, descriptor));
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
        let label = arc
            .descriptor
            .as_ref()
            .or_else(|| self.place(place_id).map(|place| &place.descriptor))
            .map(|d| d.display().to_string());
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

fn html_label(parts: &[&str]) -> String {
    let mut label = String::new();
    for (idx, part) in parts.iter().enumerate() {
        if idx > 0 {
            label.push_str("<BR ALIGN=\"LEFT\"/>");
        }
        label.push_str(&html_escape(part));
    }
    label
}

fn weight_suffix(weight: ArcWeight) -> Option<String> {
    if weight == 1 {
        None
    } else {
        Some(format!("×{}", weight))
    }
}

fn edge_label_from_parameter(name: Option<&str>, descriptor: &TypeDescriptor) -> Option<String> {
    let mut label = String::new();
    if let Some(name) = name {
        if !name.trim().is_empty() {
            label.push_str(name);
            label.push_str(": ");
        }
    }
    label.push_str(descriptor.display());
    if label.is_empty() { None } else { Some(label) }
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
