//! PCPN (Pushdown Colored Petri Net)
//!
//! ## 核心设
//!
//! ### Place 设计
//! - 每个类型 Ty 一个 place: P[Ty]
//! - Ty ::= BaseTy | Ref{mut}(Ty)
//! - P[T] 存放 T token
//! - P[&T] 存放 &T token
//! - P[&mut T] 存放 &mut T token
//! - P[&&T] 存放 &&T token（嵌套引用）
//!
//! ### Token 设计
//! - Token 只包含类型信息（完整的引用层级）
//! - 不携带变量 ID
//!
//! ### 关键变迁
//! - BorrowMut(T): P[T] → P[&mut T]
//! - EndBorrowMut(T): P[&mut T] → P[T]
//! - BorrowShr(T): P[T] → P[&T]
//! - EndBorrowShr(T): P[&T] → P[T]
//! - DerefRef(T): P[&&T] → P[&T] (降阶)
//! - MutRefToShrRef(T): P[&mut T] → P[&T] (降权)
//!
//! ### Copy 语义
//! - Copy 类型传参：返还弧（pre-1, post+1）
//! - 非 Copy 类型传参：消耗（move）
//! - 引用参数：总是消耗

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::apigraph::{ApiGraph, FunctionNode};
use crate::type_model::{PassingMode, TypeKey};

/// Place 标识
pub type PlaceId = usize;

/// Transition 标识
pub type TransitionId = usize;

/// Capability（简化版：只区分是否是引用）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// 拥有所有权
    Own,
    /// 冻结状态（兼容旧接口）
    Frz,
    /// 阻塞状态（兼容旧接口）
    Blk,
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Capability::Own => write!(f, "own"),
            Capability::Frz => write!(f, "frz"),
            Capability::Blk => write!(f, "blk"),
        }
    }
}

/// PCPN Place (库所) - 简化版
///
/// 每个类型 Ty 一个 Place
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Place {
    /// Place 唯一标识
    pub id: PlaceId,
    /// 类型键（完整类型，包含引用层级）
    pub type_key: TypeKey,
    /// 该库所对应的 capability
    pub capability: Capability,
    /// 是否是 primitive 类型
    pub is_primitive: bool,
    /// 是否是引用类型
    pub is_ref: bool,
    /// Token 上限（budget）
    pub budget: usize,
}

impl Place {
    /// 获取 Place 的显示名称
    pub fn display_name(&self) -> String {
        self.type_key.short_name()
    }
}

/// PCPN Transition (变迁)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transition {
    /// Transition 唯一标识
    pub id: TransitionId,
    /// 名称
    pub name: String,
    /// 变迁类型
    pub kind: TransitionKind,
    /// 输入弧
    pub input_arcs: Vec<Arc>,
    /// 输出弧
    pub output_arcs: Vec<Arc>,
}

/// 变迁类型
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionKind {
    /// API 调用
    ApiCall { fn_id: usize },
    /// 创建 primitive 常量
    CreatePrimitive { type_key: TypeKey },

    // ========== 借用变迁（线性模型）==========
    /// 可变借用: P[T] → P[&mut T]
    BorrowMut { base_type: TypeKey },
    /// 结束可变借用: P[&mut T] → P[T]
    EndBorrowMut { base_type: TypeKey },
    /// 共享借用: P[T] → P[&T]
    BorrowShr { base_type: TypeKey },
    /// 结束共享借用: P[&T] → P[T]
    EndBorrowShr { base_type: TypeKey },

    // ========== 引用降阶/降权 ==========
    /// 解引用: P[&&T] → P[&T] 或 P[&&mut T] → P[&mut T]
    DerefRef { inner_type: TypeKey },
    /// 降权: P[&mut T] → P[&T]
    MutRefToShrRef { base_type: TypeKey },

    // ========== 其他变迁 ==========
    /// Drop: P[T] → ε
    Drop { type_key: TypeKey },
    /// DupCopy: P[T] → P[T] + P[T]（Copy 类型扩增）
    DupCopy { type_key: TypeKey },
    /// DupClone: P[T] → P[T] + P[T]（Clone 类型扩增）
    DupClone { type_key: TypeKey },
}

/// 弧 (连接 Place 和 Transition)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Arc {
    /// 连接的 Place
    pub place_id: PlaceId,
    /// 是否消耗 token
    pub consumes: bool,
    /// 弧的注解（用于调试）
    pub annotation: Option<ArcAnnotation>,
}

/// 弧的注解
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ArcAnnotation {
    /// 参数位置
    Param { index: usize, name: String },
    /// Self 参数
    SelfParam,
    /// 返回值
    Return,
    /// 返还弧（Copy 类型）
    ReturnArc,
}

/// PCPN 网络
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pcpn {
    /// 所有库所
    pub places: Vec<Place>,
    /// 所有变迁
    pub transitions: Vec<Transition>,
    /// 类型 → Place 的映射
    #[serde(skip)]
    pub type_to_place: HashMap<TypeKey, PlaceId>,
    /// 初始标识 (primitive 类型的 places)
    pub initial_places: Vec<PlaceId>,
}

impl Default for Pcpn {
    fn default() -> Self {
        Self::new()
    }
}

/// 单态化函数实例
#[derive(Clone, Debug)]
struct MonomorphizedFn {
    /// 原始函数 ID
    fn_id: usize,
    /// 单态化后的函数名
    name: String,
    /// 泛型参数绑定 (context, name) -> concrete_type
    substitutions: HashMap<(String, String), TypeKey>,
}

impl Pcpn {
    /// 创建空的 PCPN
    pub fn new() -> Self {
        Pcpn {
            places: Vec::new(),
            transitions: Vec::new(),
            type_to_place: HashMap::new(),
            initial_places: Vec::new(),
        }
    }

    /// 从 ApiGraph 转换为 PCPN（新版简化设计）
    ///
    /// Place 设计：每个 Ty 一个 Place
    /// - P[T] 存放 T
    /// - P[&T] 存放 &T
    /// - P[&mut T] 存放 &mut T
    pub fn from_api_graph(graph: &ApiGraph) -> Self {
        let mut pcpn = Pcpn::new();

        // 1. 收集所有具体类型（不包含泛型参数）
        let mut concrete_types: IndexSet<TypeKey> = IndexSet::new();

        for type_node in &graph.type_nodes {
            let ty = &type_node.type_key;
            if !ty.contains_generic_param() {
                concrete_types.insert(ty.clone());
            }
        }

        // 2. 单态化泛型函数
        let monomorphized_fns = pcpn.monomorphize_functions(graph, &concrete_types);

        // 3. 收集所有需要的类型（包括引用类型）
        let mut all_types: IndexSet<TypeKey> = IndexSet::new();
        for ty in &concrete_types {
            all_types.insert(ty.clone());
            // 添加引用类型
            if !ty.is_ref() {
                all_types.insert(TypeKey::ref_shr(ty.clone()));
                all_types.insert(TypeKey::ref_mut(ty.clone()));
            }
        }

        // 从单态化函数中收集额外的类型
        for mono_fn in &monomorphized_fns {
            pcpn.collect_fn_all_types(
                graph,
                &graph.fn_nodes[mono_fn.fn_id],
                &mono_fn.substitutions,
                &mut all_types,
            );
        }

        // 4. 为每个类型创建 Place
        for ty in &all_types {
            if !ty.contains_generic_param() {
                pcpn.create_place_for_type(ty);
            }
        }

        // 5. 创建 API 调用变迁（单态化后）
        for mono_fn in &monomorphized_fns {
            pcpn.create_api_call_transition(graph, mono_fn);
        }

        // 6. 创建结构性变迁（借用、解引用等）
        for ty in &all_types {
            if !ty.contains_generic_param() && !ty.is_ref() && !ty.is_primitive() {
                pcpn.create_structural_transitions_for_type(ty);
            }
        }

        // 7. 创建 primitive 类型的创建变迁
        for &place_id in &pcpn.initial_places.clone() {
            let place = &pcpn.places[place_id];
            if !place.is_ref {
                let type_key = place.type_key.clone();
                pcpn.create_primitive_transition(&type_key, place_id);
            }
        }

        pcpn
    }

    /// 单态化泛型函数
    fn monomorphize_functions(
        &self,
        graph: &ApiGraph,
        concrete_types: &IndexSet<TypeKey>,
    ) -> Vec<MonomorphizedFn> {
        let mut result = Vec::new();

        for fn_node in &graph.fn_nodes {
            let generic_params = self.collect_fn_generic_params(graph, fn_node);

            if generic_params.is_empty() {
                // 非泛型函数
                result.push(MonomorphizedFn {
                    fn_id: fn_node.id,
                    name: fn_node.path.clone(),
                    substitutions: HashMap::new(),
                });
            } else {
                // 泛型函数：枚举所有满足约束的实例化
                let instantiations = self.enumerate_instantiations(&generic_params, concrete_types);
                for substitutions in instantiations {
                    let mono_name = self.build_monomorphized_name(&fn_node.path, &substitutions);
                    result.push(MonomorphizedFn {
                        fn_id: fn_node.id,
                        name: mono_name,
                        substitutions,
                    });
                }
            }
        }

        result
    }

    /// 收集函数中的所有泛型参数
    fn collect_fn_generic_params(
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

    /// 收集函数涉及的所有类型（包括引用）
    fn collect_fn_all_types(
        &self,
        graph: &ApiGraph,
        fn_node: &FunctionNode,
        substitutions: &HashMap<(String, String), TypeKey>,
        all_types: &mut IndexSet<TypeKey>,
    ) {
        for edge in graph.get_input_edges(fn_node.id) {
            let ty = graph.type_nodes[edge.type_node]
                .type_key
                .substitute(substitutions);
            if !ty.contains_generic_param() {
                all_types.insert(ty.clone());
                // 根据传递模式添加引用类型
                match &edge.passing_mode {
                    PassingMode::BorrowShr => {
                        all_types.insert(TypeKey::ref_shr(ty.clone()));
                    }
                    PassingMode::BorrowMut => {
                        all_types.insert(TypeKey::ref_mut(ty.clone()));
                    }
                    _ => {}
                }
            }
        }

        for edge in graph.get_output_edges(fn_node.id) {
            let ty = graph.type_nodes[edge.type_node]
                .type_key
                .substitute(substitutions);
            if !ty.contains_generic_param() {
                all_types.insert(ty.clone());
                match &edge.passing_mode {
                    PassingMode::ReturnBorrowShr => {
                        all_types.insert(TypeKey::ref_shr(ty.clone()));
                    }
                    PassingMode::ReturnBorrowMut => {
                        all_types.insert(TypeKey::ref_mut(ty.clone()));
                    }
                    _ => {}
                }
            }
        }
    }

    /// 枚举所有满足约束的泛型参数实例化组合
    fn enumerate_instantiations(
        &self,
        generic_params: &[(String, String, Vec<String>)],
        concrete_types: &IndexSet<TypeKey>,
    ) -> Vec<HashMap<(String, String), TypeKey>> {
        if generic_params.is_empty() {
            return vec![HashMap::new()];
        }

        let mut candidates: Vec<Vec<(&(String, String, Vec<String>), &TypeKey)>> = Vec::new();

        for param in generic_params {
            let (_ctx, _name, bounds) = param;
            let mut matching_types: Vec<(&(String, String, Vec<String>), &TypeKey)> = Vec::new();

            for ty in concrete_types {
                if ty.is_generic_param() || ty.is_ref() {
                    continue;
                }
                if bounds.is_empty() || self.type_satisfies_bounds(ty, bounds) {
                    matching_types.push((param, ty));
                }
            }

            if matching_types.is_empty() {
                return Vec::new();
            }
            candidates.push(matching_types);
        }

        self.cartesian_product(&candidates)
    }

    /// 检查类型是否满足 bounds
    fn type_satisfies_bounds(&self, ty: &TypeKey, bounds: &[String]) -> bool {
        for bound in bounds {
            let bound_lower = bound.to_lowercase();
            let satisfied = match bound_lower.as_str() {
                "default" => ty.is_primitive(),
                "copy" => ty.is_copy(),
                "clone" => true,
                "fnonce" | "fn" | "fnmut" => false,
                _ => true,
            };
            if !satisfied {
                return false;
            }
        }
        true
    }

    /// 生成笛卡尔积
    fn cartesian_product(
        &self,
        candidates: &[Vec<(&(String, String, Vec<String>), &TypeKey)>],
    ) -> Vec<HashMap<(String, String), TypeKey>> {
        if candidates.is_empty() {
            return vec![HashMap::new()];
        }

        let first = &candidates[0];
        let rest = &candidates[1..];
        let rest_products = self.cartesian_product(rest);
        let mut result = Vec::new();

        for (param, ty) in first {
            let (ctx, name, _) = param;
            for mut rest_map in rest_products.clone() {
                rest_map.insert((ctx.clone(), name.clone()), (*ty).clone());
                result.push(rest_map);
            }
        }

        result
    }

    /// 构建单态化后的函数名
    fn build_monomorphized_name(
        &self,
        path: &str,
        substitutions: &HashMap<(String, String), TypeKey>,
    ) -> String {
        if substitutions.is_empty() {
            return path.to_string();
        }

        let mut type_args: Vec<String> = substitutions.values().map(|ty| ty.short_name()).collect();
        type_args.sort();

        if path.contains('<') {
            if let Some(lt_pos) = path.find('<') {
                let mut depth = 0;
                let mut gt_pos = lt_pos;
                for (i, c) in path[lt_pos..].char_indices() {
                    match c {
                        '<' => depth += 1,
                        '>' => {
                            depth -= 1;
                            if depth == 0 {
                                gt_pos = lt_pos + i;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let base = &path[..lt_pos];
                let suffix = &path[gt_pos + 1..];
                format!("{}<{}>{}", base, type_args.join(", "), suffix)
            } else {
                format!("{}<{}>", path, type_args.join(", "))
            }
        } else if let Some(last_colon) = path.rfind("::") {
            let type_part = &path[..last_colon];
            let method_part = &path[last_colon..];
            format!("{}<{}>{}", type_part, type_args.join(", "), method_part)
        } else {
            format!("{}<{}>", path, type_args.join(", "))
        }
    }

    /// 为类型创建 Place
    fn create_place_for_type(&mut self, type_key: &TypeKey) {
        if self.type_to_place.contains_key(type_key) {
            return;
        }

        let is_primitive = type_key.base_type().is_primitive();
        let is_ref = type_key.is_ref();

        // Primitive 类型：budget = 1，初始有 1 个 token
        // 其他类型：budget = 3
        let budget = if is_primitive && !is_ref { 1 } else { 3 };

        let place_id = self.places.len();
        self.places.push(Place {
            id: place_id,
            type_key: type_key.clone(),
            capability: Capability::Own,
            is_primitive,
            is_ref,
            budget,
        });
        self.type_to_place.insert(type_key.clone(), place_id);

        // Primitive 类型（非引用）加入初始 places
        if is_primitive && !is_ref {
            self.initial_places.push(place_id);
        }
    }

    /// 获取类型对应的 Place ID
    pub fn get_place(&self, type_key: &TypeKey) -> Option<PlaceId> {
        self.type_to_place.get(type_key).copied()
    }

    /// 创建 API 调用变迁
    ///
    /// 关键规则（新版）：
    /// - Copy 类型参数：返还弧（不消耗）
    /// - 非 Copy 类型参数：消耗（move）
    /// - 引用参数（&T, &mut T）：总是消耗
    fn create_api_call_transition(&mut self, graph: &ApiGraph, mono_fn: &MonomorphizedFn) {
        let fn_node = &graph.fn_nodes[mono_fn.fn_id];
        let trans_id = self.transitions.len();
        let mut input_arcs = Vec::new();
        let mut output_arcs = Vec::new();

        // 处理输入边
        for (idx, edge) in graph.get_input_edges(fn_node.id).iter().enumerate() {
            let original_type = &graph.type_nodes[edge.type_node].type_key;
            let param_type = original_type.substitute(&mono_fn.substitutions);

            if param_type.contains_generic_param() {
                continue;
            }

            // 根据 PassingMode 确定实际的 Place 类型
            let place_type = match &edge.passing_mode {
                PassingMode::Move | PassingMode::Copy => param_type.clone(),
                PassingMode::BorrowShr => TypeKey::ref_shr(param_type.clone()),
                PassingMode::BorrowMut => TypeKey::ref_mut(param_type.clone()),
                _ => continue,
            };

            // 确保 Place 存在
            let place_id = if let Some(id) = self.get_place(&place_type) {
                id
            } else {
                self.create_place_for_type(&place_type);
                self.get_place(&place_type).unwrap()
            };

            // 判断是否消耗 token
            // - Copy 类型 + 非引用：不消耗（返还弧）
            // - 引用类型：总是消耗
            // - 非 Copy 类型：消耗（move）
            let is_ref_param = matches!(
                edge.passing_mode,
                PassingMode::BorrowShr | PassingMode::BorrowMut
            );
            let consumes = if is_ref_param {
                true // 引用参数总是消耗
            } else if param_type.is_copy() {
                false // Copy 类型不消耗（返还弧）
            } else {
                true // 非 Copy 类型消耗
            };

            let annotation = edge.param_index.map(|param_idx| {
                if fn_node.self_param.is_some() && param_idx == 0 {
                    ArcAnnotation::SelfParam
                } else {
                    let real_idx = if fn_node.self_param.is_some() {
                        param_idx - 1
                    } else {
                        param_idx
                    };
                    let name = fn_node
                        .params
                        .get(real_idx)
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| format!("arg{}", real_idx));
                    ArcAnnotation::Param { index: idx, name }
                }
            });

            input_arcs.push(Arc {
                place_id,
                consumes,
                annotation,
            });

            // 注意：Copy 类型使用非消耗弧（读取弧），不需要返还弧
            // 因为 token 本身没有被消耗，所以不需要"返还"
        }

        // 处理输出边
        for edge in graph.get_output_edges(fn_node.id) {
            let original_type = &graph.type_nodes[edge.type_node].type_key;
            let ret_type = original_type.substitute(&mono_fn.substitutions);

            if ret_type.contains_generic_param() {
                continue;
            }

            let place_type = match &edge.passing_mode {
                PassingMode::ReturnOwned => ret_type.clone(),
                PassingMode::ReturnBorrowShr => TypeKey::ref_shr(ret_type.clone()),
                PassingMode::ReturnBorrowMut => TypeKey::ref_mut(ret_type.clone()),
                _ => continue,
            };

            let place_id = if let Some(id) = self.get_place(&place_type) {
                id
            } else {
                self.create_place_for_type(&place_type);
                self.get_place(&place_type).unwrap()
            };

            output_arcs.push(Arc {
                place_id,
                consumes: false,
                annotation: Some(ArcAnnotation::Return),
            });
        }

        // 只有当有输入或输出时才创建变迁
        if output_arcs.is_empty() && input_arcs.is_empty() {
            return;
        }

        self.transitions.push(Transition {
            id: trans_id,
            name: mono_fn.name.clone(),
            kind: TransitionKind::ApiCall {
                fn_id: mono_fn.fn_id,
            },
            input_arcs,
            output_arcs,
        });
    }

    /// 为类型创建结构性变迁
    fn create_structural_transitions_for_type(&mut self, base_type: &TypeKey) {
        let short_name = base_type.short_name();
        let is_copy = base_type.is_copy();

        // 获取/创建相关 places
        let own_place = self.get_or_create_place(base_type);
        let ref_shr_type = TypeKey::ref_shr(base_type.clone());
        let ref_mut_type = TypeKey::ref_mut(base_type.clone());
        let ref_shr_place = self.get_or_create_place(&ref_shr_type);
        let ref_mut_place = self.get_or_create_place(&ref_mut_type);

        // BorrowMut: P[T] → P[&mut T]
        self.add_transition(
            format!("borrow_mut({})", short_name),
            TransitionKind::BorrowMut {
                base_type: base_type.clone(),
            },
            vec![Arc {
                place_id: own_place,
                consumes: true,
                annotation: None,
            }],
            vec![Arc {
                place_id: ref_mut_place,
                consumes: false,
                annotation: None,
            }],
        );

        // EndBorrowMut: P[&mut T] → P[T]
        self.add_transition(
            format!("end_borrow_mut({})", short_name),
            TransitionKind::EndBorrowMut {
                base_type: base_type.clone(),
            },
            vec![Arc {
                place_id: ref_mut_place,
                consumes: true,
                annotation: None,
            }],
            vec![Arc {
                place_id: own_place,
                consumes: false,
                annotation: None,
            }],
        );

        // BorrowShr: P[T] → P[&T]
        self.add_transition(
            format!("borrow_shr({})", short_name),
            TransitionKind::BorrowShr {
                base_type: base_type.clone(),
            },
            vec![Arc {
                place_id: own_place,
                consumes: true,
                annotation: None,
            }],
            vec![Arc {
                place_id: ref_shr_place,
                consumes: false,
                annotation: None,
            }],
        );

        // EndBorrowShr: P[&T] → P[T]
        self.add_transition(
            format!("end_borrow_shr({})", short_name),
            TransitionKind::EndBorrowShr {
                base_type: base_type.clone(),
            },
            vec![Arc {
                place_id: ref_shr_place,
                consumes: true,
                annotation: None,
            }],
            vec![Arc {
                place_id: own_place,
                consumes: false,
                annotation: None,
            }],
        );

        // MutRefToShrRef: P[&mut T] → P[&T]（降权）
        self.add_transition(
            format!("mut_to_shr({})", short_name),
            TransitionKind::MutRefToShrRef {
                base_type: base_type.clone(),
            },
            vec![Arc {
                place_id: ref_mut_place,
                consumes: true,
                annotation: None,
            }],
            vec![Arc {
                place_id: ref_shr_place,
                consumes: false,
                annotation: None,
            }],
        );

        // Drop: P[T] → ε
        self.add_transition(
            format!("drop({})", short_name),
            TransitionKind::Drop {
                type_key: base_type.clone(),
            },
            vec![Arc {
                place_id: own_place,
                consumes: true,
                annotation: None,
            }],
            vec![],
        );

        // Copy 类型：DupCopy: P[T] → P[T] + P[T]
        if is_copy {
            self.add_transition(
                format!("dup_copy({})", short_name),
                TransitionKind::DupCopy {
                    type_key: base_type.clone(),
                },
                vec![Arc {
                    place_id: own_place,
                    consumes: false,
                    annotation: None,
                }],
                vec![Arc {
                    place_id: own_place,
                    consumes: false,
                    annotation: None,
                }],
            );
        } else {
            // 非 Copy 类型：DupClone
            self.add_transition(
                format!("dup_clone({})", short_name),
                TransitionKind::DupClone {
                    type_key: base_type.clone(),
                },
                vec![Arc {
                    place_id: own_place,
                    consumes: false,
                    annotation: None,
                }],
                vec![Arc {
                    place_id: own_place,
                    consumes: false,
                    annotation: None,
                }],
            );
        }

        // 嵌套引用降阶：P[&&T] → P[&T]
        let ref_ref_shr_type = TypeKey::ref_shr(ref_shr_type.clone());
        if self.get_place(&ref_ref_shr_type).is_some() || true {
            // 只有当确实有 &&T 类型时才创建
            let ref_ref_place = self.get_or_create_place(&ref_ref_shr_type);
            self.add_transition(
                format!("deref(&&{})", short_name),
                TransitionKind::DerefRef {
                    inner_type: ref_shr_type.clone(),
                },
                vec![Arc {
                    place_id: ref_ref_place,
                    consumes: true,
                    annotation: None,
                }],
                vec![Arc {
                    place_id: ref_shr_place,
                    consumes: false,
                    annotation: None,
                }],
            );
        }
    }

    /// 获取或创建 Place
    fn get_or_create_place(&mut self, type_key: &TypeKey) -> PlaceId {
        if let Some(id) = self.get_place(type_key) {
            id
        } else {
            self.create_place_for_type(type_key);
            self.get_place(type_key).unwrap()
        }
    }

    /// 创建 primitive 常量变迁
    fn create_primitive_transition(&mut self, type_key: &TypeKey, place_id: PlaceId) {
        self.add_transition(
            format!("const_{}", type_key.short_name()),
            TransitionKind::CreatePrimitive {
                type_key: type_key.clone(),
            },
            vec![],
            vec![Arc {
                place_id,
                consumes: false,
                annotation: None,
            }],
        );
    }

    /// 添加变迁
    fn add_transition(
        &mut self,
        name: String,
        kind: TransitionKind,
        input_arcs: Vec<Arc>,
        output_arcs: Vec<Arc>,
    ) {
        let id = self.transitions.len();
        self.transitions.push(Transition {
            id,
            name,
            kind,
            input_arcs,
            output_arcs,
        });
    }

    /// 生成 DOT 格式
    pub fn to_dot(&self) -> String {
        let mut dot = String::new();
        dot.push_str("digraph PCPN {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  fontname=\"Helvetica\";\n");
        dot.push_str("  node [fontname=\"Helvetica\"];\n");
        dot.push_str("  edge [fontname=\"Helvetica\"];\n");
        dot.push_str("  // PCPN: One place per type (simplified)\n\n");

        // Places
        dot.push_str("  // ========== Places ==========\n");
        for place in &self.places {
            let fillcolor = if place.is_ref {
                match &place.type_key {
                    TypeKey::RefShr(_) => "lightcyan",
                    TypeKey::RefMut(_) => "mistyrose",
                    _ => "white",
                }
            } else if place.is_primitive {
                "lightgray"
            } else {
                "lightblue"
            };

            let peripheries = if self.initial_places.contains(&place.id) {
                2
            } else {
                1
            };
            let label = format!("{}\\n[B={}]", place.display_name(), place.budget);

            dot.push_str(&format!(
                "  p{} [label=\"{}\", shape=circle, style=filled, fillcolor={}, peripheries={}];\n",
                place.id, label, fillcolor, peripheries
            ));
        }
        dot.push_str("\n");

        // Transitions
        dot.push_str("  // ========== Transitions ==========\n");
        for trans in &self.transitions {
            let (color, shape) = match &trans.kind {
                TransitionKind::ApiCall { .. } => ("palegreen", "box"),
                TransitionKind::CreatePrimitive { .. } => ("lightcyan", "diamond"),
                TransitionKind::BorrowMut { .. } | TransitionKind::BorrowShr { .. } => {
                    ("lavender", "box")
                }
                TransitionKind::EndBorrowMut { .. } | TransitionKind::EndBorrowShr { .. } => {
                    ("honeydew", "box")
                }
                TransitionKind::DerefRef { .. } | TransitionKind::MutRefToShrRef { .. } => {
                    ("wheat", "box")
                }
                TransitionKind::Drop { .. } => ("gray90", "box"),
                TransitionKind::DupCopy { .. } | TransitionKind::DupClone { .. } => {
                    ("paleturquoise", "box")
                }
            };

            dot.push_str(&format!(
                "  t{} [label=\"{}\", shape={}, style=filled, fillcolor={}];\n",
                trans.id, trans.name, shape, color
            ));
        }
        dot.push_str("\n");

        // Arcs
        dot.push_str("  // ========== Arcs ==========\n");
        for trans in &self.transitions {
            for arc in &trans.input_arcs {
                let style = if arc.consumes { "solid" } else { "dashed" };
                let arrow = if arc.consumes { "normal" } else { "odot" };
                let color = self.arc_color(arc.place_id);

                dot.push_str(&format!(
                    "  p{} -> t{} [style={}, arrowhead={}, color=\"{}\"];\n",
                    arc.place_id, trans.id, style, arrow, color
                ));
            }

            for arc in &trans.output_arcs {
                let color = self.arc_color(arc.place_id);
                let style = match &arc.annotation {
                    Some(ArcAnnotation::ReturnArc) => "dashed",
                    _ => "solid",
                };
                dot.push_str(&format!(
                    "  t{} -> p{} [style={}, color=\"{}\"];\n",
                    trans.id, arc.place_id, style, color
                ));
            }
        }

        dot.push_str("}\n");
        dot
    }

    fn arc_color(&self, place_id: PlaceId) -> &'static str {
        if let Some(place) = self.places.get(place_id) {
            if place.is_ref {
                match &place.type_key {
                    TypeKey::RefShr(_) => "blue",
                    TypeKey::RefMut(_) => "red",
                    _ => "black",
                }
            } else {
                "black"
            }
        } else {
            "black"
        }
    }

    /// 统计信息
    pub fn stats(&self) -> PcpnStats {
        let api_trans = self
            .transitions
            .iter()
            .filter(|t| matches!(t.kind, TransitionKind::ApiCall { .. }))
            .count();
        let structural_trans = self.transitions.len() - api_trans;

        let ref_places = self.places.iter().filter(|p| p.is_ref).count();
        let base_places = self.places.len() - ref_places;

        PcpnStats {
            num_places: self.places.len(),
            num_base_places: base_places,
            num_ref_places: ref_places,
            num_transitions: self.transitions.len(),
            num_api_transitions: api_trans,
            num_structural_transitions: structural_trans,
            num_primitive_places: self.initial_places.len(),
        }
    }
}

/// PCPN 统计
#[derive(Debug)]
pub struct PcpnStats {
    pub num_places: usize,
    pub num_base_places: usize,
    pub num_ref_places: usize,
    pub num_transitions: usize,
    pub num_api_transitions: usize,
    pub num_structural_transitions: usize,
    pub num_primitive_places: usize,
}

impl std::fmt::Display for PcpnStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PCPN: {} places ({} base + {} ref, {} primitive), {} transitions ({} API, {} structural)",
            self.num_places,
            self.num_base_places,
            self.num_ref_places,
            self.num_primitive_places,
            self.num_transitions,
            self.num_api_transitions,
            self.num_structural_transitions
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_place_for_type() {
        let mut pcpn = Pcpn::new();
        let counter_type = TypeKey::path("Counter");

        pcpn.create_place_for_type(&counter_type);

        assert_eq!(pcpn.places.len(), 1);
        assert!(pcpn.get_place(&counter_type).is_some());
    }

    #[test]
    fn test_ref_type_place() {
        let mut pcpn = Pcpn::new();
        let ref_shr_counter = TypeKey::ref_shr(TypeKey::path("Counter"));

        pcpn.create_place_for_type(&ref_shr_counter);

        assert_eq!(pcpn.places.len(), 1);
        assert!(pcpn.places[0].is_ref);
    }
}
