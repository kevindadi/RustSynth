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
//!
//! ### Copy 语义
//! - Copy 类型传参：返还弧（pre-1, post+1）
//! - 非 Copy 类型传参：消耗（move）
//! - 引用参数：总是消耗

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::apigraph::{ApiGraph, FunctionNode};
use crate::type_model::TypeKey;

/// Place 标识
pub type PlaceId = usize;

/// Transition 标识
pub type TransitionId = usize;

/// Capability（新版：三种库所）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// 所有权库所 - 持有值本身
    Own,
    /// 共享引用库所 - 持有 &T
    Shr,
    /// 可变借用库所 - 持有 &mut T
    Mut,
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Capability::Own => write!(f, "own"),
            Capability::Shr => write!(f, "shr"),
            Capability::Mut => write!(f, "mut"),
        }
    }
}

/// PCPN Place (库所) - 新版设计
///
/// 每个类型有三个 Place：own, shr, mut
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Place {
    /// Place 唯一标识
    pub id: PlaceId,
    /// 类型键（base type，不含引用）
    pub type_key: TypeKey,
    /// 该库所对应的 capability
    pub capability: Capability,
    /// 是否是 primitive 类型
    pub is_primitive: bool,
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
    /// Guard 条件
    pub guards: Vec<Guard>,
}

/// 变迁类型
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionKind {
    /// API 调用
    ApiCall { fn_id: usize },
    /// 创建 primitive 常量
    CreatePrimitive { type_key: TypeKey },

    // ========== 借用变迁（带 token 追踪）==========
    /// 可变借用: P[T, own] → P[T, mut]
    /// 语义：从 own 库所取出 token_id，在 mut 库所生成新 token，记录借用关系
    BorrowMut {
        base_type: TypeKey,
        /// 原 token ID（从 own 库所）
        source_token: Option<TokenId>,
        /// 新 token ID（在 mut 库所）
        borrow_token: Option<TokenId>,
    },
    /// 结束可变借用: P[T, mut] → P[T, own]
    /// 语义：归还 mut 库所的 token，恢复 own 库所的原 token
    EndBorrowMut {
        base_type: TypeKey,
        /// 借用 token ID（从 mut 库所）
        borrow_token: Option<TokenId>,
        /// 原 token ID（归还到 own 库所）
        source_token: Option<TokenId>,
    },
    /// 共享借用: P[T, own] → P[T, shr]
    BorrowShr {
        base_type: TypeKey,
        source_token: Option<TokenId>,
        borrow_token: Option<TokenId>,
    },
    /// 结束共享借用: P[T, shr] → P[T, own]
    EndBorrowShr {
        base_type: TypeKey,
        borrow_token: Option<TokenId>,
        source_token: Option<TokenId>,
    },

    // ========== 引用降阶/降权 ==========
    /// 解引用: P[&&T] → P[&T] 或 P[&&mut T] → P[&mut T]
    DerefRef { inner_type: TypeKey },

    // ========== 其他变迁 ==========
    /// Drop: P[T] → ε
    Drop { type_key: TypeKey },
    // 注意：删除了 DupCopy 和 DupClone，Copy 语义通过 fire 时自动复制实现
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

/// Token 唯一标识
pub type TokenId = usize;

/// 生命周期栈帧
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LifetimeFrame {
    /// 生命周期标识
    pub lifetime: String,
    /// 这个生命周期内的借用 token ID 列表
    pub borrows: Vec<TokenId>,
    /// 这些借用引用的源 token（被禁止 drop 或可变操作）
    pub blocks: Vec<TokenId>,
}

/// 生命周期栈
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LifetimeStack {
    /// 栈帧列表（栈顶在末尾）
    pub frames: Vec<LifetimeFrame>,
}

impl LifetimeStack {
    pub fn new() -> Self {
        LifetimeStack { frames: Vec::new() }
    }

    /// 压栈：创建新的生命周期帧
    pub fn push_frame(&mut self, lifetime: String) {
        self.frames.push(LifetimeFrame {
            lifetime,
            borrows: Vec::new(),
            blocks: Vec::new(),
        });
    }

    /// 弹栈：移除生命周期帧，返回需要释放的借用
    pub fn pop_frame(&mut self) -> Option<LifetimeFrame> {
        self.frames.pop()
    }

    /// 添加借用到当前帧
    pub fn add_borrow(&mut self, lifetime: &str, borrow_id: TokenId, source_id: TokenId) {
        if let Some(frame) = self
            .frames
            .iter_mut()
            .rev()
            .find(|f| f.lifetime == lifetime)
        {
            frame.borrows.push(borrow_id);
            frame.blocks.push(source_id);
        }
    }

    /// 检查 token 是否被阻塞（不能 drop 或可变操作）
    pub fn is_blocked(&self, token_id: TokenId) -> bool {
        self.frames.iter().any(|f| f.blocks.contains(&token_id))
    }
}

/// Token 结构（带唯一 ID）
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Token {
    /// Token 唯一 ID
    pub id: TokenId,
    /// Token 的类型
    pub type_key: TypeKey,
    /// Token 当前所在的库所能力
    pub capability: Capability,
    /// 如果是借用 token，记录源 token ID
    pub borrowed_from: Option<TokenId>,
    /// 引用层级：0 = 值(T), 1 = 一级引用(&T), 2 = 二级引用(&&T), ...
    pub ref_level: usize,
    /// 生命周期标识（用于跟踪生命周期绑定）
    pub lifetime: Option<String>,
}

impl Token {
    /// 创建新的所有权 token（值，ref_level = 0）
    pub fn new_owned(id: TokenId, type_key: TypeKey) -> Self {
        Token {
            id,
            type_key,
            capability: Capability::Own,
            borrowed_from: None,
            ref_level: 0, // 值，不是引用
            lifetime: None,
        }
    }

    /// 从所有权 token 创建借用 token（一级引用，ref_level = 1）
    pub fn borrow_shr(
        id: TokenId,
        type_key: TypeKey,
        from_id: TokenId,
        lifetime: Option<String>,
    ) -> Self {
        Token {
            id,
            type_key,
            capability: Capability::Shr,
            borrowed_from: Some(from_id),
            ref_level: 1, // 一级引用 &T
            lifetime,
        }
    }

    /// 从所有权 token 创建可变借用 token（一级引用，ref_level = 1）
    pub fn borrow_mut(
        id: TokenId,
        type_key: TypeKey,
        from_id: TokenId,
        lifetime: Option<String>,
    ) -> Self {
        Token {
            id,
            type_key,
            capability: Capability::Mut,
            borrowed_from: Some(from_id),
            ref_level: 1, // 一级引用 &mut T
            lifetime,
        }
    }

    /// 创建更高层级的引用（&&T, &&&T, ...）
    pub fn add_ref_level(&self, new_id: TokenId) -> Self {
        Token {
            id: new_id,
            type_key: self.type_key.clone(),
            capability: Capability::Shr, // 多级引用总是共享的
            borrowed_from: Some(self.id),
            ref_level: self.ref_level + 1,
            lifetime: self.lifetime.clone(),
        }
    }

    /// 解引用（降低一级引用层级）
    pub fn deref(&self, new_id: TokenId) -> Option<Self> {
        if self.ref_level > 0 {
            Some(Token {
                id: new_id,
                type_key: self.type_key.clone(),
                capability: self.capability,
                borrowed_from: self.borrowed_from,
                ref_level: self.ref_level - 1,
                lifetime: self.lifetime.clone(),
            })
        } else {
            None // 不能对值解引用
        }
    }
}

/// Guard 条件（用于变迁）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Guard {
    /// Guard 类型
    pub kind: GuardKind,
    /// 检查的类型
    pub type_key: TypeKey,
    /// 错误消息
    pub error_msg: String,
}

/// Guard 类型
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardKind {
    /// 检查所有权：传递 own 时，不能有 shr 或 mut
    RequireOwn,
    /// 检查共享引用：持有 shr 时，不能有 mut
    RequireShr,
    /// 检查可变借用：持有 mut 时，不能有 shr 或其他 mut
    RequireMut,
}

/// PCPN
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pcpn {
    /// 所有库所
    pub places: Vec<Place>,
    /// 所有变迁
    pub transitions: Vec<Transition>,
    /// (类型, capability) → Place 的映射
    #[serde(skip)]
    pub type_cap_to_place: HashMap<(TypeKey, Capability), PlaceId>,
    /// 初始标识 (primitive 类型的 own places)
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
            type_cap_to_place: HashMap::new(),
            initial_places: Vec::new(),
        }
    }

    /// 从 ApiGraph 转换为 PCPN（新版设计）
    ///
    /// Place 设计：每个类型 T 有三个 Place
    /// - P[T, own] 存放 T 的所有权
    /// - P[T, shr] 存放 &T 引用
    /// - P[T, mut] 存放 &mut T 可变借用
    pub fn from_api_graph(graph: &ApiGraph) -> Self {
        let mut pcpn = Pcpn::new();

        // 1. 收集所有具体类型（base types，不包含泛型参数和引用）
        let mut concrete_types: IndexSet<TypeKey> = IndexSet::new();

        for type_node in &graph.type_nodes {
            let ty = type_node.type_key.base_type();
            if !ty.contains_generic_param() {
                concrete_types.insert(ty.clone());
            }
        }

        // 2. 单态化泛型函数
        let monomorphized_fns = pcpn.monomorphize_functions(graph, &concrete_types);

        // 3. 从单态化函数中收集额外的类型
        for mono_fn in &monomorphized_fns {
            pcpn.collect_fn_all_types(
                graph,
                &graph.fn_nodes[mono_fn.fn_id],
                &mono_fn.substitutions,
                &mut concrete_types,
            );
        }

        // 4. 为每个类型创建三个 Place（own, shr, mut）
        for ty in &concrete_types {
            if !ty.contains_generic_param() {
                pcpn.create_places_for_type(ty);
            }
        }

        // 5. 创建 API 调用变迁（单态化后）
        for mono_fn in &monomorphized_fns {
            pcpn.create_api_call_transition(graph, mono_fn);
        }

        // 6. 创建结构性变迁（借用转换等）
        // 注意：所有类型都需要借用转换，包括基本类型
        for ty in &concrete_types {
            if !ty.contains_generic_param() {
                pcpn.create_structural_transitions_for_type(ty);
            }
        }

        // 7. 创建 primitive 类型的创建变迁
        // 为所有 primitive 类型的 own place 创建持续使能的生成变迁
        let primitive_places: Vec<_> = pcpn
            .places
            .iter()
            .filter(|p| p.is_primitive && p.capability == Capability::Own)
            .map(|p| (p.id, p.type_key.clone()))
            .collect();

        for (place_id, type_key) in primitive_places {
            pcpn.create_primitive_transition(&type_key, place_id);
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

    /// 收集函数涉及的所有类型（base types）
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
            let base_ty = ty.base_type();
            if !base_ty.contains_generic_param() {
                all_types.insert(base_ty.clone());
            }
        }

        for edge in graph.get_output_edges(fn_node.id) {
            let ty = graph.type_nodes[edge.type_node]
                .type_key
                .substitute(substitutions);
            let base_ty = ty.base_type();
            if !base_ty.contains_generic_param() {
                all_types.insert(base_ty.clone());
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

    /// 为类型创建三个 Place（own, shr, mut）
    fn create_places_for_type(&mut self, type_key: &TypeKey) {
        let is_primitive = type_key.is_primitive();

        // 创建 own 库所
        // 基本类型和复合类型的 budget 都是 3（上限 3 个实例）
        let own_budget = 3;
        let own_id = self.places.len();
        self.places.push(Place {
            id: own_id,
            type_key: type_key.clone(),
            capability: Capability::Own,
            is_primitive,
            budget: own_budget,
        });
        self.type_cap_to_place
            .insert((type_key.clone(), Capability::Own), own_id);

        // Primitive 类型不在初始标识中（通过 CreatePrimitive 生成）
        // 初始标识为空，所有 token 通过变迁生成

        // 创建 shr 库所
        let shr_id = self.places.len();
        self.places.push(Place {
            id: shr_id,
            type_key: type_key.clone(),
            capability: Capability::Shr,
            is_primitive,
            budget: 3,
        });
        self.type_cap_to_place
            .insert((type_key.clone(), Capability::Shr), shr_id);

        // 创建 mut 库所
        let mut_id = self.places.len();
        self.places.push(Place {
            id: mut_id,
            type_key: type_key.clone(),
            capability: Capability::Mut,
            is_primitive,
            budget: 3,
        });
        self.type_cap_to_place
            .insert((type_key.clone(), Capability::Mut), mut_id);
    }

    /// 获取类型+能力对应的 Place ID
    pub fn get_place(&self, type_key: &TypeKey, capability: Capability) -> Option<PlaceId> {
        self.type_cap_to_place
            .get(&(type_key.clone(), capability))
            .copied()
    }

    /// 获取或创建 Place
    fn get_or_create_place(&mut self, type_key: &TypeKey, capability: Capability) -> PlaceId {
        if let Some(id) = self.get_place(type_key, capability) {
            id
        } else {
            self.create_places_for_type(type_key);
            self.get_place(type_key, capability).unwrap()
        }
    }

    /// 创建 API 调用变迁（新版：使用 own/shr/mut 库所）
    ///
    /// 关键规则：
    /// - 根据 ownership 类型连接到对应库所
    /// - 添加 Guard 检查
    fn create_api_call_transition(&mut self, graph: &ApiGraph, mono_fn: &MonomorphizedFn) {
        let fn_node = &graph.fn_nodes[mono_fn.fn_id];
        let trans_id = self.transitions.len();
        let mut input_arcs = Vec::new();
        let mut output_arcs = Vec::new();
        let mut guards = Vec::new();

        // 处理输入边
        for (idx, edge) in graph.get_input_edges(fn_node.id).iter().enumerate() {
            let original_type = &graph.type_nodes[edge.type_node].type_key;
            let param_type = original_type
                .substitute(&mono_fn.substitutions)
                .base_type()
                .clone();

            if param_type.contains_generic_param() {
                continue;
            }

            // 根据 ownership 确定从哪个库所获取
            let capability = match edge.ownership {
                crate::apigraph::OwnershipType::Own => Capability::Own,
                crate::apigraph::OwnershipType::Shr => Capability::Shr,
                crate::apigraph::OwnershipType::Mut => Capability::Mut,
            };

            let place_id = self.get_or_create_place(&param_type, capability);

            // 判断是否消耗 token
            // Own: 消耗（传递所有权）
            // Shr/Mut: 不消耗（传递引用，不发生 drop），独占性由 Guard 保证
            let consumes = match capability {
                Capability::Own => true,  // 传递所有权总是消耗
                Capability::Shr => false, // 共享引用不消耗，可以多个同时持有
                Capability::Mut => false, // 可变借用不消耗，独占性由 Guard 保证
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

            // Copy 类型的 Own 参数：添加输出弧（自动复制）
            if param_type.is_copy() && capability == Capability::Own {
                output_arcs.push(Arc {
                    place_id,
                    consumes: false,
                    annotation: Some(ArcAnnotation::ReturnArc),
                });
            }

            // 添加 Guard 检查
            if capability == Capability::Own {
                // 传递所有权时，不能有引用或可变借用
                guards.push(Guard {
                    kind: GuardKind::RequireOwn,
                    type_key: param_type.clone(),
                    error_msg: format!(
                        "[ERROR] 函数 {} 需要 {} 的所有权，但可能存在引用或借用",
                        mono_fn.name,
                        param_type.short_name()
                    ),
                });
            } else if capability == Capability::Shr {
                // 持有共享引用时，不能有可变借用
                guards.push(Guard {
                    kind: GuardKind::RequireShr,
                    type_key: param_type.clone(),
                    error_msg: format!(
                        "[ERROR] 函数 {} 需要 {} 的共享引用，但可能存在可变借用",
                        mono_fn.name,
                        param_type.short_name()
                    ),
                });
            }
        }

        // 处理输出边
        for edge in graph.get_output_edges(fn_node.id) {
            let original_type = &graph.type_nodes[edge.type_node].type_key;
            let ret_type = original_type
                .substitute(&mono_fn.substitutions)
                .base_type()
                .clone();

            if ret_type.contains_generic_param() {
                continue;
            }

            let capability = match edge.ownership {
                crate::apigraph::OwnershipType::Own => Capability::Own,
                crate::apigraph::OwnershipType::Shr => Capability::Shr,
                crate::apigraph::OwnershipType::Mut => Capability::Mut,
            };

            let place_id = self.get_or_create_place(&ret_type, capability);

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
            guards,
        });
    }

    /// 为类型创建结构性变迁（新版：使用 own/shr/mut 库所）
    fn create_structural_transitions_for_type(&mut self, base_type: &TypeKey) {
        let short_name = base_type.short_name();
        let _is_copy = base_type.is_copy();

        // 获取三个库所
        let own_place = self.get_or_create_place(base_type, Capability::Own);
        let shr_place = self.get_or_create_place(base_type, Capability::Shr);
        let mut_place = self.get_or_create_place(base_type, Capability::Mut);

        // BorrowMut: P[T, own] → P[T, mut]
        // 语义：从 own 取出 token，在 mut 生成新的借用 token
        self.add_transition(
            format!("borrow_mut({})", short_name),
            TransitionKind::BorrowMut {
                base_type: base_type.clone(),
                source_token: None, // 动态确定
                borrow_token: None, // 动态生成
            },
            vec![Arc {
                place_id: own_place,
                consumes: true,
                annotation: None,
            }],
            vec![Arc {
                place_id: mut_place,
                consumes: false,
                annotation: None,
            }],
            vec![Guard {
                kind: GuardKind::RequireMut,
                type_key: base_type.clone(),
                error_msg: format!(
                    "[ERROR] 创建 {} 的可变借用时，不能有共享引用存在",
                    short_name
                ),
            }],
        );

        // EndBorrowMut: P[T, mut] → P[T, own]
        // 语义：归还 mut 库所的借用 token，恢复 own 库所的原 token
        self.add_transition(
            format!("end_borrow_mut({})", short_name),
            TransitionKind::EndBorrowMut {
                base_type: base_type.clone(),
                borrow_token: None, // 动态确定
                source_token: None, // 从 borrowed_from 恢复
            },
            vec![Arc {
                place_id: mut_place,
                consumes: true,
                annotation: None,
            }],
            vec![Arc {
                place_id: own_place,
                consumes: false,
                annotation: None,
            }],
            vec![],
        );

        // BorrowShr: P[T, own] → P[T, shr]
        self.add_transition(
            format!("borrow_shr({})", short_name),
            TransitionKind::BorrowShr {
                base_type: base_type.clone(),
                source_token: None,
                borrow_token: None,
            },
            vec![Arc {
                place_id: own_place,
                consumes: true,
                annotation: None,
            }],
            vec![Arc {
                place_id: shr_place,
                consumes: false,
                annotation: None,
            }],
            vec![Guard {
                kind: GuardKind::RequireShr,
                type_key: base_type.clone(),
                error_msg: format!("[ERROR] 创建 {} 的共享引用时，不能有可变借用", short_name),
            }],
        );

        // EndBorrowShr: P[T, shr] → P[T, own]
        self.add_transition(
            format!("end_borrow_shr({})", short_name),
            TransitionKind::EndBorrowShr {
                base_type: base_type.clone(),
                borrow_token: None,
                source_token: None,
            },
            vec![Arc {
                place_id: shr_place,
                consumes: true,
                annotation: None,
            }],
            vec![Arc {
                place_id: own_place,
                consumes: false,
                annotation: None,
            }],
            vec![],
        );

        // 注意：Rust 中不存在 mut_to_shr 降权操作
        // 可变引用和共享引用通过 borrow 和 end_borrow 管理

        // Drop: P[T, own] → ε
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
            vec![],
        );

        // 注意：删除了 dup_copy 和 dup_clone 变迁
        // Copy 类型在使用时自动复制（在 fire 函数中处理）
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
            vec![],
        );
    }

    /// 添加变迁
    fn add_transition(
        &mut self,
        name: String,
        kind: TransitionKind,
        input_arcs: Vec<Arc>,
        output_arcs: Vec<Arc>,
        guards: Vec<Guard>,
    ) {
        let id = self.transitions.len();
        self.transitions.push(Transition {
            id,
            name,
            kind,
            input_arcs,
            output_arcs,
            guards,
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
            let fillcolor = match place.capability {
                Capability::Own => {
                    if place.is_primitive {
                        "lightgray"
                    } else {
                        "lightblue"
                    }
                }
                Capability::Shr => "lightcyan",
                Capability::Mut => "mistyrose",
            };

            let peripheries = if self.initial_places.contains(&place.id) {
                2
            } else {
                1
            };
            let label = format!(
                "{}\\n[{}]\\n[B={}]",
                place.type_key.short_name(),
                place.capability,
                place.budget
            );

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
                TransitionKind::DerefRef { .. } => ("wheat", "box"),
                TransitionKind::Drop { .. } => ("gray90", "box"),
            };

            // 添加 guard 标记和 token 追踪注释
            let mut extra_info = String::new();
            if !trans.guards.is_empty() {
                extra_info.push_str(&format!("\\n[G:{}]", trans.guards.len()));
            }

            // 为借用变迁添加 token 追踪说明
            match &trans.kind {
                TransitionKind::BorrowMut { base_type, .. } => {
                    extra_info.push_str(&format!("\\n[取token_i→生成token_j]"));
                }
                TransitionKind::BorrowShr { base_type, .. } => {
                    extra_info.push_str(&format!("\\n[取token_i→生成token_j]"));
                }
                TransitionKind::EndBorrowMut { base_type, .. } => {
                    extra_info.push_str(&format!("\\n[归还token_j→恢复token_i]"));
                }
                TransitionKind::EndBorrowShr { base_type, .. } => {
                    extra_info.push_str(&format!("\\n[归还token_j→恢复token_i]"));
                }
                _ => {}
            }

            dot.push_str(&format!(
                "  t{} [label=\"{}{}\", shape={}, style=filled, fillcolor={}];\n",
                trans.id, trans.name, extra_info, shape, color
            ));
        }
        dot.push_str("\n");

        // Arcs
        dot.push_str("  // ========== Arcs ==========\n");
        for trans in &self.transitions {
            for arc in &trans.input_arcs {
                // 所有弧都显示为普通弧（solid），不再使用抑制弧（dashed/odot）
                let style = "solid";
                let arrow = "normal";
                let color = self.arc_color(arc.place_id);

                dot.push_str(&format!(
                    "  p{} -> t{} [style={}, arrowhead={}, color=\"{}\"];\n",
                    arc.place_id, trans.id, style, arrow, color
                ));
            }

            for arc in &trans.output_arcs {
                let color = self.arc_color(arc.place_id);
                // 所有输出弧都是 solid（不使用 dashed）
                let style = "solid";
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
            match place.capability {
                Capability::Own => "black",
                Capability::Shr => "blue",
                Capability::Mut => "red",
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

        let own_places = self
            .places
            .iter()
            .filter(|p| p.capability == Capability::Own)
            .count();
        let shr_places = self
            .places
            .iter()
            .filter(|p| p.capability == Capability::Shr)
            .count();
        let mut_places = self
            .places
            .iter()
            .filter(|p| p.capability == Capability::Mut)
            .count();

        PcpnStats {
            num_places: self.places.len(),
            num_base_places: own_places,
            num_ref_places: shr_places + mut_places,
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

        pcpn.create_places_for_type(&counter_type);

        // 应该创建三个 place：own, shr, mut
        assert_eq!(pcpn.places.len(), 3);
        assert!(pcpn.get_place(&counter_type, Capability::Own).is_some());
        assert!(pcpn.get_place(&counter_type, Capability::Shr).is_some());
        assert!(pcpn.get_place(&counter_type, Capability::Mut).is_some());
    }

    #[test]
    fn test_ref_type_place() {
        let mut pcpn = Pcpn::new();
        let counter_type = TypeKey::path("Counter");

        pcpn.create_places_for_type(&counter_type);

        // 检查 shr 库所的 capability
        assert_eq!(pcpn.places.len(), 3);
        let shr_place_id = pcpn.get_place(&counter_type, Capability::Shr).unwrap();
        assert_eq!(pcpn.places[shr_place_id].capability, Capability::Shr);
    }
}
