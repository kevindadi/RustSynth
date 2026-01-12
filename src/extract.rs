//! 从 rustdoc JSON 提取 API 信息并构建 ApiGraph
//!
//! 提取函数签名、类型信息，并构建二分图

use anyhow::{Context, Result};
use rustdoc_types::{Crate, Item, ItemEnum, Type, Visibility};

use crate::apigraph::{
    ApiEdge, ApiGraph, EdgeDirection, FunctionNode, ParamInfo, SelfParam,
};
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
                    if let Some(fn_node) =
                        extract_function(&ctx, item, &func.sig, None, &mut graph)
                    {
                        add_function_edges(&mut graph, &fn_node);
                        graph.add_function_node(fn_node);
                    }
                }
            }
            ItemEnum::Impl(impl_) => {
                // 获取 Self 类型
                let self_type = ctx.normalize_type(&impl_.for_);

                for item_id in &impl_.items {
                    if let Some(method_item) = krate.index.get(item_id) {
                        if let ItemEnum::Function(func) = &method_item.inner {
                            if is_public(method_item) {
                                if let Some(fn_node) = extract_function(
                                    &ctx,
                                    method_item,
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

/// 提取上下文
struct ExtractionContext<'a> {
    krate: &'a Crate,
    id_to_path: std::collections::HashMap<rustdoc_types::Id, String>,
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

        ExtractionContext { krate, id_to_path }
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
                    let elem_types: Vec<_> =
                        elems.iter().filter_map(|e| self.normalize_type(e)).collect();
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
                // 泛型参数作为 Unknown 处理（后续单态化会替换）
                Some(TypeKey::Unknown(name.clone()))
            }

            _ => Some(TypeKey::Unknown(format!("{:?}", ty))),
        }
    }
}

/// 检查 Item 是否公开
fn is_public(item: &Item) -> bool {
    matches!(item.visibility, Visibility::Public)
}

/// 提取函数信息
fn extract_function(
    ctx: &ExtractionContext,
    item: &Item,
    sig: &rustdoc_types::FunctionSignature,
    impl_self_type: Option<TypeKey>,
    graph: &mut ApiGraph,
) -> Option<FunctionNode> {
    let name = item.name.clone()?;
    let path = format!(
        "{}::{}",
        impl_self_type
            .as_ref()
            .map(|t| t.short_name())
            .unwrap_or_default(),
        name
    );

    let mut params = Vec::new();
    let mut self_param = None;
    let mut is_method = false;

    for (param_name, ty) in &sig.inputs {
        if param_name == "self" {
            is_method = true;
            // 解析 self 参数
            if let Some(self_type) = &impl_self_type {
                let (base_type, passing_mode) = parse_self_param(ty, self_type, ctx);
                self_param = Some(SelfParam {
                    base_type,
                    passing_mode,
                });
            }
        } else {
            // 普通参数
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

    // 解析返回类型
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

    // 判断是否是入口函数
    let is_entry = self_param.is_none()
        && params
            .iter()
            .all(|p| p.base_type.is_primitive() || matches!(p.passing_mode, PassingMode::Copy));

    Some(FunctionNode {
        id: 0, // 会在 add_function_node 中设置
        path,
        name,
        is_method,
        is_entry,
        params,
        self_param,
        return_type,
        return_mode,
    })
}

/// 解析 self 参数
fn parse_self_param(
    ty: &Type,
    impl_self_type: &TypeKey,
    _ctx: &ExtractionContext,
) -> (TypeKey, PassingMode) {
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

/// 为函数添加边
fn add_function_edges(graph: &mut ApiGraph, fn_node: &FunctionNode) {
    let fn_id = graph.fn_nodes.len(); // 下一个 ID

    // Self 参数边
    if let Some(ref self_param) = fn_node.self_param {
        let type_id = graph.get_or_create_type_node(self_param.base_type.clone());
        graph.add_edge(ApiEdge {
            fn_node: fn_id,
            type_node: type_id,
            direction: EdgeDirection::Input,
            passing_mode: self_param.passing_mode.clone(),
            param_index: Some(0),
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
        graph.add_edge(ApiEdge {
            fn_node: fn_id,
            type_node: type_id,
            direction: EdgeDirection::Input,
            passing_mode: param.passing_mode.clone(),
            param_index: Some(param_idx),
        });
    }

    // 返回值边
    if let (Some(return_type), Some(return_mode)) = (&fn_node.return_type, &fn_node.return_mode) {
        let type_id = graph.get_or_create_type_node(return_type.clone());
        graph.add_edge(ApiEdge {
            fn_node: fn_id,
            type_node: type_id,
            direction: EdgeDirection::Output,
            passing_mode: return_mode.clone(),
            param_index: None,
        });
    }
}
