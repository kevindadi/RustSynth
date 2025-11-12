use std::{fmt::Write, sync::Arc};

use indexmap::IndexMap;
use rustdoc_types::Id;
use serde::{Deserialize, Serialize};

use super::type_repr::TypeDescriptor;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlaceId(pub(crate) usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransitionId(pub(crate) usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArcMultiplicity {
    One,
    Many(u32),
}

impl Default for ArcMultiplicity {
    fn default() -> Self {
        ArcMultiplicity::One
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Place {
    pub id: PlaceId,
    pub descriptor: TypeDescriptor,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParameterSummary {
    pub name: Option<Arc<str>>,
    pub descriptor: TypeDescriptor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArcKind {
    Normal,
    Inhibitor,
    Reset,
}

impl ArcKind {
    fn default_normal() -> Self {
        ArcKind::Normal
    }
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

/// Petri 网中函数信息的摘要.
///
/// 类型被建模为 Place, 可调用实体被建模为 Transition.调用时会消耗参数类型的令牌,
/// 若有返回值则产生返回类型的令牌.
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransitionInput {
    pub place: PlaceId,
    pub multiplicity: ArcMultiplicity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter: Option<ParameterSummary>,
    #[serde(default = "ArcKind::default_normal")]
    pub kind: ArcKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransitionOutput {
    pub place: PlaceId,
    pub multiplicity: ArcMultiplicity,
    pub descriptor: TypeDescriptor,
    #[serde(default = "ArcKind::default_normal")]
    pub kind: ArcKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transition {
    pub id: TransitionId,
    pub summary: FunctionSummary,
    pub inputs: Vec<TransitionInput>,
    pub outputs: Vec<TransitionOutput>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct PetriNet {
    places: IndexMap<PlaceId, Place>,
    transitions: IndexMap<TransitionId, Transition>,
    #[serde(skip)]
    place_lookup: IndexMap<TypeDescriptor, PlaceId>,
    #[serde(skip)]
    next_place_id: usize,
    #[serde(skip)]
    next_transition_id: usize,
}

impl PetriNet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_place(&mut self, descriptor: TypeDescriptor) -> PlaceId {
        self.rebuild_place_lookup_if_needed();
        if let Some(id) = self.place_lookup.get(&descriptor) {
            return *id;
        }

        self.sync_place_counter();
        let id = PlaceId(self.next_place_id);
        self.next_place_id += 1;

        self.place_lookup.insert(descriptor.clone(), id);
        self.places.insert(id, Place { id, descriptor });
        id
    }

    pub fn add_transition(&mut self, summary: FunctionSummary) -> TransitionId {
        self.sync_transition_counter();
        let id = TransitionId(self.next_transition_id);
        self.next_transition_id += 1;

        self.transitions.insert(
            id,
            Transition {
                id,
                summary,
                inputs: Vec::new(),
                outputs: Vec::new(),
            },
        );

        id
    }

    pub fn add_input_arc(&mut self, transition: TransitionId, arc: TransitionInput) {
        self.transitions
            .get_mut(&transition)
            .expect("transition not found when adding input arc")
            .inputs
            .push(arc);
    }

    pub fn add_output_arc(&mut self, transition: TransitionId, arc: TransitionOutput) {
        self.transitions
            .get_mut(&transition)
            .expect("transition not found when adding output arc")
            .outputs
            .push(arc);
    }

    fn sync_place_counter(&mut self) {
        if self.next_place_id == 0 && !self.places.is_empty() {
            if let Some(max_id) = self.places.keys().map(|id| id.0).max() {
                self.next_place_id = max_id + 1;
            }
        }
    }

    fn sync_transition_counter(&mut self) {
        if self.next_transition_id == 0 && !self.transitions.is_empty() {
            if let Some(max_id) = self.transitions.keys().map(|id| id.0).max() {
                self.next_transition_id = max_id + 1;
            }
        }
    }

    fn rebuild_place_lookup_if_needed(&mut self) {
        if self.place_lookup.len() == self.places.len() {
            return;
        }

        self.place_lookup.clear();
        for (id, place) in &self.places {
            self.place_lookup.insert(place.descriptor.clone(), *id);
        }
    }

    /// 以插入顺序遍历所有 Place(类型节点).    
    pub fn places(&self) -> impl Iterator<Item = &Place> {
        self.places.values()
    }

    /// 以插入顺序遍历所有 Transition(函数/方法节点).
    pub fn transitions(&self) -> impl Iterator<Item = &Transition> {
        self.transitions.values()
    }

    /// 根据 ID 查找指定 Place.
    pub fn place(&self, id: PlaceId) -> Option<&Place> {
        self.places.get(&id)
    }

    /// 根据 ID 查找指定 Transition.
    pub fn transition(&self, id: TransitionId) -> Option<&Transition> {
        self.transitions.get(&id)
    }

    /// 若类型已存在, 返回对应的 Place ID.
    pub fn place_id(&self, descriptor: &TypeDescriptor) -> Option<PlaceId> {
        self.place_lookup
            .get(descriptor)
            .copied()
            .or_else(|| {
                self.places
                    .iter()
                    .find_map(|(id, place)| (place.descriptor == *descriptor).then_some(*id))
            })
    }

    /// 返回 Place(类型节点) 总数.
    pub fn place_count(&self) -> usize {
        self.places.len()
    }

    /// 返回 Transition(函数/方法节点) 总数.
    pub fn transition_count(&self) -> usize {
        self.transitions.len()
    }

    /// 输出兼容 Graphviz 的 DOT 字符串,便于可视化调试.
    pub fn to_dot(&self) -> String {
        let mut dot = String::new();
        dot.push_str("digraph PetriNet {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  node [fontname=\"Helvetica\"];\n");

        for place in self.places.values() {
            let text = format!("p{}: {}", place.id.0, place.descriptor.display());
            let label = html_label(&[text.as_str()]);
            let _ = writeln!(
                dot,
                "  p{} [shape=circle,label=<{}>];",
                place.id.0,
                label
            );
        }

        for transition in self.transitions.values() {
            let summary = &transition.summary;
            let name = summary
                .qualified_path
                .as_ref()
                .map(|p| p.as_ref())
                .unwrap_or(summary.name.as_ref());
            let label = html_label(&[name, summary.signature.as_ref()]);
            let _ = writeln!(
                dot,
                "  t{} [shape=box,label=<{}>];",
                transition.id.0,
                label
            );
        }

        for transition in self.transitions.values() {
            for input in &transition.inputs {
                let name_ref = input
                    .parameter
                    .as_ref()
                    .and_then(|param| param.name.as_deref());
                let descriptor_ref = input
                    .parameter
                    .as_ref()
                    .map(|param| &param.descriptor)
                    .or_else(|| self.place(input.place).map(|place| &place.descriptor));
                let label = descriptor_ref.and_then(|descriptor| {
                    edge_label_from_parameter(name_ref, descriptor)
                });
                let multiplicity = multiplicity_suffix(input.multiplicity);
                let attr = edge_attr(
                    input.kind,
                    combine_edge_parts(label, multiplicity),
                );
                let _ = writeln!(
                    dot,
                    "  p{} -> t{}{};",
                    input.place.0,
                    transition.id.0,
                    attr
                );
            }

            for output in &transition.outputs {
                let label = Some(output.descriptor.display().to_string());
                let multiplicity = multiplicity_suffix(output.multiplicity);
                let attr = edge_attr(
                    output.kind,
                    combine_edge_parts(label, multiplicity),
                );
                let _ = writeln!(
                    dot,
                    "  t{} -> p{}{};",
                    transition.id.0,
                    output.place.0,
                    attr
                );
            }
        }

        dot.push_str("}\n");
        dot
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

fn multiplicity_suffix(multiplicity: ArcMultiplicity) -> Option<String> {
    match multiplicity {
        ArcMultiplicity::One => None,
        ArcMultiplicity::Many(n) => Some(format!("×{}", n)),
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
    if label.is_empty() {
        None
    } else {
        Some(label)
    }
}

fn combine_edge_parts(main: Option<String>, multiplicity: Option<String>) -> Option<String> {
    match (main, multiplicity) {
        (None, None) => None,
        (Some(label), None) => Some(label),
        (None, Some(multi)) => Some(multi),
        (Some(mut label), Some(multi)) => {
            if !label.is_empty() {
                label.push(' ');
            }
            label.push_str(&multi);
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

        net.add_input_arc(
            transition_id,
            TransitionInput {
                place: place_in,
                multiplicity: ArcMultiplicity::One,
                parameter: Some(ParameterSummary {
                    name: Some(Arc::<str>::from("value")),
                    descriptor: parameter_desc,
                }),
                kind: ArcKind::Normal,
            },
        );

        net.add_output_arc(
            transition_id,
            TransitionOutput {
                place: place_out,
                multiplicity: ArcMultiplicity::One,
                descriptor: output_desc,
                kind: ArcKind::Normal,
            },
        );

        let dot = net.to_dot();

        assert!(dot.contains("digraph PetriNet"));
        assert!(dot.contains("p0 [shape=circle"));
        assert!(dot.contains("p1 [shape=circle"));
        assert!(dot.contains("t0 [shape=box"));
        assert!(dot.contains("p0 -> t0"));
        assert!(dot.contains("t0 -> p1"));
    }
}

