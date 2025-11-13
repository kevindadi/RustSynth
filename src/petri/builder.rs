use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use rustdoc_types::{Crate, Function, GenericParamDefKind, Id, Impl, Item, ItemEnum, Path as RustdocPath, Type};

use super::net::{
    ArcData, ArcKind, FunctionContext, FunctionSummary, ParameterSummary, PetriNet,
    PlaceId,
};
use super::type_repr::TypeDescriptor;
use super::util::TypeFormatter;

pub struct PetriNetBuilder<'a> {
    crate_: &'a Crate,
    net: PetriNet,
    impl_function_ids: HashSet<Id>,
}

impl<'a> PetriNetBuilder<'a> {
    /// 基于 rustdoc JSON 构造新的 Petri 网构建器
    ///
    /// 构建器会遍历 rustdoc 的 index , 将公开函数/方法映射为在类型之间移动令牌的变迁
    pub fn new(crate_: &'a Crate) -> Self {
        Self {
            crate_,
            net: PetriNet::new(),
            impl_function_ids: HashSet::new(),
        }
    }

    pub fn from_crate(crate_: &'a Crate) -> PetriNet {
        let mut builder = Self::new(crate_);
        builder.ingest();
        builder.finish()
    }

    /// 遍历 rustdoc 索引, 将所有函数类条目注册为变迁
    ///
    /// 自由函数直接记入; impl块中的方法在上下文里携带 Self, 便于后续替换关联类型.
    pub fn ingest(&mut self) {
        for item in self.crate_.index.values() {
            if let ItemEnum::Impl(impl_block) = &item.inner {
                self.ingest_impl(item, impl_block);
            }
        }

        for item in self.crate_.index.values() {
            if let ItemEnum::Function(func) = &item.inner {
                if self.impl_function_ids.contains(&item.id) {
                    continue;
                }
                if !func.has_body {
                    continue;
                }
                self.ingest_function(item, func, FunctionContext::FreeFunction);
            }
        }
    }

    pub fn finish(self) -> PetriNet {
        self.net
    }

    /// 处理单个 impl 块, 将其中的方法映射为变迁
    ///
    /// impl 的接收者会记录在上下文, 以便后续把参数/返回中的 Self 替换成实际类型.
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

        let trait_method_lookup: Option<HashMap<String, Id>> = impl_block.trait_.as_ref().and_then(|trait_path| {
            self.crate_.index.get(&trait_path.id).and_then(|trait_item| {
                if let ItemEnum::Trait(trait_def) = &trait_item.inner {
                    let mut map = HashMap::new();
                    for method_id in &trait_def.items {
                        if let Some(item) = self.crate_.index.get(method_id) {
                            if let ItemEnum::Function(_) = &item.inner {
                                if let Some(name) = item.name.as_deref() {
                                    map.entry(name.to_string()).or_insert(*method_id);
                                }
                            }
                        }
                    }
                    Some(map)
                } else {
                    None
                }
            })
        });

        for item_id in &impl_block.items {
            if let Some(inner_item) = self.crate_.index.get(item_id) {
                if let ItemEnum::Function(func) = &inner_item.inner {
                    self.impl_function_ids.insert(inner_item.id);
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

        // Trait default methods instantiated for this impl.
        if let Some(method_lookup) = trait_method_lookup.as_ref() {
            for method_name in &impl_block.provided_trait_methods {
                if let Some(method_id) = method_lookup.get(method_name) {
                    if let Some(item) = self.crate_.index.get(method_id) {
                        if let ItemEnum::Function(func) = &item.inner {
                            self.impl_function_ids.insert(item.id);
                            self.ingest_function_with_context(
                                item,
                                func,
                                context.clone(),
                                impl_generics.clone(),
                                impl_where.clone(),
                                impl_trait_bounds.clone(),
                            );
                        }
                    }
                }
            }
        }

        // Methods referenced via `item` (impl item itself) are handled via the loop above.
        let _ = item;
    }

    fn ingest_function(&mut self, item: &Item, func: &Function, context: FunctionContext) {
        let context = if matches!(context, FunctionContext::FreeFunction) {
            self.infer_free_function_context(item).unwrap_or(context)
        } else {
            context
        };

        self.ingest_function_with_context(
            item,
            func,
            context,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }

    /// 参数 -> 输入 place, 返回值 -> 输出 place, 同时在摘要中保存泛型与 where 约束以供分析.
    fn ingest_function_with_context(
        &mut self,
        item: &Item,
        func: &Function,
        context: FunctionContext,
        impl_generics: Vec<String>,
        impl_where: Vec<String>,
        impl_trait_bounds: Vec<String>,
    ) {
        let receiver_descriptor = context_receiver_descriptor(&context);

        let mut summary_inputs = Vec::new();
        let mut input_arcs = Vec::new();
        for (name, ty) in &func.sig.inputs {
            let mut descriptor = TypeDescriptor::from_type(ty);
            if let Some(receiver) = receiver_descriptor {
                if let Some(replaced) = descriptor.replace_self(receiver) {
                    descriptor = replaced;
                }
            }
            let place_id = self.ensure_place(descriptor.clone());
            let parameter = ParameterSummary {
                name: (!name.is_empty()).then(|| Arc::<str>::from(name.as_str())),
                descriptor: descriptor.clone(),
            };
            summary_inputs.push(parameter.clone());
            input_arcs.push((place_id, ArcData {
                weight: 1,
                parameter: Some(parameter),
                kind: ArcKind::Normal,
                descriptor: None,
            }));
        }

        let mut output_descriptor = func
            .sig
            .output
            .as_ref()
            .map(|ty| TypeDescriptor::from_type(ty));

        if let (Some(receiver), Some(descriptor)) = (receiver_descriptor, output_descriptor.as_mut()) {
            if let Some(replaced) = descriptor.replace_self(receiver) {
                *descriptor = replaced;
            }
        }

        let mut output_arcs = Vec::new();
        if let Some(descriptor) = output_descriptor.clone() {
            let place_id = self.ensure_place(descriptor.clone());
            output_arcs.push((place_id, ArcData {
                weight: 1,
                parameter: None,
                kind: ArcKind::Normal,
                descriptor: Some(descriptor),
            }));
        }

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
            inputs: summary_inputs,
            output: output_descriptor,
        };

        let transition_id = self.net.add_transition(function_summary);

        for (place_id, arc_data) in input_arcs {
            self.net.add_input_arc_from_place(place_id, transition_id, arc_data);
        }

        for (place_id, arc_data) in output_arcs {
            self.net.add_output_arc_to_place(transition_id, place_id, arc_data);
        }
    }

    fn infer_free_function_context(&self, item: &Item) -> Option<FunctionContext> {
        let summary = self.crate_.paths.get(&item.id)?;
        if summary.path.len() < 2 {
            return None;
        }

        let owner_path = &summary.path[..summary.path.len() - 1];
        let owner_item = self.find_item_by_path(owner_path)?;

        let owner_path_str = owner_path.join("::");
        let path = RustdocPath {
            path: owner_path_str.clone(),
            id: owner_item.id,
            args: None,
        };
        let resolved_type = Type::ResolvedPath(path.clone());
        let descriptor = TypeDescriptor::from_type(&resolved_type);

        match &owner_item.inner {
            ItemEnum::Struct(_) | ItemEnum::Enum(_) | ItemEnum::Union(_) => {
                Some(FunctionContext::InherentMethod { receiver: descriptor })
            }
            ItemEnum::Trait(_) => Some(FunctionContext::TraitImplementation {
                receiver: descriptor,
                trait_path: Arc::<str>::from(TypeFormatter::path_to_string(&path)),
            }),
            _ => None,
        }
    }

    fn find_item_by_path(&self, path: &[String]) -> Option<&Item> {
        self.crate_.index.values().find(|candidate| {
            self.crate_
                .paths
                .get(&candidate.id)
                .map(|summary| paths_equal(&summary.path, path))
                .unwrap_or(false)
        })
    }

    fn ensure_place(&mut self, descriptor: TypeDescriptor) -> PlaceId {
        self.net.add_place(descriptor)
    }

    fn lookup_qualified_path(&self, item: &Item) -> Option<Arc<str>> {
        self.crate_
            .paths
            .get(&item.id)
            .map(|summary| Arc::<str>::from(summary.path.join("::")))
    }
}

fn context_receiver_descriptor(context: &FunctionContext) -> Option<&TypeDescriptor> {
    match context {
        FunctionContext::InherentMethod { receiver } => Some(receiver),
        FunctionContext::TraitImplementation { receiver, .. } => Some(receiver),
        FunctionContext::FreeFunction => None,
    }
}

fn paths_equal(lhs: &[String], rhs: &[String]) -> bool {
    lhs.len() == rhs.len() && lhs.iter().zip(rhs.iter()).all(|(a, b)| a == b)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn load_test_crate(name: &str) -> Crate {
        let path = format!("./localdata/test_data/{name}/rustdoc.json");
        let content =
            std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("failed to read {path}: {err}"));
        serde_json::from_str(&content)
            .unwrap_or_else(|err| panic!("failed to parse {path} as rustdoc JSON: {err}"))
    }

    #[test]
    fn replaces_self_in_method_receivers() {
        let crate_ = load_test_crate("method_self_receivers");
        let net = PetriNetBuilder::from_crate(&crate_);

        for (_place_id, place) in net.places() {
            assert!(
                !place.descriptor.display().contains("Self"),
                "unexpected Self in place {:?}",
                place
            );
        }

        for (_transition_id, transition) in net.transitions() {
            let receiver = context_receiver_descriptor(&transition.summary.context);

            if let Some(_) = receiver {
                for (_place_id, input) in net.transition_inputs(_transition_id) {
                    if let Some(param) = &input.parameter {
                        assert!(
                            !param.descriptor.display().contains("Self"),
                            "Self remained in {:?} of {:?}",
                            param,
                            transition.summary.name
                        );
                    }
                }

                if let Some(output) = &transition.summary.output {
                    assert!(
                        !output.display().contains("Self"),
                        "Self remained in output {:?} of {:?}",
                        output,
                        transition.summary.name
                    );
                }
            }
        }
    }
}

