use super::shim::ShimRegistry;
use super::structure::{EdgeData, EdgeKind, PetriNet, PlaceData, TransitionData, TransitionKind};
use crate::ir_graph::structure::{EdgeMode, IrGraph, OpKind, OpNode, TypeNode};
use rustdoc_types::Id;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

/// KnowledgeBase: Act as the Solver for generics and traits.
#[derive(Debug, Default)]
struct KnowledgeBase {
    /// Map Trait Name (e.g., "Read") -> List of Concrete Types that implement it
    trait_impls: HashMap<String, Vec<TypeNode>>,
    /// Cache for Id -> Trait Name lookup to avoid repeated lookups
    trait_id_to_name: HashMap<Id, String>,
}

impl KnowledgeBase {
    /// Build KnowledgeBase from IrGraph
    fn build(ir: &IrGraph) -> Self {
        let mut kb = KnowledgeBase::default();
        let parsed = ir.parsed_crate();

        // 1. Build Trait Id -> Name map
        for trait_info in &parsed.traits {
            kb.trait_id_to_name
                .insert(trait_info.id.clone(), trait_info.name.clone());
        }

        // 2. Populate trait_impls
        // ir.trait_impls is Map<TraitId, Vec<TypeId>>
        for (trait_id, type_ids) in &ir.trait_impls {
            if let Some(trait_name) = kb.trait_id_to_name.get(trait_id) {
                let types: Vec<TypeNode> = type_ids
                    .iter()
                    .map(|tid| {
                        // Create TypeNode::Struct/Enum/etc from Id
                        // Since we don't know the exact kind easily here without looking up,
                        // we can try to find it in type_nodes or construct a best guess.
                        // However, IrGraph already constructed TypeNodes.
                        // We need the TypeNode that corresponds to this Id.
                        // A simple way is to create a TypeNode::Struct(Some(id)) as a placeholder wrapper,
                        // or better, search existing type_nodes in IrGraph.
                        // But finding by ID in IrGraph type_nodes is O(N).
                        // Let's check ParsedCrate type index.
                        if let Some(item) = parsed.type_index.get(tid) {
                            use rustdoc_types::ItemEnum;
                            match &item.inner {
                                ItemEnum::Struct(_) => TypeNode::Struct(Some(tid.clone())),
                                ItemEnum::Enum(_) => TypeNode::Enum(Some(tid.clone())),
                                ItemEnum::Union(_) => TypeNode::Union(Some(tid.clone())),
                                // For primitives or others, it might be harder.
                                // But usually trait impls are on these items.
                                _ => TypeNode::Struct(Some(tid.clone())), // Fallback
                            }
                        } else {
                            // External type?
                            TypeNode::Struct(Some(tid.clone()))
                        }
                    })
                    .collect();

                kb.trait_impls
                    .entry(trait_name.clone())
                    .or_default()
                    .extend(types);
            }
        }

        kb
    }

    /// Find concrete types that satisfy all given trait bounds (by Trait Ids)
    fn find_satisfying_types(&self, trait_bounds: &[Id]) -> Vec<TypeNode> {
        if trait_bounds.is_empty() {
            return Vec::new();
        }

        // 1. Get lists of types for each trait
        let mut type_lists = Vec::new();
        for tid in trait_bounds {
            if let Some(name) = self.trait_id_to_name.get(tid) {
                if let Some(types) = self.trait_impls.get(name) {
                    type_lists.push(types);
                } else {
                    // One trait has no implementations -> intersection is empty
                    return Vec::new();
                }
            } else {
                // Unknown trait ID
                return Vec::new();
            }
        }

        // 2. Find intersection
        // Start with the first list
        let mut intersection: HashSet<&TypeNode> = type_lists[0].iter().collect();

        for list in &type_lists[1..] {
            let current_set: HashSet<&TypeNode> = list.iter().collect();
            intersection.retain(|t| current_set.contains(t));
        }

        intersection.into_iter().cloned().collect()
    }
}

/// Petri Net 构建器
pub struct PetriNetBuilder;

impl PetriNetBuilder {
    /// 从 IR Graph 构建 Petri Net
    pub fn from_ir(ir: &IrGraph) -> PetriNet {
        Self::from_ir_with_shims(ir, ShimRegistry::with_default_shims())
    }

    /// 从 IR Graph 构建 Petri Net，使用自定义的 Shim 注册表
    pub fn from_ir_with_shims(ir: &IrGraph, shim_registry: ShimRegistry) -> PetriNet {
        let mut net = PetriNet::new();
        let kb = KnowledgeBase::build(ir);

        let mut builder = BuilderContext {
            net: &mut net,
            ir,
            kb,
            copy_trait_id: Self::find_copy_trait(ir),
        };

        builder.build();

        // 使用新的抽象 Shim 机制
        log::info!("应用 Shim 填补机制...");
        shim_registry.apply_all(&mut net);

        net
    }

    fn find_copy_trait(ir: &IrGraph) -> Option<Id> {
        for trait_info in &ir.parsed_crate().traits {
            if trait_info.name == "Copy" || trait_info.name.ends_with("::Copy") {
                return Some(trait_info.id);
            }
        }
        None
    }
}

struct BuilderContext<'a> {
    net: &'a mut PetriNet,
    ir: &'a IrGraph,
    kb: KnowledgeBase,
    copy_trait_id: Option<Id>,
}

impl<'a> BuilderContext<'a> {
    fn build(&mut self) {
        // We iterate over operations and instantiate them.
        // Copy keys to avoid borrow checker issues if we needed to mutate self inside loop
        // (but we are mutating self.net, and reading self.ir/kb)
        let operations = &self.ir.operations;
        for op in operations {
            self.process_operation(op);
        }
    }

    fn process_operation(&mut self, op: &OpNode) {
        if op.is_generic() {
            self.expand_generic_operation(op);
        } else {
            // Concrete operation
            self.create_transition(op, &HashMap::new(), None);
        }
    }

    fn expand_generic_operation(&mut self, op: &OpNode) {
        // Simple Monomorphization:
        // We only handle single generic parameter for V1 to keep it simple,
        // or simple cartesian product if we want to be fancy.
        // Prompt says: "Loop through these concrete types."

        // 1. Identify Generic Params
        // op.generic_constraints: Name -> Vec<TraitId>
        // Let's assume one generic param R for now as per prompt example, or handle independent params.

        // Strategy: For each param, find candidates. Then Cartesian Product.
        let mut param_candidates: HashMap<String, Vec<TypeNode>> = HashMap::new();

        for (param_name, bounds) in &op.generic_constraints {
            let candidates = self.kb.find_satisfying_types(bounds);
            if candidates.is_empty() {
                // Cannot instantiate this generic op
                return;
            }
            param_candidates.insert(param_name.clone(), candidates);
        }

        // Cartesian Product
        // For V1, let's just handle up to 2 params hardcoded or recursion.
        // A generic recursive function to generate combinations.
        let keys: Vec<String> = param_candidates.keys().cloned().collect();
        let mut assignment = HashMap::new();
        self.generate_generic_combinations(op, &keys, 0, &mut assignment, &param_candidates);
    }

    fn generate_generic_combinations(
        &mut self,
        op: &OpNode,
        keys: &[String],
        idx: usize,
        assignment: &mut HashMap<String, TypeNode>,
        candidates_map: &HashMap<String, Vec<TypeNode>>,
    ) {
        if idx >= keys.len() {
            // Base case: assignment is complete
            self.create_transition(
                op,
                assignment,
                Some(self.mangle_generic_name(op.name.clone(), assignment)),
            );
            return;
        }

        let key = &keys[idx];
        if let Some(candidates) = candidates_map.get(key) {
            for cand in candidates {
                assignment.insert(key.clone(), cand.clone());
                self.generate_generic_combinations(op, keys, idx + 1, assignment, candidates_map);
            }
        }
    }

    fn mangle_generic_name(
        &self,
        base_name: String,
        assignment: &HashMap<String, TypeNode>,
    ) -> String {
        let mut suffix = String::new();
        // stable ordering
        let mut sorted_keys: Vec<_> = assignment.keys().collect();
        sorted_keys.sort();

        for key in sorted_keys {
            let ty_name = self.ir.get_type_name(&assignment[key]).unwrap_or("unknown");
            // Sanitize
            let clean_name = ty_name
                .replace("::", "_")
                .replace("<", "_")
                .replace(">", "");
            suffix.push_str(&format!("_{}", clean_name));
        }
        format!("{}{}", base_name, suffix)
    }

    fn create_transition(
        &mut self,
        op: &OpNode,
        generic_map: &HashMap<String, TypeNode>,
        custom_name: Option<String>,
    ) {
        // 1. Create Transition Data
        let trans_id = self.hash_id_with_context(&op.id, generic_map);
        let func_name = custom_name.unwrap_or_else(|| op.name.clone());

        let kind = match op.kind {
            OpKind::FnCall => TransitionKind::FnCall,
            OpKind::FieldAccessor { .. } => TransitionKind::FieldAccessor,
            OpKind::MethodCall { .. } => TransitionKind::MethodCall,
            OpKind::AssocFn { .. } => TransitionKind::AssocFn,
            OpKind::ConstantAlias { .. } => TransitionKind::ConstantAlias,
            OpKind::StaticAlias { .. } => TransitionKind::StaticAlias,
        };

        // Convert generic_map to String->String for serialization/display
        let generic_map_str: HashMap<String, String> = generic_map
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    self.ir.get_type_name(v).unwrap_or("?").to_string(),
                )
            })
            .collect();

        let trans_data = TransitionData {
            id: trans_id,
            func_name,
            kind,
            generic_map: generic_map_str,
        };

        let trans_idx = self.net.add_transition(trans_data);

        // 2. Handle Inputs
        for (idx, input_edge) in op.inputs.iter().enumerate() {
            let concrete_type = self.substitute_type(&input_edge.type_node, generic_map);

            // Skip GenericParams that weren't resolved (shouldn't happen if monomorphized correctly,
            // unless logic error or unused param)
            if let TypeNode::GenericParam { .. } = concrete_type {
                // Warn?
                continue;
            }

            let place_data = self.create_place_data(&concrete_type);
            let place_idx = self.net.add_place(place_data);

            let edge_data = self.convert_edge_mode(input_edge.mode, idx);
            self.net.connect(place_idx, trans_idx, edge_data);
        }

        // 3. Handle Output
        if let Some(output_edge) = &op.output {
            let concrete_type = self.substitute_type(&output_edge.type_node, generic_map);
            if let TypeNode::GenericParam { .. } = concrete_type {
            } else {
                let place_data = self.create_place_data(&concrete_type);
                let place_idx = self.net.add_place(place_data);
                let edge_data = self.convert_edge_mode(output_edge.mode, 0);
                self.net.connect(trans_idx, place_idx, edge_data);
            }
        }

        // 4. Handle Error Output
        if let Some(error_edge) = &op.error_output {
            let concrete_type = self.substitute_type(&error_edge.type_node, generic_map);
            if let TypeNode::GenericParam { .. } = concrete_type {
            } else {
                let place_data = self.create_place_data(&concrete_type);
                let place_idx = self.net.add_place(place_data);
                let edge_data = self.convert_edge_mode(error_edge.mode, 1);
                self.net.connect(trans_idx, place_idx, edge_data);
            }
        }
    }

    /// Recursively substitute generic params in TypeNode
    fn substitute_type(&self, node: &TypeNode, map: &HashMap<String, TypeNode>) -> TypeNode {
        match node {
            TypeNode::GenericParam { name, .. } => {
                if let Some(concrete) = map.get(name) {
                    concrete.clone()
                } else {
                    node.clone()
                }
            }
            TypeNode::Tuple(elems) => {
                TypeNode::Tuple(elems.iter().map(|e| self.substitute_type(e, map)).collect())
            }
            TypeNode::Array(inner) => TypeNode::Array(Box::new(self.substitute_type(inner, map))),
            TypeNode::QualifiedPath {
                parent,
                name,
                trait_id,
            } => TypeNode::QualifiedPath {
                parent: Box::new(self.substitute_type(parent, map)),
                name: name.clone(),
                trait_id: trait_id.clone(),
            },
            TypeNode::GenericInstance {
                base_id,
                path,
                type_args,
            } => TypeNode::GenericInstance {
                base_id: base_id.clone(),
                path: path.clone(),
                type_args: type_args
                    .iter()
                    .map(|a| self.substitute_type(a, map))
                    .collect(),
            },
            // Other types usually don't contain children to recurse or are primitive
            _ => node.clone(),
        }
    }

    fn create_place_data(&self, type_node: &TypeNode) -> PlaceData {
        let id = self.hash_type(type_node);
        // We construct name manually if it's a substituted type because IR might not know it
        // Or we rely on ir.get_type_name for base and build up for complex.
        // For simplicity, we try ir.get_type_name first, if None, we reconstruct.
        let type_name = if let Some(name) = self.ir.get_type_name(type_node) {
            name.to_string()
        } else {
            // Reconstruct name for synthesized types (e.g. Vec<File>)
            match type_node {
                TypeNode::GenericInstance {
                    path, type_args, ..
                } => {
                    let base = path.split("::").last().unwrap_or(path);
                    let args: Vec<String> = type_args
                        .iter()
                        .map(|a| self.create_place_data(a).type_name)
                        .collect();
                    format!("{}<{}>", base, args.join(", "))
                }
                TypeNode::Struct(Some(id)) => format!("Struct_{:?}", id), // Fallback
                _ => "UnknownGenerated".to_string(),
            }
        };

        let is_source = matches!(type_node, TypeNode::Primitive(_));
        let is_copy = self.check_is_copy(type_node);

        // 获取完整路径
        let resolved_path = self.ir.get_type_path(type_node).map(|s| s.to_string());

        PlaceData {
            id,
            type_name,
            resolved_path,
            is_source,
            is_copy,
        }
    }

    // Reuse existing helpers
    fn check_is_copy(&self, type_node: &TypeNode) -> bool {
        if let TypeNode::Primitive(name) = type_node {
            match name.as_str() {
                "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64"
                | "i128" | "isize" | "f32" | "f64" | "bool" | "char" => return true,
                _ => {}
            }
        }
        if let Some(copy_id) = self.copy_trait_id {
            let type_id = match type_node {
                TypeNode::Struct(Some(id))
                | TypeNode::Enum(Some(id))
                | TypeNode::Union(Some(id)) => Some(id),
                _ => None,
            };
            if let Some(tid) = type_id {
                return self.ir.implements_trait(tid, &copy_id);
            }
        }
        false
    }

    fn convert_edge_mode(&self, mode: EdgeMode, index: usize) -> EdgeData {
        let (kind, is_raw_ptr) = match mode {
            EdgeMode::Move => (EdgeKind::Move, false),
            EdgeMode::Ref => (EdgeKind::Ref, false),
            EdgeMode::MutRef => (EdgeKind::MutRef, false),
            EdgeMode::RawPtr => (EdgeKind::Ref, true),
            EdgeMode::MutRawPtr => (EdgeKind::MutRef, true),
        };
        EdgeData {
            kind,
            index,
            is_raw_ptr,
        }
    }

    fn hash_type(&self, type_node: &TypeNode) -> u64 {
        let mut hasher = DefaultHasher::new();
        type_node.hash(&mut hasher);
        hasher.finish()
    }

    fn hash_id_with_context(&self, id: &Id, context: &HashMap<String, TypeNode>) -> u64 {
        let mut hasher = DefaultHasher::new();
        id.hash(&mut hasher);
        // also hash context
        for (k, v) in context {
            k.hash(&mut hasher);
            v.hash(&mut hasher);
        }
        hasher.finish()
    }
}
