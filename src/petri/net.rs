use petgraph::{Direction, graph::NodeIndex, stable_graph::StableGraph, visit::EdgeRef};
use rustdoc_types::Id;

use crate::petri::structure::{Flow, Place, Transition};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlaceId(pub NodeIndex);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TransitionId(pub NodeIndex);

#[derive(Clone, Debug)]
pub enum Node {
    Place(Place),
    Transition(Transition),
}

#[derive(Debug)]
pub struct PetriNet {
    pub(crate) graph: StableGraph<Node, Flow>,

    pub struct_places: indexmap::IndexMap<Id, PlaceId>,
    pub enum_places: indexmap::IndexMap<Id, PlaceId>,
    pub union_places: indexmap::IndexMap<Id, PlaceId>,
    pub variant_places: indexmap::IndexMap<Id, PlaceId>,
}

impl Default for PetriNet {
    fn default() -> Self {
        Self {
            graph: StableGraph::new(),
            struct_places: indexmap::IndexMap::new(),
            enum_places: indexmap::IndexMap::new(),
            union_places: indexmap::IndexMap::new(),
            variant_places: indexmap::IndexMap::new(),
        }
    }
}

impl PetriNet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_place(&mut self, place: Place) {
        self.graph.add_node(Node::Place(place));
    }

    pub fn add_place_and_get_id(&mut self, place: Place) -> PlaceId {
        PlaceId(self.graph.add_node(Node::Place(place)))
    }

    pub fn add_transition(&mut self, transition: Transition) {
        self.graph.add_node(Node::Transition(transition));
    }

    pub fn add_transition_and_get_id(&mut self, transition: Transition) -> TransitionId {
        TransitionId(self.graph.add_node(Node::Transition(transition)))
    }

    pub fn add_flow(&mut self, place: PlaceId, transition: TransitionId, flow: Flow) {
        self.graph.add_edge(place.0, transition.0, flow);
    }

    pub fn add_flow_from_transition(
        &mut self,
        transition: TransitionId,
        place: PlaceId,
        flow: Flow,
    ) {
        self.graph.add_edge(transition.0, place.0, flow);
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

    /// 获取 Transition 的所有输入边(从 Place 到 Transition)
    pub fn transition_inputs(
        &self,
        transition: TransitionId,
    ) -> impl Iterator<Item = (PlaceId, &Flow)> {
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

    /// 获取 Transition 的所有输出边(从 Transition 到 Place)
    pub fn transition_outputs(
        &self,
        transition: TransitionId,
    ) -> impl Iterator<Item = (PlaceId, &Flow)> {
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
}
