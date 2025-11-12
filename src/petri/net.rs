use std::sync::Arc;

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

    pub(crate) fn insert_place(&mut self, place: Place) {
        self.place_lookup
            .insert(place.descriptor.clone(), place.id);
        self.places.insert(place.id, place);
    }

    pub(crate) fn insert_transition(&mut self, transition: Transition) {
        self.transitions.insert(transition.id, transition);
    }
}

