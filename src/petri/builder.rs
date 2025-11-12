use std::collections::BTreeSet;
use std::sync::Arc;

use indexmap::IndexMap;
use rustdoc_types::{Crate, Function, GenericParamDefKind, Impl, Item, ItemEnum};

use super::net::{
    ArcMultiplicity, FunctionContext, FunctionSummary, ParameterSummary, PetriNet, Place,
    PlaceId, Transition, TransitionId, TransitionInput, TransitionOutput,
};
use super::type_repr::TypeDescriptor;
use super::util::TypeFormatter;

pub struct PetriNetBuilder<'a> {
    crate_: &'a Crate,
    net: PetriNet,
    place_index: IndexMap<TypeDescriptor, PlaceId>,
    next_place_id: usize,
    next_transition_id: usize,
}

impl<'a> PetriNetBuilder<'a> {
    pub fn new(crate_: &'a Crate) -> Self {
        Self {
            crate_,
            net: PetriNet::default(),
            place_index: IndexMap::new(),
            next_place_id: 0,
            next_transition_id: 0,
        }
    }

    pub fn from_crate(crate_: &'a Crate) -> PetriNet {
        let mut builder = Self::new(crate_);
        builder.ingest();
        builder.finish()
    }

    pub fn ingest(&mut self) {
        for item in self.crate_.index.values() {
            match &item.inner {
                ItemEnum::Function(func) => {
                    self.ingest_function(item, func, FunctionContext::FreeFunction);
                }
                ItemEnum::Impl(impl_block) => {
                    self.ingest_impl(item, impl_block);
                }
                _ => {}
            }
        }
    }

    pub fn finish(mut self) -> PetriNet {
        // Reconstruct lookup table inside net using the builder-side index to avoid duplicates.
        for (descriptor, id) in &self.place_index {
            if self.net.place_id(descriptor).is_none() {
                self.net.insert_place(Place {
                    id: *id,
                    descriptor: descriptor.clone(),
                });
            }
        }
        self.net
    }

    fn ingest_impl(&mut self, item: &Item, impl_block: &Impl) {
        let receiver = TypeDescriptor::from_type(&impl_block.for_);
        let context = if let Some(trait_path) = &impl_block.trait_ {
            let trait_path_str = Arc::<str>::from(TypeFormatter::path_to_string(trait_path));
            FunctionContext::TraitImplementation {
                receiver: receiver.clone(),
                trait_path: trait_path_str,
            }
        } else {
            FunctionContext::InherentMethod {
                receiver: receiver.clone(),
            }
        };

        let impl_generics = TypeFormatter::format_generic_params(&impl_block.generics.params);
        let impl_where = TypeFormatter::format_where_predicates(&impl_block.generics.where_predicates);

        let mut impl_trait_bounds = Vec::new();
        if let Some(trait_path) = &impl_block.trait_ {
            impl_trait_bounds.push(TypeFormatter::path_to_string(trait_path));
        }

        for item_id in &impl_block.items {
            if let Some(inner_item) = self.crate_.index.get(item_id) {
                if let ItemEnum::Function(func) = &inner_item.inner {
                    self.ingest_function_with_context(
                        inner_item,
                        func,
                        context.clone(),
                        impl_generics.clone(),
                        impl_where.clone(),
                        impl_trait_bounds.clone(),
                    );
                }
            }
        }

        // Impl items declared elsewhere (e.g. provided by trait) are recorded in `provided_trait_methods`.
        if !impl_block.provided_trait_methods.is_empty() && impl_block.trait_.is_some() {
            // Nothing to add: provided methods have default bodies in the trait, so they are not callable here.
        }

        // Methods referenced via `item` (impl item itself) are handled via the loop above.
        let _ = item;
    }

    fn ingest_function(&mut self, item: &Item, func: &Function, context: FunctionContext) {
        self.ingest_function_with_context(
            item,
            func,
            context,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }

    fn ingest_function_with_context(
        &mut self,
        item: &Item,
        func: &Function,
        context: FunctionContext,
        impl_generics: Vec<String>,
        impl_where: Vec<String>,
        impl_trait_bounds: Vec<String>,
    ) {
        let mut inputs = Vec::new();
        let mut transition_inputs = Vec::new();
        for (name, ty) in &func.sig.inputs {
            let descriptor = TypeDescriptor::from_type(ty);
            let place_id = self.ensure_place(descriptor.clone());
            let parameter = ParameterSummary {
                name: (!name.is_empty()).then(|| Arc::<str>::from(name.as_str())),
                descriptor: descriptor.clone(),
            };
            inputs.push(parameter.clone());
            transition_inputs.push(TransitionInput {
                place: place_id,
                multiplicity: ArcMultiplicity::One,
                parameter,
            });
        }

        let output_descriptor = func
            .sig
            .output
            .as_ref()
            .map(|ty| TypeDescriptor::from_type(ty));

        let outputs = output_descriptor
            .as_ref()
            .map(|descriptor| {
                let place_id = self.ensure_place(descriptor.clone());
                TransitionOutput {
                    place: place_id,
                    multiplicity: ArcMultiplicity::One,
                    descriptor: descriptor.clone(),
                }
            })
            .into_iter()
            .collect::<Vec<_>>();

        let mut generics = impl_generics
            .into_iter()
            .chain(TypeFormatter::format_generic_params(&func.generics.params).into_iter())
            .map(|s| Arc::<str>::from(s))
            .collect::<Vec<_>>();
        dedup_arc_vec(&mut generics);

        let mut where_clauses = impl_where
            .into_iter()
            .chain(
                TypeFormatter::format_where_predicates(&func.generics.where_predicates).into_iter(),
            )
            .map(|s| Arc::<str>::from(s))
            .collect::<Vec<_>>();
        dedup_arc_vec(&mut where_clauses);

        let mut trait_bounds = impl_trait_bounds
            .into_iter()
            .map(|s| Arc::<str>::from(s))
            .collect::<Vec<_>>();
        trait_bounds.extend(
            extract_trait_bounds(&func.generics.params)
                .into_iter()
                .map(Arc::<str>::from),
        );
        dedup_arc_vec(&mut trait_bounds);

        let signature = Arc::<str>::from(TypeFormatter::function_signature(
            func,
            item.name.as_deref().unwrap_or("<anonymous>"),
        ));

        let function_summary = FunctionSummary {
            item_id: item.id,
            name: Arc::<str>::from(item.name.as_deref().unwrap_or("<anonymous>")),
            qualified_path: self.lookup_qualified_path(item),
            signature,
            generics,
            where_clauses,
            trait_bounds,
            context,
            inputs,
            output: output_descriptor,
        };

        let transition = Transition {
            id: self.next_transition_id(),
            summary: function_summary,
            inputs: transition_inputs,
            outputs,
        };

        self.net.insert_transition(transition);
    }

    fn ensure_place(&mut self, descriptor: TypeDescriptor) -> PlaceId {
        if let Some(id) = self.place_index.get(&descriptor) {
            return *id;
        }

        let id = PlaceId(self.next_place_id);
        self.next_place_id += 1;
        self.place_index.insert(descriptor.clone(), id);
        self.net.insert_place(Place {
            id,
            descriptor,
        });
        id
    }

    fn next_transition_id(&mut self) -> TransitionId {
        let id = TransitionId(self.next_transition_id);
        self.next_transition_id += 1;
        id
    }

    fn lookup_qualified_path(&self, item: &Item) -> Option<Arc<str>> {
        self.crate_
            .paths
            .get(&item.id)
            .map(|summary| Arc::<str>::from(summary.path.join("::")))
    }
}

fn dedup_arc_vec(vec: &mut Vec<Arc<str>>) {
    let mut seen = BTreeSet::new();
    vec.retain(|value| seen.insert(value.clone()));
}

fn extract_trait_bounds(params: &[rustdoc_types::GenericParamDef]) -> Vec<String> {
    let mut bounds = Vec::new();
    for param in params {
        if let GenericParamDefKind::Type { bounds: param_bounds, .. } = &param.kind {
            for bound in param_bounds {
                bounds.push(TypeFormatter::format_generic_bound(bound));
            }
        }
    }
    bounds
}

