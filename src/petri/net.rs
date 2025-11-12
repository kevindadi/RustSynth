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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransitionInput {
    pub place: PlaceId,
    pub multiplicity: ArcMultiplicity,
    pub parameter: ParameterSummary,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransitionOutput {
    pub place: PlaceId,
    pub multiplicity: ArcMultiplicity,
    pub descriptor: TypeDescriptor,
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
}

impl PetriNet {
    pub fn places(&self) -> impl Iterator<Item = &Place> {
        self.places.values()
    }

    pub fn transitions(&self) -> impl Iterator<Item = &Transition> {
        self.transitions.values()
    }

    pub fn place(&self, id: PlaceId) -> Option<&Place> {
        self.places.get(&id)
    }

    pub fn transition(&self, id: TransitionId) -> Option<&Transition> {
        self.transitions.get(&id)
    }

    pub fn place_id(&self, descriptor: &TypeDescriptor) -> Option<PlaceId> {
        self.place_lookup.get(descriptor).copied()
    }

    pub fn place_count(&self) -> usize {
        self.places.len()
    }

    pub fn transition_count(&self) -> usize {
        self.transitions.len()
    }

    /// 输出兼容 Graphviz 的 DOT 字符串，便于可视化调试。
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
                let label = edge_label_from_parameter(
                    input.parameter.name.as_deref(),
                    &input.parameter.descriptor,
                );
                let multiplicity = multiplicity_suffix(input.multiplicity);
                let attr = edge_attr(combine_edge_parts(label, multiplicity));
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
                let attr = edge_attr(combine_edge_parts(label, multiplicity));
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

    pub(crate) fn insert_place(&mut self, place: Place) {
        self.place_lookup
            .insert(place.descriptor.clone(), place.id);
        self.places.insert(place.id, place);
    }

    pub(crate) fn insert_transition(&mut self, transition: Transition) {
        self.transitions.insert(transition.id, transition);
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

fn edge_attr(label: Option<String>) -> String {
    if let Some(label) = label {
        format!(" [label=<{}>]", html_escape(&label))
    } else {
        String::new()
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
        let mut net = PetriNet::default();

        let place_in = Place {
            id: PlaceId(0),
            descriptor: descriptor_from_type(Type::Primitive("u64".into())),
        };
        let place_out = Place {
            id: PlaceId(1),
            descriptor: descriptor_from_type(Type::Primitive("u32".into())),
        };
        net.insert_place(place_in.clone());
        net.insert_place(place_out.clone());

        let parameter_desc = descriptor_from_type(Type::Primitive("u64".into()));
        let output_desc = descriptor_from_type(Type::Primitive("u32".into()));

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

        let transition = Transition {
            id: TransitionId(0),
            summary,
            inputs: vec![TransitionInput {
                place: place_in.id,
                multiplicity: ArcMultiplicity::One,
                parameter: ParameterSummary {
                    name: Some(Arc::<str>::from("value")),
                    descriptor: parameter_desc,
                },
            }],
            outputs: vec![TransitionOutput {
                place: place_out.id,
                multiplicity: ArcMultiplicity::One,
                descriptor: output_desc,
            }],
        };

        net.insert_transition(transition);

        let dot = net.to_dot();

        assert!(dot.contains("digraph PetriNet"));
        assert!(dot.contains("p0 [shape=circle"));
        assert!(dot.contains("p1 [shape=circle"));
        assert!(dot.contains("t0 [shape=box"));
        assert!(dot.contains("p0 -> t0"));
        assert!(dot.contains("t0 -> p1"));
    }
}

