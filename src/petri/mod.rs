mod builder;
mod net;
mod synthesis;
pub mod schema;
pub mod guards;
pub mod converter;
pub(crate) mod type_repr;
pub(crate) mod util;

pub use builder::PetriNetBuilder;
pub use net::{
    ArcData, ArcWeight, FunctionContext, FunctionSummary, ParameterSummary, 
    PetriNet, PetriNetStatistics, Place, PlaceId, Token, Transition, TransitionId,
};
pub use synthesis::{StepState, SynthesisConfig, SynthesisOutcome, SynthesisPlan, Synthesizer};
pub use type_repr::{BorrowKind, TypeDescriptor};
pub use guards::{GuardContext, GuardEvaluator};
pub use schema::{
    JsonPetriNet, JsonPlace, JsonToken, JsonTransition, JsonEdge, 
    JsonGuard, JsonGuardCondition, JsonMetadata, JsonField,
};
pub use converter::{
    convert_rustdoc_to_petri, 
    convert_rustdoc_file_to_petri,
    convert_and_save,
    convert_rustdoc_to_petri_with_options,
    batch_convert,
    ConversionOptions,
};
