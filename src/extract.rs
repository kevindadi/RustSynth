//! 从 rustdoc JSON 提取 API 信息并构建 ApiGraph
//!
//! 提取函数签名、类型信息,并构建二分图

use std::rc::Rc;

use anyhow::Result;
use rustdoc_types::{Crate, Item, ItemEnum, Type, Visibility};

use crate::apigraph::{
    ApiEdge, ApiGraph, EdgeDirection, FunctionNode, LifetimeBinding, OwnershipType, ParamInfo,
    SelfParam,
};
use crate::lifetime_analyzer::LifetimeAnalyzer;
use crate::type_model::{PassingMode, TypeKey};

/// 从 Crate 构建 ApiGraph
pub fn build_api_graph(krate: &Crate, module_filter: &[String]) -> Result<ApiGraph> {
    let mut graph = ApiGraph::new();
    let ctx = ExtractionContext::new(krate);

    // 收集所有 impl 块中的方法 ID
    let mut impl_method_ids = std::collections::HashSet::new();
    for item in krate.index.values() {
        if let ItemEnum::Impl(impl_) = &item.inner {
            impl_method_ids.extend(impl_.items.iter().cloned());
        }
    }

    // 遍历所有 items
    for (id, item) in &krate.index {
        let should_process = is_public(item) || matches!(item.inner, ItemEnum::Impl(_));
        if !should_process {
            continue;
        }

        // 模块过滤
        if !module_filter.is_empty() {
            let path = ctx.get_item_path(id);
            if !module_filter.iter().any(|m| path.starts_with(m)) {
                continue;
            }
        }

        match &item.inner {
            ItemEnum::Function(func) => {
                // 跳过 impl 块中的方法
                if impl_method_ids.contains(id) {
                    continue;
                }

                if is_public(item) {
                    let fn_name = item.name.as_deref().unwrap_or("unknown");
                    // 创建函数级别的上下文
                    let fn_ctx = ctx.with_function_context(&func.generics, fn_name);

                    if let Some(fn_node) =
                        extract_function(&fn_ctx, item, &func.generics, &func.sig, None, &mut graph)
                    {
                        add_function_edges(&mut graph, &fn_node);
                        graph.add_function_node(fn_node);
                    }
                }
            }
            ItemEnum::Impl(impl_) => {
                // 先获取基础类型名称(不包含泛型参数)
                let base_type_name = get_base_type_name(&impl_.for_);

                // 创建一个临时上下文来提取 impl 块的泛型参数
                let temp_ctx = ctx.with_impl_context(None, &impl_.generics, &base_type_name);

                // 现在用包含泛型参数的上下文解析 Self 类型
                let self_type = temp_ctx.normalize_type(&impl_.for_);

                // 创建最终的 impl 上下文
                let impl_ctx =
                    ctx.with_impl_context(self_type.clone(), &impl_.generics, &base_type_name);

                for item_id in &impl_.items {
                    if let Some(method_item) = krate.index.get(item_id) {
                        if let ItemEnum::Function(func) = &method_item.inner {
                            if is_public(method_item) {
                                let fn_name = method_item.name.as_deref().unwrap_or("unknown");
                                // 创建方法级别的上下文(继承 impl 的泛型参数)
                                let method_ctx =
                                    impl_ctx.with_function_context(&func.generics, fn_name);

                                if let Some(fn_node) = extract_function(
                                    &method_ctx,
                                    method_item,
                                    &func.generics,
                                    &func.sig,
                                    self_type.clone(),
                                    &mut graph,
                                ) {
                                    add_function_edges(&mut graph, &fn_node);
                                    graph.add_function_node(fn_node);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(graph)
}

/// 泛型参数信息
#[derive(Clone, Debug)]
struct GenericParamInfo {
    /// 参数名
    name: String,
    /// 所属上下文(类型名或函数名)
    context: String,
    /// Trait bounds
    bounds: Vec<String>,
}

/// 提取上下文
struct ExtractionContext<'a> {
    krate: &'a Crate,
    /// 共享的 ID 到路径映射(Rc 避免重复克隆)
    id_to_path: Rc<std::collections::HashMap<rustdoc_types::Id, String>>,
    /// 当前 impl 块的 Self 类型(用于解析 Self 泛型)
    current_self_type: Option<TypeKey>,
    /// 当前上下文的泛型参数映射(参数名 -> 信息)
    generic_params: std::collections::HashMap<String, GenericParamInfo>,
    /// 当前上下文名称
    current_context: String,
}

impl<'a> ExtractionContext<'a> {
    fn new(krate: &'a Crate) -> Self {
        let mut id_to_path: std::collections::HashMap<rustdoc_types::Id, String> =
            std::collections::HashMap::new();

        // 构建 ID 到路径的映射
        for (id, summary) in &krate.paths {
            let path = summary.path.join("::");
            id_to_path.insert(id.clone(), path);
        }

        ExtractionContext {
            krate,
            id_to_path: Rc::new(id_to_path),
            current_self_type: None,
            generic_params: std::collections::HashMap::new(),
            current_context: String::new(),
        }
    }

    /// 设置当前的 Self 类型和泛型参数(进入 impl 块时调用)
    fn with_impl_context(
        &self,
        self_type: Option<TypeKey>,
        generics: &rustdoc_types::Generics,
        context_name: &str,
    ) -> Self {
        let mut ctx = ExtractionContext {
            krate: self.krate,
            id_to_path: Rc::clone(&self.id_to_path),
            current_self_type: self_type,
            generic_params: std::collections::HashMap::new(),
            current_context: context_name.to_string(),
        };
        ctx.extract_generics(generics, context_name);
        ctx
    }

    /// 创建函数级别的上下文(继承 impl 的泛型参数)
    fn with_function_context(&self, generics: &rustdoc_types::Generics, fn_name: &str) -> Self {
        let context = if self.current_context.is_empty() {
            fn_name.to_string()
        } else {
            format!("{}::{}", self.current_context, fn_name)
        };

        let mut ctx = ExtractionContext {
            krate: self.krate,
            id_to_path: Rc::clone(&self.id_to_path),
            current_self_type: self.current_self_type.clone(),
            generic_params: self.generic_params.clone(), // 继承 impl 的泛型参数
            current_context: context.clone(),
        };
        ctx.extract_generics(generics, &context);
        ctx
    }

    /// 从 Generics 提取泛型参数信息
    fn extract_generics(&mut self, generics: &rustdoc_types::Generics, context: &str) {
        // 1. 从 params 提取
        for param in &generics.params {
            if let rustdoc_types::GenericParamDefKind::Type { bounds, .. } = &param.kind {
                let bound_names: Vec<String> = bounds
                    .iter()
                    .filter_map(|b| self.extract_trait_bound(b))
                    .collect();

                self.generic_params.insert(
                    param.name.clone(),
                    GenericParamInfo {
                        name: param.name.clone(),
                        context: context.to_string(),
                        bounds: bound_names,
                    },
                );
            }
        }

        // 2. 从 where_predicates 补充 bounds
        for predicate in &generics.where_predicates {
            if let rustdoc_types::WherePredicate::BoundPredicate {
                type_: Type::Generic(name),
                bounds,
                ..
            } = predicate
            {
                let additional_bounds: Vec<String> = bounds
                    .iter()
                    .filter_map(|b| self.extract_trait_bound(b))
                    .collect();

                if let Some(info) = self.generic_params.get_mut(name) {
                    for bound in additional_bounds {
                        if !info.bounds.contains(&bound) {
                            info.bounds.push(bound);
                        }
                    }
                }
            }
        }
    }

    /// 提取 trait bound 名称
    fn extract_trait_bound(&self, bound: &rustdoc_types::GenericBound) -> Option<String> {
        match bound {
            rustdoc_types::GenericBound::TraitBound { trait_, .. } => {
                // 获取 trait 路径
                let trait_path = self
                    .id_to_path
                    .get(&trait_.id)
                    .cloned()
                    .unwrap_or_else(|| trait_.path.clone());
                // 只取最后一段
                Some(
                    trait_path
                        .split("::")
                        .last()
                        .unwrap_or(&trait_path)
                        .to_string(),
                )
            }
            rustdoc_types::GenericBound::Outlives(_) => None,
            rustdoc_types::GenericBound::Use(_) => None,
        }
    }

    fn get_item_path(&self, id: &rustdoc_types::Id) -> String {
        self.id_to_path
            .get(id)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn normalize_type(&self, ty: &Type) -> Option<TypeKey> {
        match ty {
            Type::Primitive(name) => Some(TypeKey::primitive(name)),

            Type::ResolvedPath(path) => {
                let base_path = self
                    .id_to_path
                    .get(&path.id)
                    .cloned()
                    .unwrap_or_else(|| path.path.clone());

                let args = if let Some(ref generic_args) = path.args {
                    match generic_args.as_ref() {
                        rustdoc_types::GenericArgs::AngleBracketed { args, .. } => args
                            .iter()
                            .filter_map(|arg| {
                                if let rustdoc_types::GenericArg::Type(t) = arg {
                                    self.normalize_type(t)
                                } else {
                                    None
                                }
                            })
                            .collect(),
                        _ => vec![],
                    }
                } else {
                    vec![]
                };

                Some(TypeKey::path_with_args(&base_path, args))
            }

            Type::BorrowedRef {
                is_mutable, type_, ..
            } => {
                let inner = self.normalize_type(type_)?;
                if *is_mutable {
                    Some(TypeKey::ref_mut(inner))
                } else {
                    Some(TypeKey::ref_shr(inner))
                }
            }

            Type::Tuple(elems) => {
                if elems.is_empty() {
                    Some(TypeKey::unit())
                } else {
                    let elem_types: Vec<_> = elems
                        .iter()
                        .filter_map(|e| self.normalize_type(e))
                        .collect();
                    Some(TypeKey::Tuple(elem_types))
                }
            }

            Type::Slice(inner) => {
                let inner_type = self.normalize_type(inner)?;
                Some(TypeKey::Slice(Box::new(inner_type)))
            }

            Type::Array { type_, len } => {
                let elem_type = self.normalize_type(type_)?;
                let len_val = len.parse().unwrap_or(0);
                Some(TypeKey::Array {
                    elem: Box::new(elem_type),
                    len: len_val,
                })
            }

            Type::RawPointer { is_mutable, type_ } => {
                let inner = self.normalize_type(type_)?;
                Some(TypeKey::RawPtr {
                    mutable: *is_mutable,
                    inner: Box::new(inner),
                })
            }

            Type::Generic(name) => {
                // Self 替换为实际的 Self 类型
                if name == "Self" || name == "self" {
                    self.current_self_type.clone()
                } else if let Some(info) = self.generic_params.get(name) {
                    // 已知的泛型参数,使用 GenericParam
                    Some(TypeKey::GenericParam {
                        context: info.context.clone(),
                        name: info.name.clone(),
                        bounds: info.bounds.clone(),
                    })
                } else {
                    // 未知的泛型参数(可能来自外部),创建无 bounds 的 GenericParam
                    Some(TypeKey::GenericParam {
                        context: self.current_context.clone(),
                        name: name.clone(),
                        bounds: vec![],
                    })
                }
            }

            // 关联类型:<T as Trait>::Item
            Type::QualifiedPath {
                name,
                self_type,
                trait_,
                ..
            } => {
                // 构建关联类型路径
                let self_str = self
                    .normalize_type(self_type)
                    .map(|t| t.short_name())
                    .unwrap_or_else(|| "?".to_string());

                let trait_str = trait_
                    .as_ref()
                    .map(|t| {
                        self.id_to_path
                            .get(&t.id)
                            .cloned()
                            .unwrap_or_else(|| t.path.clone())
                    })
                    .unwrap_or_default();

                // 格式: <Self as Trait>::Item
                let assoc_path = if trait_str.is_empty() {
                    format!("{}::{}", self_str, name)
                } else {
                    format!("<{} as {}>::{}", self_str, trait_str, name)
                };

                Some(TypeKey::AssociatedType(assoc_path))
            }

            _ => Some(TypeKey::Unknown(format!("{:?}", ty))),
        }
    }
}

/// 检查 Item 是否公开
fn is_public(item: &Item) -> bool {
    matches!(item.visibility, Visibility::Public)
}

/// 获取类型的基础名称(不包含泛型参数)
fn get_base_type_name(ty: &Type) -> String {
    match ty {
        Type::ResolvedPath(path) => {
            // 只取路径的最后一段
            path.path
                .split("::")
                .last()
                .unwrap_or(&path.path)
                .to_string()
        }
        Type::Primitive(name) => name.clone(),
        Type::Generic(name) => name.clone(),
        _ => "Unknown".to_string(),
    }
}

/// 提取函数信息
fn extract_function(
    ctx: &ExtractionContext,
    item: &Item,
    generics: &rustdoc_types::Generics,
    sig: &rustdoc_types::FunctionSignature,
    impl_self_type: Option<TypeKey>,
    _graph: &mut ApiGraph,
) -> Option<FunctionNode> {
    let name = item.name.clone()?;
    let path = if let Some(self_type) = &impl_self_type {
        format!("{}::{}", self_type.short_name(), name)
    } else {
        name.clone()
    };

    let mut params = Vec::new();
    let mut self_param = None;
    let mut is_method = false;

    for (param_name, ty) in &sig.inputs {
        if param_name == "self" {
            is_method = true;
            if let Some(self_type) = &impl_self_type {
                let (base_type, passing_mode) = parse_self_param(ty, self_type);
                self_param = Some(SelfParam {
                    base_type,
                    passing_mode,
                });
            }
        } else {
            if let Some(type_key) = ctx.normalize_type(ty) {
                let (base_type, passing_mode) = decompose_type(&type_key);
                params.push(ParamInfo {
                    name: param_name.clone(),
                    base_type,
                    passing_mode,
                });
            }
        }
    }

    let (return_type, return_mode) = if let Some(output) = &sig.output {
        if let Some(type_key) = ctx.normalize_type(output) {
            if type_key == TypeKey::unit() {
                (None, None)
            } else {
                let (base, mode) = decompose_return_type(&type_key);
                (Some(base), Some(mode))
            }
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    let is_entry = self_param.is_none()
        && params
            .iter()
            .all(|p| p.base_type.is_primitive() || matches!(p.passing_mode, PassingMode::Copy));

    let is_const = sig.is_c_variadic == false && check_const_fn(item);
    let is_zero_ary = self_param.is_none() && params.is_empty();
    let is_const_producer = is_const && is_zero_ary && return_type.is_some();

    let lifetime_bindings = extract_lifetime_bindings(generics, sig, &return_mode);

    Some(FunctionNode {
        id: 0,
        path,
        name,
        is_method,
        is_entry,
        is_const,
        is_const_producer,
        params,
        self_param,
        return_type,
        return_mode,
        lifetime_bindings,
    })
}

fn check_const_fn(item: &Item) -> bool {
    if let ItemEnum::Function(func) = &item.inner {
        func.sig.is_c_variadic == false
    } else {
        false
    }
}

/// 提取所有生命周期绑定信息(支持多个生命周期)
fn extract_lifetime_bindings(
    generics: &rustdoc_types::Generics,
    sig: &rustdoc_types::FunctionSignature,
    return_mode: &Option<PassingMode>,
) -> Vec<LifetimeBinding> {
    // 只有返回引用的函数才需要生命周期绑定
    if !matches!(
        return_mode,
        Some(PassingMode::ReturnBorrowShr) | Some(PassingMode::ReturnBorrowMut)
    ) {
        return vec![];
    }

    let is_shared = matches!(return_mode, Some(PassingMode::ReturnBorrowShr));

    // 使用 LifetimeAnalyzer 分析函数签名
    let analysis = LifetimeAnalyzer::analyze(generics, sig);

    // 如果分析结果包含生命周期绑定,使用所有绑定
    if !analysis.lifetime_bindings.is_empty() {
        return analysis
            .lifetime_bindings
            .iter()
            .map(|binding| LifetimeBinding {
                lifetime: binding.lifetime.clone(),
                source_param_index: binding.bound_to_param,
                is_shared,
            })
            .collect();
    }

    // 回退到 Rust 的生命周期省略规则:
    // 规则1: 如果只有一个输入生命周期参数(包括 &self),所有省略的生命周期绑定到该参数
    // 规则2: 如果有 &self 或 &mut self,省略的返回生命周期绑定到 self
    let has_self_ref = sig
        .inputs
        .iter()
        .any(|(name, ty)| name == "self" && matches!(ty, rustdoc_types::Type::BorrowedRef { .. }));

    if has_self_ref {
        // 规则2: 绑定到 self
        return vec![LifetimeBinding {
            lifetime: "'_".to_string(),
            source_param_index: 0,
            is_shared,
        }];
    }

    // 规则1: 统计引用参数数量
    let ref_params: Vec<usize> = sig
        .inputs
        .iter()
        .enumerate()
        .filter(|(_, (_, ty))| matches!(ty, rustdoc_types::Type::BorrowedRef { .. }))
        .map(|(i, _)| i)
        .collect();

    if ref_params.len() == 1 {
        return vec![LifetimeBinding {
            lifetime: "'_".to_string(),
            source_param_index: ref_params[0],
            is_shared,
        }];
    }

    // 无法确定绑定关系,保守地绑定到第一个参数
    if !sig.inputs.is_empty() {
        return vec![LifetimeBinding {
            lifetime: "'_".to_string(),
            source_param_index: 0,
            is_shared,
        }];
    }

    vec![]
}

/// 解析 self 参数
fn parse_self_param(ty: &Type, impl_self_type: &TypeKey) -> (TypeKey, PassingMode) {
    match ty {
        Type::BorrowedRef { is_mutable, .. } => {
            if *is_mutable {
                (impl_self_type.clone(), PassingMode::BorrowMut)
            } else {
                (impl_self_type.clone(), PassingMode::BorrowShr)
            }
        }
        _ => {
            // self by value
            if impl_self_type.is_copy() {
                (impl_self_type.clone(), PassingMode::Copy)
            } else {
                (impl_self_type.clone(), PassingMode::Move)
            }
        }
    }
}

/// 分解类型为 base type + passing mode
fn decompose_type(type_key: &TypeKey) -> (TypeKey, PassingMode) {
    match type_key {
        TypeKey::RefShr(inner) => (inner.as_ref().clone(), PassingMode::BorrowShr),
        TypeKey::RefMut(inner) => (inner.as_ref().clone(), PassingMode::BorrowMut),
        _ => {
            if type_key.is_copy() {
                (type_key.clone(), PassingMode::Copy)
            } else {
                (type_key.clone(), PassingMode::Move)
            }
        }
    }
}

/// 分解返回类型
fn decompose_return_type(type_key: &TypeKey) -> (TypeKey, PassingMode) {
    match type_key {
        TypeKey::RefShr(inner) => (inner.as_ref().clone(), PassingMode::ReturnBorrowShr),
        TypeKey::RefMut(inner) => (inner.as_ref().clone(), PassingMode::ReturnBorrowMut),
        _ => (type_key.clone(), PassingMode::ReturnOwned),
    }
}

/// 将 PassingMode 转换为 OwnershipType
fn passing_mode_to_ownership(mode: &PassingMode) -> OwnershipType {
    match mode {
        PassingMode::Move | PassingMode::Copy | PassingMode::ReturnOwned => OwnershipType::Own,
        PassingMode::BorrowShr | PassingMode::ReturnBorrowShr => OwnershipType::Shr,
        PassingMode::BorrowMut | PassingMode::ReturnBorrowMut => OwnershipType::Mut,
    }
}

/// 检查是否需要解引用
fn requires_deref(mode: &PassingMode) -> bool {
    // 当需要 own 但当前持有引用时,需要解引用
    matches!(mode, PassingMode::Move) && false // 简化版:暂不检测
}

/// 为函数添加边
fn add_function_edges(graph: &mut ApiGraph, fn_node: &FunctionNode) {
    let fn_id = graph.fn_nodes.len(); // 下一个 ID

    // Self 参数边
    if let Some(ref self_param) = fn_node.self_param {
        let type_id = graph.get_or_create_type_node(self_param.base_type.clone());
        let ownership = passing_mode_to_ownership(&self_param.passing_mode);
        let requires_deref = requires_deref(&self_param.passing_mode);

        graph.add_edge(ApiEdge {
            fn_node: fn_id,
            type_node: type_id,
            direction: EdgeDirection::Input,
            passing_mode: self_param.passing_mode.clone(),
            ownership,
            requires_deref,
            param_index: Some(0),
            lifetime: None,
        });
    }

    // 参数边
    for (i, param) in fn_node.params.iter().enumerate() {
        let type_id = graph.get_or_create_type_node(param.base_type.clone());
        let param_idx = if fn_node.self_param.is_some() {
            i + 1
        } else {
            i
        };
        let ownership = passing_mode_to_ownership(&param.passing_mode);
        let requires_deref = requires_deref(&param.passing_mode);

        graph.add_edge(ApiEdge {
            fn_node: fn_id,
            type_node: type_id,
            direction: EdgeDirection::Input,
            passing_mode: param.passing_mode.clone(),
            ownership,
            requires_deref,
            param_index: Some(param_idx),
            lifetime: None,
        });
    }

    // 返回值边
    if let (Some(return_type), Some(return_mode)) = (&fn_node.return_type, &fn_node.return_mode) {
        let type_id = graph.get_or_create_type_node(return_type.clone());
        let ownership = passing_mode_to_ownership(return_mode);

        graph.add_edge(ApiEdge {
            fn_node: fn_id,
            type_node: type_id,
            direction: EdgeDirection::Output,
            passing_mode: return_mode.clone(),
            ownership,
            requires_deref: false, // 返回值不需要解引用
            param_index: None,
            lifetime: None,
        });
    }
}
