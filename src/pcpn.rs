//! PCPN (Pushdown Colored Petri Net) - 9-Place 模型
//!
//! 对每个基础类型 T,区分 {T, &T, &mut T} × {own, frz, blk} = 9 个 place.

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::apigraph::{ApiGraph, FunctionNode, OwnershipType};
use crate::type_model::TypeKey;
use crate::types::{Capability, Place, PlaceId, PlaceKey, TransitionId, TyGround, TypeForm};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transition {
    pub id: TransitionId,
    pub name: String,
    pub kind: TransitionKind,
    pub input_arcs: Vec<Arc>,
    pub output_arcs: Vec<Arc>,
    pub guards: Vec<Guard>,
    pub is_const_producer: bool,
    /// 生命周期绑定信息:返回引用的 API 调用会携带此信息,
    /// 用于仿真时确定借用来源(source_param_index → 对应 input_arcs 中的位置)
    #[serde(default)]
    pub lifetime_bindings: Vec<LifetimeBindingInfo>,
}

/// 用于 PCPN Transition 的生命周期绑定信息
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LifetimeBindingInfo {
    /// 返回引用绑定到的输入参数索引(在 input_arcs 中的位置)
    pub source_arc_index: usize,
    /// 是否是共享引用
    pub is_shared: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionKind {
    ApiCall { fn_id: usize, fn_path: String },
    CreatePrimitive { ty: TyGround },
    ConstProducer { ty: TyGround, fn_path: String },
    BorrowShrFirst { base_type: TyGround },
    BorrowShrNext { base_type: TyGround },
    BorrowMut { base_type: TyGround },
    EndBorrowShrKeepFrz { base_type: TyGround },
    EndBorrowShrUnfreeze { base_type: TyGround },
    EndBorrowMut { base_type: TyGround },
    Drop { ty: TyGround, form: TypeForm },
    CopyUse { ty: TyGround },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Arc {
    pub place_id: PlaceId,
    #[serde(default)]
    pub consumes: bool,
    #[serde(default)]
    pub annotation: Option<ArcAnnotation>,
    /// Optional inscription for CPN token transformation on this arc
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inscription: Option<ArcInscription>,
}

impl Arc {
    pub fn new(place_id: PlaceId, consumes: bool, annotation: Option<ArcAnnotation>) -> Self {
        Arc {
            place_id,
            consumes,
            annotation,
            inscription: None,
        }
    }

    pub fn with_inscription(mut self, inscription: ArcInscription) -> Self {
        self.inscription = Some(inscription);
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ArcAnnotation {
    Param { index: usize, name: String },
    SelfParam,
    Return,
    ReturnArc,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Guard {
    pub kind: GuardKind,
    pub base_type: TyGround,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardKind {
    NoFrzNoBlk,
    NoBlk,
    NoFrzNoOtherBlk,
    NotBlocked,
    StackTopMatches,
    /// Token count in a specific place must be within [min, max]
    PlaceCountRange {
        form: TypeForm,
        cap: Capability,
        min: usize,
        max: usize,
    },
    /// Stack depth must not exceed a threshold
    StackDepthMax { max_depth: usize },
    /// Conjunction of multiple guard conditions (all must hold)
    And(Vec<GuardKind>),
}

/// Arc inscription defines how tokens are transformed when flowing through an arc.
/// Standard CPN uses inscription functions to map input tokens to output tokens.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ArcInscription {
    /// Identity: token passes through unchanged (default behavior)
    Identity,
    /// Project: extract a field/component from the token type
    Project { field: String },
    /// Wrap: wrap the token in a container type (e.g., T -> Option<T>)
    Wrap { wrapper_type: TyGround },
    /// Filter: only tokens matching a type predicate can flow
    Filter { expected_type: TyGround },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pcpn {
    pub places: Vec<Place>,
    pub transitions: Vec<Transition>,
    #[serde(skip)]
    pub place_index: HashMap<PlaceKey, PlaceId>,
    pub type_universe: IndexSet<TyGround>,
}

impl Default for Pcpn {
    fn default() -> Self {
        Self::new()
    }
}

impl Pcpn {
    pub fn new() -> Self {
        Pcpn {
            places: Vec::new(),
            transitions: Vec::new(),
            place_index: HashMap::new(),
            type_universe: IndexSet::new(),
        }
    }

    pub fn from_api_graph(graph: &ApiGraph) -> Self {
        let mut pcpn = Pcpn::new();

        for type_node in &graph.type_nodes {
            if let Some(ty) = pcpn.convert_type_key(&type_node.type_key) {
                pcpn.type_universe.insert(ty);
            }
        }

        let monomorphized = pcpn.monomorphize_functions(graph);

        for mono_fn in &monomorphized {
            pcpn.collect_fn_types(graph, mono_fn);
        }

        for ty in pcpn.type_universe.clone() {
            pcpn.create_9_places_for_type(&ty, 3);
        }

        for mono_fn in &monomorphized {
            pcpn.create_api_transition(graph, mono_fn);
        }

        for ty in pcpn.type_universe.clone() {
            pcpn.create_structural_transitions(&ty);
        }

        pcpn
    }

    fn convert_type_key(&self, tk: &TypeKey) -> Option<TyGround> {
        match tk {
            TypeKey::Primitive(s) => Some(TyGround::primitive(s)),
            TypeKey::Path { crate_path, args } => {
                let converted_args: Vec<TyGround> = args
                    .iter()
                    .filter_map(|a| self.convert_type_key(a))
                    .collect();
                Some(TyGround::path_with_args(crate_path, converted_args))
            }
            TypeKey::Tuple(elems) => {
                let converted: Vec<TyGround> = elems
                    .iter()
                    .filter_map(|e| self.convert_type_key(e))
                    .collect();
                // Use TyGround::tuple() to normalize empty tuples to Unit
                Some(TyGround::tuple(converted))
            }
            TypeKey::Slice(inner) => {
                // 切片类型转为带泛型参数的 Path
                let inner_ty = self.convert_type_key(inner)?;
                Some(TyGround::path_with_args("[T]", vec![inner_ty]))
            }
            TypeKey::Array { elem, len } => {
                let elem_ty = self.convert_type_key(elem)?;
                Some(TyGround::path_with_args(
                    &format!("[T;{}]", len),
                    vec![elem_ty],
                ))
            }
            TypeKey::RefShr(inner) | TypeKey::RefMut(inner) => self.convert_type_key(inner),
            TypeKey::AssociatedType(path) => {
                // 将关联类型视为不透明路径类型
                Some(TyGround::path(path))
            }
            TypeKey::FnPtr { .. } | TypeKey::RawPtr { .. } => {
                // 函数指针和裸指针暂不参与 PCPN 模型
                None
            }
            TypeKey::GenericParam { .. } => None,
            TypeKey::Unknown(_) => None,
        }
    }

    fn create_9_places_for_type(&mut self, base_type: &TyGround, budget: usize) {
        for form in [TypeForm::Value, TypeForm::RefShr, TypeForm::RefMut] {
            for cap in [Capability::Own, Capability::Frz, Capability::Blk] {
                let key = PlaceKey::new(base_type.clone(), form.clone(), cap);
                if self.place_index.contains_key(&key) {
                    continue;
                }
                let id = self.places.len();
                self.places.push(Place {
                    id,
                    base_type: base_type.clone(),
                    form: form.clone(),
                    cap,
                    budget,
                });
                self.place_index.insert(key, id);
            }
        }
    }

    pub fn get_place(
        &self,
        base_type: &TyGround,
        form: &TypeForm,
        cap: Capability,
    ) -> Option<PlaceId> {
        let key = PlaceKey::new(base_type.clone(), form.clone(), cap);
        self.place_index.get(&key).copied()
    }

    fn get_or_create_place(
        &mut self,
        base_type: &TyGround,
        form: &TypeForm,
        cap: Capability,
        budget: usize,
    ) -> PlaceId {
        let key = PlaceKey::new(base_type.clone(), form.clone(), cap);
        if let Some(&id) = self.place_index.get(&key) {
            return id;
        }
        let id = self.places.len();
        self.places.push(Place {
            id,
            base_type: base_type.clone(),
            form: form.clone(),
            cap,
            budget,
        });
        self.place_index.insert(key, id);
        id
    }

    fn monomorphize_functions(&self, graph: &ApiGraph) -> Vec<MonoFn> {
        let mut result = Vec::new();
        for fn_node in &graph.fn_nodes {
            let generics = self.collect_fn_generics(graph, fn_node);
            if generics.is_empty() {
                result.push(MonoFn {
                    fn_id: fn_node.id,
                    name: fn_node.path.clone(),
                    subst: HashMap::new(),
                });
            } else {
                let instantiations = self.enumerate_instantiations(&generics);
                for subst in instantiations {
                    let mono_name = self.build_mono_name(&fn_node.path, &subst);
                    result.push(MonoFn {
                        fn_id: fn_node.id,
                        name: mono_name,
                        subst,
                    });
                }
            }
        }
        result
    }

    fn collect_fn_generics(
        &self,
        graph: &ApiGraph,
        fn_node: &FunctionNode,
    ) -> Vec<(String, String, Vec<String>)> {
        let mut params = Vec::new();
        for edge in graph.get_input_edges(fn_node.id) {
            let ty = &graph.type_nodes[edge.type_node].type_key;
            for (ctx, name, bounds) in ty.collect_generic_params() {
                let key = (ctx.clone(), name.clone(), bounds.clone());
                if !params.contains(&key) {
                    params.push(key);
                }
            }
        }
        for edge in graph.get_output_edges(fn_node.id) {
            let ty = &graph.type_nodes[edge.type_node].type_key;
            for (ctx, name, bounds) in ty.collect_generic_params() {
                let key = (ctx.clone(), name.clone(), bounds.clone());
                if !params.contains(&key) {
                    params.push(key);
                }
            }
        }
        params
    }

    fn enumerate_instantiations(
        &self,
        params: &[(String, String, Vec<String>)],
    ) -> Vec<HashMap<(String, String), TyGround>> {
        if params.is_empty() {
            return vec![HashMap::new()];
        }

        let mut candidates: Vec<Vec<(&(String, String, Vec<String>), &TyGround)>> = Vec::new();
        for param in params {
            let (_, _, bounds) = param;
            let mut matching: Vec<(&(String, String, Vec<String>), &TyGround)> = Vec::new();
            for ty in &self.type_universe {
                if bounds.is_empty() || self.satisfies_bounds(ty, bounds) {
                    matching.push((param, ty));
                }
            }
            if matching.is_empty() {
                return Vec::new();
            }
            candidates.push(matching);
        }

        self.cartesian_product(&candidates)
    }

    fn satisfies_bounds(&self, ty: &TyGround, bounds: &[String]) -> bool {
        for bound in bounds {
            let ok = match bound.as_str() {
                // 精确匹配 trait bound(区分大小写,符合 Rust 惯例)
                "Copy" | "copy" => ty.is_copy(),
                "Clone" | "clone" => {
                    // Copy 蕴含 Clone
                    ty.is_copy() || !ty.is_primitive()
                }
                "Default" | "default" => {
                    // 原始类型和 Unit 都有 Default
                    ty.is_primitive() || matches!(ty, TyGround::Unit)
                }
                "Send" | "Sync" | "Sized" | "Unpin" => {
                    // 大多数类型实现这些 auto traits
                    true
                }
                "Debug" | "Display" => {
                    // 保守地假设原始类型实现
                    ty.is_primitive()
                }
                "PartialEq" | "Eq" | "PartialOrd" | "Ord" | "Hash" => {
                    // 原始类型都实现这些
                    ty.is_primitive()
                }
                _ => {
                    // 未知 bound:保守接受(允许单态化尝试)
                    true
                }
            };
            if !ok {
                return false;
            }
        }
        true
    }

    fn cartesian_product(
        &self,
        candidates: &[Vec<(&(String, String, Vec<String>), &TyGround)>],
    ) -> Vec<HashMap<(String, String), TyGround>> {
        if candidates.is_empty() {
            return vec![HashMap::new()];
        }

        // 限制组合爆炸:总实例化数量不超过阈值
        const MAX_INSTANTIATIONS: usize = 64;

        let first = &candidates[0];
        let rest = &candidates[1..];
        let rest_products = self.cartesian_product(rest);
        let mut result = Vec::new();
        for (param, ty) in first {
            let (ctx, name, _) = param;
            for rest_map in &rest_products {
                let mut new_map = rest_map.clone();
                new_map.insert((ctx.clone(), name.clone()), (*ty).clone());
                result.push(new_map);
                if result.len() >= MAX_INSTANTIATIONS {
                    return result;
                }
            }
        }
        result
    }

    fn build_mono_name(&self, path: &str, subst: &HashMap<(String, String), TyGround>) -> String {
        if subst.is_empty() {
            return path.to_string();
        }
        let mut args: Vec<String> = subst.values().map(|ty| ty.short_name()).collect();
        args.sort();
        format!("{}<{}>", path, args.join(", "))
    }

    fn collect_fn_types(&mut self, graph: &ApiGraph, mono_fn: &MonoFn) {
        let fn_node = &graph.fn_nodes[mono_fn.fn_id];
        for edge in graph.get_input_edges(fn_node.id) {
            let tk = &graph.type_nodes[edge.type_node].type_key;
            if let Some(ty) = self.apply_subst_to_type_key(tk, &mono_fn.subst) {
                self.type_universe.insert(ty);
            }
        }
        for edge in graph.get_output_edges(fn_node.id) {
            let tk = &graph.type_nodes[edge.type_node].type_key;
            if let Some(ty) = self.apply_subst_to_type_key(tk, &mono_fn.subst) {
                self.type_universe.insert(ty);
            }
        }
    }

    fn apply_subst_to_type_key(
        &self,
        tk: &TypeKey,
        subst: &HashMap<(String, String), TyGround>,
    ) -> Option<TyGround> {
        let substituted = tk.substitute(
            &subst
                .iter()
                .map(|(k, v)| (k.clone(), self.ty_ground_to_type_key(v)))
                .collect(),
        );
        self.convert_type_key(&substituted)
    }

    fn ty_ground_to_type_key(&self, ty: &TyGround) -> TypeKey {
        match ty {
            TyGround::Primitive(s) => TypeKey::Primitive(s.clone()),
            TyGround::Path { name, args } => TypeKey::Path {
                crate_path: name.clone(),
                args: args.iter().map(|a| self.ty_ground_to_type_key(a)).collect(),
            },
            TyGround::Tuple(elems) => TypeKey::Tuple(
                elems
                    .iter()
                    .map(|e| self.ty_ground_to_type_key(e))
                    .collect(),
            ),
            // Both Unit and empty Tuple map to empty TypeKey::Tuple
            TyGround::Unit => TypeKey::Tuple(vec![]),
        }
    }

    fn create_api_transition(&mut self, graph: &ApiGraph, mono_fn: &MonoFn) {
        let fn_node = &graph.fn_nodes[mono_fn.fn_id];
        let trans_id = self.transitions.len();
        let mut input_arcs = Vec::new();
        let mut output_arcs = Vec::new();
        let mut guards = Vec::new();

        let is_const_producer = fn_node.is_entry && graph.get_input_edges(fn_node.id).is_empty();

        for (idx, edge) in graph.get_input_edges(fn_node.id).iter().enumerate() {
            let tk = &graph.type_nodes[edge.type_node].type_key;
            let base_ty = match self.apply_subst_to_type_key(tk, &mono_fn.subst) {
                Some(t) => t,
                None => continue,
            };

            let (form, cap, consumes) = match edge.ownership {
                OwnershipType::Own => (TypeForm::Value, Capability::Own, true),
                OwnershipType::Shr => (TypeForm::RefShr, Capability::Own, false),
                OwnershipType::Mut => (TypeForm::RefMut, Capability::Own, false),
            };

            let place_id = self.get_or_create_place(&base_ty, &form, cap, 3);
            let annotation = edge.param_index.map(|pi| {
                if fn_node.self_param.is_some() && pi == 0 {
                    ArcAnnotation::SelfParam
                } else {
                    ArcAnnotation::Param {
                        index: idx,
                        name: format!("arg{}", idx),
                    }
                }
            });

            input_arcs.push(Arc::new(place_id, consumes, annotation));

            if base_ty.is_copy() && form == TypeForm::Value {
                output_arcs.push(Arc::new(place_id, false, Some(ArcAnnotation::ReturnArc)));
            }

            match edge.ownership {
                OwnershipType::Own => {
                    guards.push(Guard {
                        kind: GuardKind::NoFrzNoBlk,
                        base_type: base_ty.clone(),
                    });
                }
                OwnershipType::Shr => {
                    guards.push(Guard {
                        kind: GuardKind::NoBlk,
                        base_type: base_ty.clone(),
                    });
                }
                OwnershipType::Mut => {
                    guards.push(Guard {
                        kind: GuardKind::NoFrzNoOtherBlk,
                        base_type: base_ty.clone(),
                    });
                }
            }
        }

        for edge in graph.get_output_edges(fn_node.id) {
            let tk = &graph.type_nodes[edge.type_node].type_key;
            let base_ty = match self.apply_subst_to_type_key(tk, &mono_fn.subst) {
                Some(t) => t,
                None => continue,
            };

            let (form, cap) = match edge.ownership {
                OwnershipType::Own => (TypeForm::Value, Capability::Own),
                OwnershipType::Shr => (TypeForm::RefShr, Capability::Own),
                OwnershipType::Mut => (TypeForm::RefMut, Capability::Own),
            };

            let place_id = self.get_or_create_place(&base_ty, &form, cap, 3);
            output_arcs.push(Arc::new(place_id, false, Some(ArcAnnotation::Return)));
        }

        if input_arcs.is_empty() && output_arcs.is_empty() {
            return;
        }

        let kind = if is_const_producer && input_arcs.is_empty() {
            if let Some(out_arc) = output_arcs.first() {
                let place = &self.places[out_arc.place_id];
                TransitionKind::ConstProducer {
                    ty: place.base_type.clone(),
                    fn_path: mono_fn.name.clone(),
                }
            } else {
                TransitionKind::ApiCall {
                    fn_id: mono_fn.fn_id,
                    fn_path: mono_fn.name.clone(),
                }
            }
        } else {
            TransitionKind::ApiCall {
                fn_id: mono_fn.fn_id,
                fn_path: mono_fn.name.clone(),
            }
        };

        // 提取生命周期绑定信息
        let lifetime_bindings = fn_node
            .lifetime_bindings
            .iter()
            .filter_map(|lb| {
                // source_param_index 指的是 API 签名中的参数位置
                // 需要映射到 input_arcs 中的实际索引
                // input_arcs 的顺序跟随 get_input_edges,索引与 param_index 对应
                let arc_idx = input_arcs.iter().position(|arc| {
                    arc.annotation
                        .as_ref()
                        .map(|ann| match ann {
                            ArcAnnotation::SelfParam => lb.source_param_index == 0,
                            ArcAnnotation::Param { index, .. } => {
                                *index == lb.source_param_index
                                    || (fn_node.self_param.is_some()
                                        && *index + 1 == lb.source_param_index)
                            }
                            _ => false,
                        })
                        .unwrap_or(false)
                });
                arc_idx.map(|idx| LifetimeBindingInfo {
                    source_arc_index: idx,
                    is_shared: lb.is_shared,
                })
            })
            .collect();

        self.transitions.push(Transition {
            id: trans_id,
            name: mono_fn.name.clone(),
            kind,
            input_arcs,
            output_arcs,
            guards,
            is_const_producer,
            lifetime_bindings,
        });
    }

    fn create_structural_transitions(&mut self, base_type: &TyGround) {
        let short = base_type.short_name();

        let own_val = self.get_or_create_place(base_type, &TypeForm::Value, Capability::Own, 3);
        let frz_val = self.get_or_create_place(base_type, &TypeForm::Value, Capability::Frz, 3);
        let blk_val = self.get_or_create_place(base_type, &TypeForm::Value, Capability::Blk, 3);
        let own_shr = self.get_or_create_place(base_type, &TypeForm::RefShr, Capability::Own, 3);
        let own_mut = self.get_or_create_place(base_type, &TypeForm::RefMut, Capability::Own, 3);

        self.add_transition(
            format!("borrow_shr_first({})", short),
            TransitionKind::BorrowShrFirst {
                base_type: base_type.clone(),
            },
            vec![Arc::new(own_val, true, None)],
            vec![
                Arc::new(frz_val, false, None),
                Arc::new(own_shr, false, None),
            ],
            vec![Guard {
                kind: GuardKind::NoBlk,
                base_type: base_type.clone(),
            }],
        );

        self.add_transition(
            format!("borrow_shr_next({})", short),
            TransitionKind::BorrowShrNext {
                base_type: base_type.clone(),
            },
            vec![Arc::new(frz_val, false, None)],
            vec![Arc::new(own_shr, false, None)],
            vec![],
        );

        self.add_transition(
            format!("end_shr_keep_frz({})", short),
            TransitionKind::EndBorrowShrKeepFrz {
                base_type: base_type.clone(),
            },
            vec![
                Arc::new(frz_val, false, None),
                Arc::new(own_shr, true, None),
            ],
            vec![Arc::new(frz_val, false, None)],
            vec![Guard {
                kind: GuardKind::StackTopMatches,
                base_type: base_type.clone(),
            }],
        );

        self.add_transition(
            format!("end_shr_unfreeze({})", short),
            TransitionKind::EndBorrowShrUnfreeze {
                base_type: base_type.clone(),
            },
            vec![
                Arc::new(frz_val, true, None),
                Arc::new(own_shr, true, None),
            ],
            vec![Arc::new(own_val, false, None)],
            vec![Guard {
                kind: GuardKind::StackTopMatches,
                base_type: base_type.clone(),
            }],
        );

        self.add_transition(
            format!("borrow_mut({})", short),
            TransitionKind::BorrowMut {
                base_type: base_type.clone(),
            },
            vec![Arc::new(own_val, true, None)],
            vec![
                Arc::new(blk_val, false, None),
                Arc::new(own_mut, false, None),
            ],
            vec![Guard {
                kind: GuardKind::NoFrzNoBlk,
                base_type: base_type.clone(),
            }],
        );

        self.add_transition(
            format!("end_mut({})", short),
            TransitionKind::EndBorrowMut {
                base_type: base_type.clone(),
            },
            vec![
                Arc::new(blk_val, true, None),
                Arc::new(own_mut, true, None),
            ],
            vec![Arc::new(own_val, false, None)],
            vec![Guard {
                kind: GuardKind::StackTopMatches,
                base_type: base_type.clone(),
            }],
        );

        self.add_transition(
            format!("drop_val({})", short),
            TransitionKind::Drop {
                ty: base_type.clone(),
                form: TypeForm::Value,
            },
            vec![Arc::new(own_val, true, None)],
            vec![],
            vec![Guard {
                kind: GuardKind::NotBlocked,
                base_type: base_type.clone(),
            }],
        );

        self.add_transition(
            format!("drop_shr({})", short),
            TransitionKind::Drop {
                ty: base_type.clone(),
                form: TypeForm::RefShr,
            },
            vec![Arc::new(own_shr, true, None)],
            vec![],
            vec![],
        );

        self.add_transition(
            format!("drop_mut({})", short),
            TransitionKind::Drop {
                ty: base_type.clone(),
                form: TypeForm::RefMut,
            },
            vec![Arc::new(own_mut, true, None)],
            vec![],
            vec![],
        );

        if base_type.is_primitive() {
            self.add_transition(
                format!("const_{}", short),
                TransitionKind::CreatePrimitive {
                    ty: base_type.clone(),
                },
                vec![],
                vec![Arc::new(own_val, false, None)],
                vec![],
            );
        }

        if base_type.is_copy() {
            self.add_transition(
                format!("copy_use({})", short),
                TransitionKind::CopyUse {
                    ty: base_type.clone(),
                },
                vec![Arc::new(own_val, false, None)],
                vec![Arc::new(own_val, false, None)],
                vec![],
            );
        }
    }

    fn add_transition(
        &mut self,
        name: String,
        kind: TransitionKind,
        input_arcs: Vec<Arc>,
        output_arcs: Vec<Arc>,
        guards: Vec<Guard>,
    ) {
        let id = self.transitions.len();
        let is_const = matches!(
            kind,
            TransitionKind::CreatePrimitive { .. } | TransitionKind::ConstProducer { .. }
        );
        self.transitions.push(Transition {
            id,
            name,
            kind,
            input_arcs,
            output_arcs,
            guards,
            is_const_producer: is_const,
            lifetime_bindings: vec![],
        });
    }

    pub fn to_dot(&self) -> String {
        let mut dot = String::new();
        dot.push_str("digraph PCPN {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  fontname=\"Helvetica\";\n");
        dot.push_str("  node [fontname=\"Helvetica\"];\n");
        dot.push_str("  edge [fontname=\"Helvetica\"];\n\n");

        for place in &self.places {
            let color = match (&place.form, &place.cap) {
                (TypeForm::Value, Capability::Own) => "lightblue",
                (TypeForm::Value, Capability::Frz) => "lightyellow",
                (TypeForm::Value, Capability::Blk) => "mistyrose",
                (TypeForm::RefShr, Capability::Own) => "lightcyan",
                (TypeForm::RefMut, Capability::Own) => "lavenderblush",
                _ => "white",
            };
            let label = place.key().display_name();
            dot.push_str(&format!(
                "  p{} [label=\"{}\\nB={}\", shape=circle, style=filled, fillcolor={}];\n",
                place.id, label, place.budget, color
            ));
        }
        dot.push_str("\n");

        for trans in &self.transitions {
            let color = match &trans.kind {
                TransitionKind::ApiCall { .. } => "palegreen",
                TransitionKind::ConstProducer { .. } | TransitionKind::CreatePrimitive { .. } => {
                    "lightcyan"
                }
                TransitionKind::BorrowShrFirst { .. }
                | TransitionKind::BorrowShrNext { .. }
                | TransitionKind::BorrowMut { .. } => "lavender",
                TransitionKind::EndBorrowShrKeepFrz { .. }
                | TransitionKind::EndBorrowShrUnfreeze { .. }
                | TransitionKind::EndBorrowMut { .. } => "honeydew",
                TransitionKind::Drop { .. } => "lightgray",
                TransitionKind::CopyUse { .. } => "wheat",
            };
            let label = if trans.guards.is_empty() {
                trans.name.clone()
            } else {
                format!("{}\\n[G:{}]", trans.name, trans.guards.len())
            };
            dot.push_str(&format!(
                "  t{} [label=\"{}\", shape=box, style=filled, fillcolor={}];\n",
                trans.id, label, color
            ));
        }
        dot.push_str("\n");

        for trans in &self.transitions {
            for arc in &trans.input_arcs {
                let style = if arc.consumes { "solid" } else { "dashed" };
                dot.push_str(&format!(
                    "  p{} -> t{} [style={}];\n",
                    arc.place_id, trans.id, style
                ));
            }
            for arc in &trans.output_arcs {
                dot.push_str(&format!("  t{} -> p{};\n", trans.id, arc.place_id));
            }
        }

        dot.push_str("}\n");
        dot
    }

    pub fn stats(&self) -> PcpnStats {
        let api_trans = self
            .transitions
            .iter()
            .filter(|t| matches!(t.kind, TransitionKind::ApiCall { .. }))
            .count();
        let const_trans = self
            .transitions
            .iter()
            .filter(|t| {
                matches!(
                    t.kind,
                    TransitionKind::CreatePrimitive { .. } | TransitionKind::ConstProducer { .. }
                )
            })
            .count();
        PcpnStats {
            num_places: self.places.len(),
            num_types: self.type_universe.len(),
            num_transitions: self.transitions.len(),
            num_api_transitions: api_trans,
            num_const_transitions: const_trans,
            num_structural_transitions: self.transitions.len() - api_trans - const_trans,
        }
    }
}

#[derive(Clone, Debug)]
struct MonoFn {
    fn_id: usize,
    name: String,
    subst: HashMap<(String, String), TyGround>,
}

#[derive(Debug)]
pub struct PcpnStats {
    pub num_places: usize,
    pub num_types: usize,
    pub num_transitions: usize,
    pub num_api_transitions: usize,
    pub num_const_transitions: usize,
    pub num_structural_transitions: usize,
}

impl std::fmt::Display for PcpnStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PCPN: {} places ({} types × 9), {} transitions ({} API, {} const, {} structural)",
            self.num_places,
            self.num_types,
            self.num_transitions,
            self.num_api_transitions,
            self.num_const_transitions,
            self.num_structural_transitions
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apigraph::build_counter_api_graph;

    #[test]
    fn test_9_places_created() {
        let mut pcpn = Pcpn::new();
        let ty = TyGround::path("Counter");
        pcpn.create_9_places_for_type(&ty, 3);

        assert_eq!(pcpn.places.len(), 9);

        assert!(
            pcpn.get_place(&ty, &TypeForm::Value, Capability::Own)
                .is_some()
        );
        assert!(
            pcpn.get_place(&ty, &TypeForm::Value, Capability::Frz)
                .is_some()
        );
        assert!(
            pcpn.get_place(&ty, &TypeForm::Value, Capability::Blk)
                .is_some()
        );
        assert!(
            pcpn.get_place(&ty, &TypeForm::RefShr, Capability::Own)
                .is_some()
        );
        assert!(
            pcpn.get_place(&ty, &TypeForm::RefMut, Capability::Own)
                .is_some()
        );
    }

    #[test]
    fn test_structural_transitions() {
        let mut pcpn = Pcpn::new();
        let ty = TyGround::primitive("i32");
        pcpn.type_universe.insert(ty.clone());
        pcpn.create_9_places_for_type(&ty, 3);
        pcpn.create_structural_transitions(&ty);

        let has_borrow_shr = pcpn
            .transitions
            .iter()
            .any(|t| t.name.contains("borrow_shr"));
        let has_borrow_mut = pcpn
            .transitions
            .iter()
            .any(|t| t.name.contains("borrow_mut"));
        let has_const = pcpn.transitions.iter().any(|t| t.name.contains("const_"));

        assert!(has_borrow_shr);
        assert!(has_borrow_mut);
        assert!(has_const);
    }

    // ===================================================================
    //  Counter ApiGraph → PCPN 转换测试
    // ===================================================================

    #[test]
    fn test_counter_api_to_pcpn() {
        let graph = build_counter_api_graph();
        let pcpn = Pcpn::from_api_graph(&graph);

        let counter_ty = TyGround::path("Counter");
        let i32_ty = TyGround::primitive("i32");

        // --- Places: 2 types × 9 = 18 ---
        assert_eq!(pcpn.places.len(), 18, "Counter×9 + i32×9 = 18 places");

        for form in [TypeForm::Value, TypeForm::RefShr, TypeForm::RefMut] {
            for cap in [Capability::Own, Capability::Frz, Capability::Blk] {
                assert!(
                    pcpn.get_place(&counter_ty, &form, cap).is_some(),
                    "Missing place: Counter {:?} {:?}", form, cap
                );
                assert!(
                    pcpn.get_place(&i32_ty, &form, cap).is_some(),
                    "Missing place: i32 {:?} {:?}", form, cap
                );
            }
        }

        // --- API Transitions ---
        let api_trans: Vec<_> = pcpn.transitions.iter()
            .filter(|t| matches!(t.kind, TransitionKind::ApiCall { .. } | TransitionKind::ConstProducer { .. }))
            .collect();
        assert_eq!(api_trans.len(), 4, "new/get/inc/into_value");

        // Counter::new → ConstProducer, output = (Counter, Value, Own)
        let new_t = api_trans.iter().find(|t| t.name == "Counter::new").unwrap();
        assert!(matches!(new_t.kind, TransitionKind::ConstProducer { .. }));
        assert!(new_t.input_arcs.is_empty());
        assert_eq!(new_t.output_arcs.len(), 1);
        let new_out = &pcpn.places[new_t.output_arcs[0].place_id];
        assert_eq!(new_out.base_type, counter_ty);
        assert_eq!(new_out.form, TypeForm::Value);
        assert_eq!(new_out.cap, Capability::Own);

        // Counter::get → input &Counter (RefShr,Own) read, output i32 (Value,Own)
        let get_t = api_trans.iter().find(|t| t.name == "Counter::get").unwrap();
        assert!(matches!(get_t.kind, TransitionKind::ApiCall { .. }));
        assert_eq!(get_t.input_arcs.len(), 1);
        let get_in = &pcpn.places[get_t.input_arcs[0].place_id];
        assert_eq!(get_in.base_type, counter_ty);
        assert_eq!(get_in.form, TypeForm::RefShr);
        assert!(!get_t.input_arcs[0].consumes, "&self is read-only");
        assert!(get_t.output_arcs.iter().any(|a| {
            let p = &pcpn.places[a.place_id];
            p.base_type == i32_ty && p.form == TypeForm::Value
        }));
        assert!(get_t.guards.iter().any(|g| g.kind == GuardKind::NoBlk));

        // Counter::inc → input &mut Counter (RefMut,Own) read, no meaningful output
        let inc_t = api_trans.iter().find(|t| t.name == "Counter::inc").unwrap();
        assert_eq!(inc_t.input_arcs.len(), 1);
        let inc_in = &pcpn.places[inc_t.input_arcs[0].place_id];
        assert_eq!(inc_in.base_type, counter_ty);
        assert_eq!(inc_in.form, TypeForm::RefMut);
        assert!(!inc_t.input_arcs[0].consumes);
        assert!(inc_t.guards.iter().any(|g| g.kind == GuardKind::NoFrzNoOtherBlk));

        // Counter::into_value → input Counter (Value,Own) consume, output i32
        let into_t = api_trans.iter().find(|t| t.name == "Counter::into_value").unwrap();
        assert_eq!(into_t.input_arcs.len(), 1);
        let into_in = &pcpn.places[into_t.input_arcs[0].place_id];
        assert_eq!(into_in.base_type, counter_ty);
        assert_eq!(into_in.form, TypeForm::Value);
        assert!(into_t.input_arcs[0].consumes, "move consumes token");
        assert!(into_t.output_arcs.iter().any(|a| {
            let p = &pcpn.places[a.place_id];
            p.base_type == i32_ty && p.form == TypeForm::Value
        }));
        assert!(into_t.guards.iter().any(|g| g.kind == GuardKind::NoFrzNoBlk));
    }

    #[test]
    fn test_pcpn_structural_completeness() {
        let graph = build_counter_api_graph();
        let pcpn = Pcpn::from_api_graph(&graph);

        let counter_ty = TyGround::path("Counter");
        let i32_ty = TyGround::primitive("i32");

        // i32 有 CreatePrimitive + CopyUse
        assert!(pcpn.transitions.iter().any(|t|
            matches!(&t.kind, TransitionKind::CreatePrimitive { ty } if *ty == i32_ty)));
        assert!(pcpn.transitions.iter().any(|t|
            matches!(&t.kind, TransitionKind::CopyUse { ty } if *ty == i32_ty)));

        // Counter 无 CreatePrimitive / CopyUse
        assert!(!pcpn.transitions.iter().any(|t|
            matches!(&t.kind, TransitionKind::CreatePrimitive { ty } if *ty == counter_ty)));
        assert!(!pcpn.transitions.iter().any(|t|
            matches!(&t.kind, TransitionKind::CopyUse { ty } if *ty == counter_ty)));

        // 每个类型都有 9 个标准结构转换
        for ty in [&counter_ty, &i32_ty] {
            let short = ty.short_name();
            for pat in [
                "borrow_shr_first", "borrow_shr_next",
                "end_shr_keep_frz", "end_shr_unfreeze",
                "borrow_mut", "end_mut",
                "drop_val", "drop_shr", "drop_mut",
            ] {
                let expected = format!("{}({})", pat, short);
                assert!(
                    pcpn.transitions.iter().any(|t| t.name == expected),
                    "Missing structural transition: {}", expected
                );
            }
        }
    }

    #[test]
    fn test_pcpn_guards() {
        let graph = build_counter_api_graph();
        let pcpn = Pcpn::from_api_graph(&graph);
        let counter_ty = TyGround::path("Counter");

        let bsf = pcpn.transitions.iter()
            .find(|t| t.name == "borrow_shr_first(Counter)").unwrap();
        assert!(bsf.guards.iter().any(|g| g.kind == GuardKind::NoBlk));

        let bm = pcpn.transitions.iter()
            .find(|t| t.name == "borrow_mut(Counter)").unwrap();
        assert!(bm.guards.iter().any(|g| g.kind == GuardKind::NoFrzNoBlk));

        for name in [
            "end_shr_keep_frz(Counter)",
            "end_shr_unfreeze(Counter)",
            "end_mut(Counter)",
        ] {
            let t = pcpn.transitions.iter().find(|t| t.name == name).unwrap();
            assert!(t.guards.iter().any(|g| g.kind == GuardKind::StackTopMatches),
                "{} should have StackTopMatches guard", name);
        }

        let dv = pcpn.transitions.iter()
            .find(|t| t.name == "drop_val(Counter)").unwrap();
        assert!(dv.guards.iter().any(|g| g.kind == GuardKind::NotBlocked && g.base_type == counter_ty));

        let ds = pcpn.transitions.iter()
            .find(|t| t.name == "drop_shr(Counter)").unwrap();
        assert!(ds.guards.is_empty());

        let dm = pcpn.transitions.iter()
            .find(|t| t.name == "drop_mut(Counter)").unwrap();
        assert!(dm.guards.is_empty());
    }

    #[test]
    fn test_pcpn_stats() {
        let graph = build_counter_api_graph();
        let pcpn = Pcpn::from_api_graph(&graph);
        let stats = pcpn.stats();

        assert_eq!(stats.num_places, 18);
        assert_eq!(stats.num_types, 2);
        assert_eq!(stats.num_api_transitions, 3, "get, inc, into_value");
        assert_eq!(stats.num_const_transitions, 2, "Counter::new + const_i32");
        let total = stats.num_api_transitions + stats.num_const_transitions + stats.num_structural_transitions;
        assert_eq!(total, stats.num_transitions);
    }
}
