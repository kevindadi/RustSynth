use petgraph::graph::NodeIndex;
use rustdoc_types::{GenericParamDefKind, Generics, Id, ItemEnum, Type};

use super::builder::IrGraphBuilder;
use super::structure::{EdgeMode, NodeType, TypeRelation};
use crate::support_types::method_blacklist::is_blacklisted_method;

impl<'ir> IrGraphBuilder<'ir> {
    /// 构建类型实现的方法节点
    pub fn build_impl_methods(&mut self) {
        use log::debug;
        debug!(
            "开始构建 impl 方法, TypeCache 中节点数: {}",
            self.type_cache.total_count()
        );

        let type_impls = self.type_impls.clone();

        for (&type_id, method_ids) in &type_impls {
            for &method_id in method_ids {
                if let Some(method_item) = self.parsed.crate_data.index.get(&method_id) {
                    if let ItemEnum::Function(func) = &method_item.inner {
                        let method_name = method_item.name.as_deref().unwrap_or("unknown");

                        // 过滤黑名单方法
                        if is_blacklisted_method(method_name) {
                            continue;
                        }

                        // 创建操作节点
                        let op_node_idx = self.graph.add_type_node(method_name);
                        self.op_node_maps.insert(method_id, op_node_idx);
                        self.graph
                            .node_types
                            .insert(op_node_idx, NodeType::ImplMethod);

                        // 处理方法的泛型参数及约束
                        self.process_method_generics(method_id, &func.generics, method_name);

                        // 处理输入参数（识别 self 并连接到类型）
                        self.process_function_inputs_with_self(
                            op_node_idx,
                            &func.sig.inputs,
                            method_name,
                            Some(type_id),
                        );

                        // 处理返回值
                        if let Some(output) = &func.sig.output {
                            self.process_function_output(
                                op_node_idx,
                                output,
                                method_name,
                                Some(type_id),
                            );
                        }
                    }
                }
            }
        }
    }

    /// 构建 Trait 定义的方法节点
    /// 这些是 trait 本身定义的方法，而不是实现
    pub fn build_trait_defined_methods(&mut self) {
        let method_impls = self.method_impls.clone();

        for (&trait_id, method_ids) in &method_impls {
            for &method_id in method_ids {
                if let Some(method_item) = self.parsed.crate_data.index.get(&method_id) {
                    if let ItemEnum::Function(func) = &method_item.inner {
                        let method_name = method_item.name.as_deref().unwrap_or("unknown");

                        // 过滤黑名单方法
                        if is_blacklisted_method(method_name) {
                            continue;
                        }

                        // 创建操作节点（trait 定义的方法）
                        let op_node_idx = self.graph.add_type_node(method_name);
                        self.op_node_maps.insert(method_id, op_node_idx);
                        self.graph
                            .node_types
                            .insert(op_node_idx, NodeType::TraitMethod);

                        // 处理方法的泛型参数
                        // 优先使用归一化的 Trait 级别泛型，如果没有才创建方法级别的
                        if !func.generics.params.is_empty() {
                            // 获取 trait 名称用于查找归一化的泛型
                            let trait_name = self
                                .parsed
                                .crate_data
                                .index
                                .get(&trait_id)
                                .and_then(|item| item.name.as_deref())
                                .unwrap_or("unknown");

                            self.process_method_generics_with_trait(
                                method_id,
                                &func.generics,
                                method_name,
                                trait_name,
                            );
                        }

                        // 处理输入参数（识别 self 并连接到 trait）
                        self.process_function_inputs_with_self(
                            op_node_idx,
                            &func.sig.inputs,
                            method_name,
                            Some(trait_id),
                        );

                        // 处理返回值
                        if let Some(output) = &func.sig.output {
                            self.process_function_output(
                                op_node_idx,
                                output,
                                method_name,
                                Some(trait_id),
                            );
                        }
                    }
                }
            }
        }
    }

    /// 处理 Trait 方法的泛型参数（优先使用归一化的泛型）
    fn process_method_generics_with_trait(
        &mut self,
        method_id: Id,
        generics: &Generics,
        method_name: &str,
        trait_name: &str,
    ) {
        use log::debug;
        use rustdoc_types::GenericBound;

        for param in &generics.params {
            if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                // 检查是否存在归一化的泛型节点
                let normalized_key = format!("{}:{}", trait_name, param.name);

                if self.generic_node_maps.contains_key(&normalized_key) {
                    // 使用归一化的泛型节点（已在 Trait 级别创建）
                    debug!(
                        "使用归一化泛型: {} (来自 Trait {})",
                        normalized_key, trait_name
                    );
                    continue;
                }

                // 否则，为此方法创建独立的泛型节点
                let generic_name = format!("{}:{}", method_name, param.name);
                let generic_node_idx = self.graph.add_type_node(&generic_name);
                self.graph
                    .node_types
                    .insert(generic_node_idx, NodeType::Generic);
                self.generic_node_maps
                    .insert(param.name.clone(), generic_node_idx);

                debug!("创建方法独立泛型: {}", generic_name);

                // 处理 Trait 约束
                for bound in bounds {
                    if let GenericBound::TraitBound { trait_, .. } = bound {
                        let trait_id = trait_.id;
                        if let Some(&trait_node) = self.type_node_maps.get(&trait_id) {
                            self.graph.add_type_relation(
                                generic_node_idx,
                                trait_node,
                                EdgeMode::Require,
                                Some(format!("{} requires", param.name)),
                            );
                            debug!("泛型约束: {} requires trait {}", param.name, trait_id.0);
                        }
                    }
                }
            }
        }
    }

    /// 处理方法的泛型参数及约束
    fn process_method_generics(&mut self, method_id: Id, generics: &Generics, method_name: &str) {
        use log::debug;
        use rustdoc_types::GenericBound;

        for param in &generics.params {
            if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                // 创建泛型参数节点
                let generic_name = format!("{}:{}", method_name, param.name);
                let generic_node_idx = self.graph.add_type_node(&generic_name);
                self.graph
                    .node_types
                    .insert(generic_node_idx, NodeType::Generic);

                // 插入两个 key：完整名和短名
                // 1. 完整名（MethodName:GenericName）- 用于精确查找
                self.generic_node_maps
                    .insert(generic_name.clone(), generic_node_idx);
                // 2. 短名（GenericName）- 用于简单查找
                self.generic_node_maps
                    .insert(param.name.clone(), generic_node_idx);

                debug!(
                    "创建方法泛型参数: {} (存储为 {} 和 {})",
                    generic_name, generic_name, param.name
                );

                // 处理 Trait 约束
                for bound in bounds {
                    if let GenericBound::TraitBound { trait_, .. } = bound {
                        let trait_id = trait_.id;

                        // 获取或创建 Trait 节点
                        let trait_node = if let Some(&node) = self.type_node_maps.get(&trait_id) {
                            node
                        } else {
                            // 外部 Trait，创建节点
                            let trait_name = trait_.path.split("::").last().unwrap_or(&trait_.path);
                            let node = self.graph.add_type_node(trait_name);
                            self.graph.node_types.insert(node, NodeType::Trait);
                            self.type_node_maps.insert(trait_id, node);

                            debug!("创建外部 Trait 节点: {} (id: {})", trait_name, trait_id.0);
                            node
                        };

                        // 创建 Require 边
                        self.graph.add_type_relation(
                            generic_node_idx,
                            trait_node,
                            EdgeMode::Require,
                            Some(format!("{} requires", param.name)),
                        );
                        debug!("泛型约束: {} requires trait {}", param.name, trait_.path);
                    }
                }
            }
        }
    }

    /// 处理函数输入参数（识别 self 并连接到类型/Trait）
    fn process_function_inputs_with_self(
        &mut self,
        op_node_idx: NodeIndex,
        inputs: &[(String, Type)],
        method_name: &str,
        owner_id: Option<Id>, // 类型或 Trait 的 ID
    ) {
        use log::debug;

        for (param_name, param_type) in inputs {
            // 识别 self 参数
            if param_name == "self" {
                if let Some(owner_id) = owner_id {
                    if let Some(&owner_node) = self.type_node_maps.get(&owner_id) {
                        // 根据类型确定 EdgeMode
                        let mode = match param_type {
                            Type::BorrowedRef { is_mutable, .. } => {
                                if *is_mutable {
                                    EdgeMode::MutRef
                                } else {
                                    EdgeMode::Ref
                                }
                            }
                            _ => EdgeMode::Move, // self 或者其他
                        };

                        // 创建从类型/Trait 到方法的边
                        self.graph.type_graph.add_edge(
                            owner_node,
                            op_node_idx,
                            TypeRelation {
                                mode,
                                label: Some("self".to_string()),
                            },
                        );
                        debug!(
                            "连接方法 {} 到所属类型/Trait (mode: {:?})",
                            method_name, mode
                        );
                        continue;
                    }
                }
            }

            // 处理其他参数
            if let Some(type_node_idx) =
                self.resolve_type_node_with_owner(param_type, method_name, owner_id)
            {
                // 根据参数类型确定 EdgeMode
                let mode = match param_type {
                    Type::BorrowedRef { is_mutable, .. } => {
                        if *is_mutable {
                            EdgeMode::MutRef
                        } else {
                            EdgeMode::Ref
                        }
                    }
                    Type::RawPointer { is_mutable, .. } => {
                        if *is_mutable {
                            EdgeMode::MutPtr
                        } else {
                            EdgeMode::Ptr
                        }
                    }
                    _ => EdgeMode::Move,
                };

                // 创建从类型到操作的边（输入边）
                self.graph.type_graph.add_edge(
                    type_node_idx,
                    op_node_idx,
                    TypeRelation {
                        mode,
                        label: Some(param_name.clone()),
                    },
                );

                debug!(
                    "方法 {} 参数: {} -> type_node (mode: {:?})",
                    method_name, param_name, mode
                );
            }
        }
    }

    /// 处理函数输入参数
    fn process_function_inputs(
        &mut self,
        op_node_idx: NodeIndex,
        inputs: &[(String, Type)],
        method_name: &str,
    ) {
        for (param_name, param_type) in inputs {
            if let Some(type_node_idx) = self.resolve_type_node(param_type, method_name) {
                // 创建从类型到操作的边（输入边）
                self.graph.type_graph.add_edge(
                    type_node_idx,
                    op_node_idx,
                    TypeRelation {
                        mode: EdgeMode::Move,
                        label: Some(param_name.clone()),
                    },
                );
            }
        }
    }

    /// 处理函数返回值
    fn process_function_output(
        &mut self,
        op_node_idx: NodeIndex,
        output: &Type,
        method_name: &str,
        owner_id: Option<Id>,
    ) {
        // 检查是否是 Result<T, E>
        if let Some((ok_type, err_type)) = self.extract_result_types(output) {
            // 创建 Result 展开节点
            let unwrap_node = self.create_unwrap_node(method_name, "unwrap");

            // op -> unwrap
            self.graph.type_graph.add_edge(
                op_node_idx,
                unwrap_node,
                TypeRelation {
                    mode: EdgeMode::Move,
                    label: Some("Result".to_string()),
                },
            );

            // unwrap -> Ok(T)
            if let Some(ok_node) =
                self.resolve_type_node_with_owner(&ok_type, method_name, owner_id)
            {
                self.graph.type_graph.add_edge(
                    unwrap_node,
                    ok_node,
                    TypeRelation {
                        mode: EdgeMode::Move,
                        label: Some("Ok".to_string()),
                    },
                );
            }

            // unwrap -> Err(E)
            if let Some(err_node) =
                self.resolve_type_node_with_owner(&err_type, method_name, owner_id)
            {
                self.graph.type_graph.add_edge(
                    unwrap_node,
                    err_node,
                    TypeRelation {
                        mode: EdgeMode::Move,
                        label: Some("Err".to_string()),
                    },
                );
            }
        }
        // 检查是否是 Option<T>
        else if let Some(some_type) = self.extract_option_type(output) {
            // 创建 Option 展开节点
            let unwrap_node = self.create_unwrap_node(method_name, "unwrap_option");

            // op -> unwrap
            self.graph.type_graph.add_edge(
                op_node_idx,
                unwrap_node,
                TypeRelation {
                    mode: EdgeMode::Move,
                    label: Some("Option".to_string()),
                },
            );

            // unwrap -> Some(T)
            if let Some(some_node) =
                self.resolve_type_node_with_owner(&some_type, method_name, owner_id)
            {
                self.graph.type_graph.add_edge(
                    unwrap_node,
                    some_node,
                    TypeRelation {
                        mode: EdgeMode::Move,
                        label: Some("Some".to_string()),
                    },
                );
            }

            // unwrap -> None (unit type)
            let none_node = self.get_or_create_primitive_node("()");
            self.graph.type_graph.add_edge(
                unwrap_node,
                none_node,
                TypeRelation {
                    mode: EdgeMode::Move,
                    label: Some("None".to_string()),
                },
            );
        }
        // 普通返回类型
        else {
            if let Some(type_node_idx) =
                self.resolve_type_node_with_owner(output, method_name, owner_id)
            {
                // 创建从操作到类型的边（输出边）
                self.graph.type_graph.add_edge(
                    op_node_idx,
                    type_node_idx,
                    TypeRelation {
                        mode: EdgeMode::Move,
                        label: None,
                    },
                );
            }
        }
    }

    /// 解析类型节点
    /// context_owner_id: 当前上下文的所有者 ID（类型或 Trait），用于解析 Self 和泛型
    fn resolve_type_node_with_owner(
        &mut self,
        ty: &Type,
        context_name: &str,
        context_owner_id: Option<Id>,
    ) -> Option<NodeIndex> {
        match ty {
            // ResolvedPath: 已解析的路径类型（struct, enum, trait 等）
            Type::ResolvedPath(path) => {
                use super::type_cache::TypeContext;
                use log::debug;

                // 使用 TypeCache 创建 TypeKey（会处理泛型参数）
                let context = TypeContext {
                    current_owner: context_owner_id,
                    generic_scopes: Default::default(), // 简化处理
                };

                if let Some(type_key) = self.type_cache.create_type_key(ty, &context) {
                    // 先尝试从 TypeCache 查找
                    if let Some(node) = self.type_cache.get_node(&type_key) {
                        return Some(node);
                    }

                    // 如果不存在，创建节点
                    let type_name = if let Some(args) = &path.args {
                        // 有泛型参数，创建带参数的类型名
                        if let rustdoc_types::GenericArgs::AngleBracketed { args, .. } = &**args {
                            let arg_names: Vec<String> = args
                                .iter()
                                .filter_map(|arg| {
                                    if let rustdoc_types::GenericArg::Type(arg_type) = arg {
                                        // 简化：只取类型名
                                        match arg_type {
                                            Type::Primitive(name) => Some(name.clone()),
                                            Type::ResolvedPath(p) => Some(
                                                p.path
                                                    .split("::")
                                                    .last()
                                                    .unwrap_or(&p.path)
                                                    .to_string(),
                                            ),
                                            Type::Generic(name) => Some(name.clone()),
                                            _ => Some("?".to_string()),
                                        }
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            if !arg_names.is_empty() {
                                format!(
                                    "{}<{}>",
                                    path.path.split("::").last().unwrap_or(&path.path),
                                    arg_names.join(", ")
                                )
                            } else {
                                path.path
                                    .split("::")
                                    .last()
                                    .unwrap_or(&path.path)
                                    .to_string()
                            }
                        } else {
                            path.path
                                .split("::")
                                .last()
                                .unwrap_or(&path.path)
                                .to_string()
                        }
                    } else {
                        path.path
                            .split("::")
                            .last()
                            .unwrap_or(&path.path)
                            .to_string()
                    };

                    let node = self.graph.add_type_node(&type_name);

                    // 根据名称推断类型
                    let node_type = if type_name.chars().next().map_or(false, |c| c.is_uppercase())
                    {
                        NodeType::Struct
                    } else {
                        NodeType::TypeAlias
                    };

                    self.graph.node_types.insert(node, node_type);

                    // 更新 TypeCache
                    self.type_cache.insert_node(type_key, node);

                    // 也更新 type_node_maps（用于快速查找基础类型）
                    self.type_node_maps.insert(path.id, node);

                    debug!(
                        "创建类型节点: {} (id: {}, 路径: {})",
                        type_name, path.id.0, path.path
                    );

                    Some(node)
                } else {
                    None
                }
            }

            // Generic: 泛型参数，包括 Self
            Type::Generic(name) => {
                // 特殊处理：Self 指向所属类型/Trait
                if name == "Self" {
                    if let Some(owner_id) = context_owner_id {
                        return self.type_node_maps.get(&owner_id).copied();
                    }
                }

                // 【核心修复】优先使用 TypeCache 查找类型的泛型参数（如 EncoderWriter:E）
                use super::type_cache::{GenericScope as CacheGenericScope, TypeKey};
                use log::debug;

                if let Some(owner_id) = context_owner_id {
                    let type_key = TypeKey::Generic {
                        name: name.clone(),
                        scope: CacheGenericScope::Type(owner_id),
                    };

                    debug!("查找泛型 TypeCache key: {:?}", type_key);

                    if let Some(idx) = self.type_cache.get_node(&type_key) {
                        debug!(
                            "✓ TypeCache找到类型泛型: {} (owner: {}, node: {:?})",
                            name, owner_id.0, idx
                        );
                        return Some(idx);
                    } else {
                        debug!("✗ TypeCache未找到, key: {:?}", type_key);
                    }
                }

                // TypeCache 未找到，尝试查找归一化泛型（Trait:GenericName）
                if let Some(owner_id) = context_owner_id {
                    if let Some(owner_item) = self.parsed.crate_data.index.get(&owner_id) {
                        if let Some(owner_name) = &owner_item.name {
                            let normalized_key = format!("{}:{}", owner_name, name);
                            if let Some(&idx) = self.generic_node_maps.get(&normalized_key) {
                                debug!("✓ 归一化泛型找到: {}", normalized_key);
                                return Some(idx);
                            }
                        }
                    }
                }

                // 尝试方法级泛型（如 decode:T）
                let method_generic_key = format!("{}:{}", context_name, name);
                if let Some(&idx) = self.generic_node_maps.get(&method_generic_key) {
                    debug!("✓ 方法级key找到泛型: {}", method_generic_key);
                    return Some(idx);
                }

                // 最后尝试短名查找（简单泛型，可能被覆盖）
                if let Some(&idx) = self.generic_node_maps.get(name) {
                    debug!("通过短名找到泛型: {}", name);
                    return Some(idx);
                }

                debug!(
                    "未找到泛型: {} (context: {}, owner: {:?})",
                    name, context_name, context_owner_id
                );
                None
            }

            // Primitive: 基本类型
            Type::Primitive(name) => Some(self.get_or_create_primitive_node(name)),

            // Array/Slice: 数组和切片
            Type::Array { type_, .. } | Type::Slice(type_) => {
                // 递归解析内部类型
                self.resolve_type_node_with_owner(type_, context_name, context_owner_id)
            }

            // Tuple: 元组类型
            Type::Tuple(elements) => {
                // 空元组 () 统一处理
                if elements.is_empty() {
                    return Some(self.get_or_create_primitive_node("()"));
                }
                Some(self.create_tuple_node(elements, context_name, context_owner_id))
            }

            // BorrowedRef: 引用类型 &T, &mut T
            Type::BorrowedRef { type_, .. } => {
                // 递归解析内部类型
                self.resolve_type_node_with_owner(type_, context_name, context_owner_id)
            }

            // RawPointer: 裸指针 *const T, *mut T
            Type::RawPointer { type_, .. } => {
                self.resolve_type_node_with_owner(type_, context_name, context_owner_id)
            }

            // FunctionPointer: 函数指针
            Type::FunctionPointer(_) => Some(self.create_function_pointer_node(context_name)),

            // DynTrait: trait object (dyn Trait)
            Type::DynTrait(_) => Some(self.create_dyn_trait_node(context_name)),

            // QualifiedPath: 关联类型 <Type as Trait>::AssocType
            Type::QualifiedPath {
                name,
                self_type,
                trait_,
                ..
            } => {
                use log::debug;

                // 解析 self_type 获取类型 ID（self_type 是 &Box<Type>）
                let type_id = match self_type.as_ref() {
                    Type::ResolvedPath(path) => Some(path.id),
                    Type::Generic(generic_name) if generic_name == "Self" => context_owner_id,
                    _ => None,
                };

                if let Some(trait_path) = trait_ {
                    let trait_id = trait_path.id;
                    // 获取 trait 名称
                    let trait_name = self
                        .parsed
                        .crate_data
                        .index
                        .get(&trait_id)
                        .and_then(|item| item.name.as_deref())
                        .unwrap_or("unknown");

                    // 获取类型名称
                    let type_name = if let Some(tid) = type_id {
                        self.parsed
                            .crate_data
                            .index
                            .get(&tid)
                            .and_then(|item| item.name.as_deref())
                            .unwrap_or("unknown")
                    } else {
                        // 如果无法从 self_type 获取，尝试从 context_owner_id 获取
                        if let Some(owner_id) = context_owner_id {
                            self.parsed
                                .crate_data
                                .index
                                .get(&owner_id)
                                .and_then(|item| item.name.as_deref())
                                .unwrap_or("unknown")
                        } else {
                            "unknown"
                        }
                    };

                    // 查找关联类型节点
                    // 优先查找 Type.AssocType（impl 中重新定义的），如果没有则查找 Trait.AssocType（trait 中定义的默认值）
                    let assoc_type_key = format!("{}.{}", type_name, name);
                    if let Some(&assoc_node) = self.assoc_type_maps.get(&assoc_type_key) {
                        debug!("✓ 找到关联类型: {} (impl 中定义)", assoc_type_key);
                        return Some(assoc_node);
                    }

                    let trait_assoc_key = format!("{}.{}", trait_name, name);
                    if let Some(&assoc_node) = self.assoc_type_maps.get(&trait_assoc_key) {
                        debug!("✓ 找到 Trait 关联类型: {} (trait 中定义)", trait_assoc_key);
                        return Some(assoc_node);
                    }

                    debug!(
                        "✗ 未找到关联类型: {} 或 {}",
                        assoc_type_key, trait_assoc_key
                    );
                }

                None
            }

            _ => None,
        }
    }

    /// 解析类型节点（兼容旧接口）
    fn resolve_type_node(&mut self, ty: &Type, context_name: &str) -> Option<NodeIndex> {
        self.resolve_type_node_with_owner(ty, context_name, None)
    }

    /// 提取 Result<T, E> 的类型
    fn extract_result_types(&self, ty: &Type) -> Option<(Type, Type)> {
        if let Type::ResolvedPath(path) = ty {
            // 通过 path 识别 Result（可能是 std::result::Result, io::Result 等）
            let is_result = path.path.ends_with("Result")
                || path.path == "Result"
                || path.path.contains("::Result");

            if is_result {
                if let Some(args) = &path.args {
                    if let rustdoc_types::GenericArgs::AngleBracketed { args, .. } = &**args {
                        if args.len() >= 2 {
                            // 标准 Result<T, E>
                            if let (
                                rustdoc_types::GenericArg::Type(ok_type),
                                rustdoc_types::GenericArg::Type(err_type),
                            ) = (&args[0], &args[1])
                            {
                                return Some((ok_type.clone(), err_type.clone()));
                            }
                        } else if args.len() == 1 {
                            // TypeAlias 形式的 Result（如 io::Result<T>），第二个参数被固定
                            // 提取 T，并创建一个通用的 Error 类型
                            if let rustdoc_types::GenericArg::Type(ok_type) = &args[0] {
                                // 根据 path 推断 Error 类型
                                let error_type = if path.path.contains("io::") {
                                    // io::Result -> io::Error
                                    Type::ResolvedPath(rustdoc_types::Path {
                                        path: "io::Error".to_string(),
                                        id: path.id, // 使用相同的 id（外部类型）
                                        args: None,
                                    })
                                } else {
                                    // 其他情况，使用通用 Error
                                    Type::Generic("Error".to_string())
                                };
                                return Some((ok_type.clone(), error_type));
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// 提取 Option<T> 的类型
    fn extract_option_type(&self, ty: &Type) -> Option<Type> {
        if let Type::ResolvedPath(path) = ty {
            // 通过 path 识别 Option（可能是 std::option::Option 或 Option）
            let is_option = path.path.ends_with("Option")
                || path.path == "Option"
                || path.path.contains("::Option");

            if is_option {
                if let Some(args) = &path.args {
                    if let rustdoc_types::GenericArgs::AngleBracketed { args, .. } = &**args {
                        if !args.is_empty() {
                            if let rustdoc_types::GenericArg::Type(some_type) = &args[0] {
                                return Some(some_type.clone());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// 创建展开操作节点
    fn create_unwrap_node(&mut self, method_name: &str, op_type: &str) -> NodeIndex {
        let unwrap_name = format!("{}::{}", method_name, op_type);
        let unwrap_node = self.graph.add_type_node(&unwrap_name);
        self.graph
            .node_types
            .insert(unwrap_node, NodeType::UnwrapOp);
        unwrap_node
    }

    /// 获取或创建基本类型节点
    fn get_or_create_primitive_node(&mut self, name: &str) -> NodeIndex {
        let node_id = format!("prim:{}", name);

        if let Some(&idx) = self.other_types.get(&node_id) {
            idx
        } else {
            let idx = self.graph.add_type_node(name);
            self.graph.node_types.insert(idx, NodeType::Primitive);
            self.other_types.insert(node_id, idx); // 存储以保证唯一性
            idx
        }
    }

    /// 创建元组节点
    fn create_tuple_node(
        &mut self,
        elements: &[Type],
        context_name: &str,
        owner_id: Option<Id>,
    ) -> NodeIndex {
        let tuple_name = format!("tuple_{}_elems", elements.len());
        let tuple_node = self.graph.add_type_node(&tuple_name);

        // 为元组的每个元素创建边
        for (idx, elem_type) in elements.iter().enumerate() {
            if let Some(elem_node) =
                self.resolve_type_node_with_owner(elem_type, context_name, owner_id)
            {
                self.graph.type_graph.add_edge(
                    tuple_node,
                    elem_node,
                    TypeRelation {
                        mode: EdgeMode::Move,
                        label: Some(format!("field_{}", idx)),
                    },
                );
            }
        }

        tuple_node
    }

    /// 创建函数指针节点
    fn create_function_pointer_node(&mut self, context_name: &str) -> NodeIndex {
        let fn_ptr_name = format!("{}_fn_ptr", context_name);
        self.graph.add_type_node(&fn_ptr_name)
    }

    /// 创建 dyn trait 节点
    fn create_dyn_trait_node(&mut self, context_name: &str) -> NodeIndex {
        let dyn_name = format!("{}_dyn_trait", context_name);
        self.graph.add_type_node(&dyn_name)
    }
}
