pub mod builder;
pub mod structure;

pub use builder::PetriNetBuilder;
pub use structure::{
    EdgeData, EdgeKind, NodePayload, PetriNet, PlaceData, PlaceId, TransitionData, TransitionId,
    TransitionKind,
};
