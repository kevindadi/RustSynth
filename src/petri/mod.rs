mod builder;
pub mod converter;
pub mod guards;
mod net;
pub mod schema;
mod synthesis;
pub(crate) mod type_repr;
pub(crate) mod util;

pub use builder::PetriNetBuilder;
pub use converter::{
    ConversionOptions, batch_convert, convert_and_save, convert_rustdoc_file_to_petri,
    convert_rustdoc_to_petri, convert_rustdoc_to_petri_with_options,
};
pub use guards::{GuardContext, GuardEvaluator};
pub use net::{
    ArcData, ArcWeight, FunctionContext, FunctionSummary, ParameterSummary, PetriNet,
    PetriNetStatistics, Place, PlaceId, Token, Transition, TransitionId,
};
pub use schema::{
    JsonEdge, JsonField, JsonGuard, JsonGuardCondition, JsonMetadata, JsonPetriNet, JsonPlace,
    JsonToken, JsonTransition,
};
pub use synthesis::{StepState, SynthesisConfig, SynthesisOutcome, SynthesisPlan, Synthesizer};
pub use type_repr::{BorrowKind, TypeDescriptor};
