use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use rustdoc_types::{
    Crate, Function, GenericParamDefKind, Id, Impl, Item, ItemEnum, Path as RustdocPath, Type,
    WherePredicate,
};

use super::net::{
    ArcData, ArcKind, FunctionContext, FunctionSummary, ParameterSummary, PetriNet, PlaceId,
};
use super::type_repr::TypeDescriptor;
use super::util::TypeFormatter;

pub struct PetriNetBuilder<'a> {
    crate_: &'a Crate,
    net: PetriNet,
    impl_function_ids: HashSet<Id>,
    // 记录泛型参数及其约束
    generic_parameters: HashMap<String, Vec<Arc<str>>>,
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
            generic_parameters: HashMap::new(),
        }
    }

    pub fn from_crate(crate_: &'a Crate) -> PetriNet {
        let mut builder = Self::new(crate_);
        builder.ingest();
        builder.finish()
    }

    /// 遍历 rustdoc 索引, 将所有函数类条目注册为变迁
    ///
    /// 无约束函数直接记入; impl块中的方法在上下文里携带 Self, 便于后续替换关联类型.
    pub fn ingest(&mut self) {
        // 第一步：创建所有基本类型的库所（使用先验知识预填 trait 实现）
        self.create_primitive_places();

        // 第二步：处理 impl 块，收集方法
        for item in self.crate_.index.values() {
            if let ItemEnum::Impl(impl_block) = &item.inner {
                self.ingest_impl(item, impl_block);
            }
        }

        // 第三步：处理无约束函数，并收集泛型参数
        for item in self.crate_.index.values() {
            if let ItemEnum::Function(func) = &item.inner {
                if self.impl_function_ids.contains(&item.id) {
                    continue;
                }
                // 使用函数名作为上下文，如果没有名称则使用 ID
                let context = if let Some(name) = item.name.as_deref() {
                    name.to_string()
                } else if let Some(path) = self.lookup_qualified_path(item) {
                    path.to_string()
                } else {
                    "<anonymous>".to_string()
                };
                self.collect_generic_parameters(&func.generics, &context);
                if !func.has_body {
                    continue;
                }
                self.ingest_function(item, func, FunctionContext::FreeFunction);
            }
        }

        // 第四步：创建泛型参数库所
        self.create_generic_parameter_places();

        // 第五步：创建从基本类型到符合约束的泛型参数的变迁
        self.create_constraint_transitions();
    }

    pub fn finish(self) -> PetriNet {
        self.net
    }

    /// 处理单个 impl 块, 将其中的方法映射为变迁
    ///
    /// impl 的接收者会记录在上下文, 以便后续把参数/返回中的 Self 替换成实际类型.
    fn ingest_impl(&mut self, item: &Item, impl_block: &Impl) {
        // 获取 impl 目标的类型名称作为上下文
        let impl_context = {
            let receiver_type = TypeFormatter::type_name(&impl_block.for_);
            // 如果有 trait，使用 trait 名称
            if let Some(trait_path) = &impl_block.trait_ {
                format!(
                    "impl<{}> {} for {}",
                    TypeFormatter::format_generic_params(&impl_block.generics.params).join(", "),
                    TypeFormatter::path_to_string(trait_path),
                    receiver_type
                )
            } else {
                format!("impl {}", receiver_type)
            }
        };

        // 收集 impl 块中的泛型参数
        self.collect_generic_parameters(&impl_block.generics, &impl_context);

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
        let impl_where =
            TypeFormatter::format_where_predicates(&impl_block.generics.where_predicates);

        let mut impl_trait_bounds = Vec::new();
        if let Some(trait_path) = &impl_block.trait_ {
            impl_trait_bounds.push(TypeFormatter::path_to_string(trait_path));
        }

        let trait_method_lookup: Option<HashMap<String, Id>> =
            impl_block.trait_.as_ref().and_then(|trait_path| {
                self.crate_
                    .index
                    .get(&trait_path.id)
                    .and_then(|trait_item| {
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
                    // 使用方法名作为上下文，如果没有名称则使用 ID
                    let method_context = if let Some(name) = inner_item.name.as_deref() {
                        name.to_string()
                    } else if let Some(path) = self.lookup_qualified_path(inner_item) {
                        path.to_string()
                    } else {
                        "<anonymous_method>".to_string()
                    };

                    // 收集函数中的泛型参数
                    self.collect_generic_parameters(&func.generics, &method_context);

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
                            // 使用 trait 方法名作为上下文
                            let method_context = if let Some(name) = item.name.as_deref() {
                                name.to_string()
                            } else if let Some(path) = self.lookup_qualified_path(item) {
                                path.to_string()
                            } else {
                                "<anonymous_trait_method>".to_string()
                            };

                            // 收集函数中的泛型参数
                            self.collect_generic_parameters(&func.generics, &method_context);

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

        self.ingest_function_with_context(item, func, context, Vec::new(), Vec::new(), Vec::new());
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
            input_arcs.push((
                place_id,
                ArcData {
                    weight: 1,
                    parameter: Some(parameter.clone()),
                    kind: ArcKind::Normal,
                    descriptor: None,
                    borrow_kind: Some(descriptor.borrow_kind()),
                },
            ));
        }

        let mut output_descriptor = func
            .sig
            .output
            .as_ref()
            .map(|ty| TypeDescriptor::from_type(ty));

        if let (Some(receiver), Some(descriptor)) =
            (receiver_descriptor, output_descriptor.as_mut())
        {
            if let Some(replaced) = descriptor.replace_self(receiver) {
                *descriptor = replaced;
            }
        }

        let mut output_arcs = Vec::new();
        if let Some(descriptor) = output_descriptor.clone() {
            let place_id = self.ensure_place(descriptor.clone());
            output_arcs.push((
                place_id,
                ArcData {
                    weight: 1,
                    parameter: None,
                    kind: ArcKind::Normal,
                    descriptor: Some(descriptor.clone()),
                    borrow_kind: Some(descriptor.borrow_kind()),
                },
            ));
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
            self.net
                .add_input_arc_from_place(place_id, transition_id, arc_data);
        }

        for (place_id, arc_data) in output_arcs {
            self.net
                .add_output_arc_to_place(transition_id, place_id, arc_data);
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
                Some(FunctionContext::InherentMethod {
                    receiver: descriptor,
                })
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

    /// 获取基本类型的预定义 trait 实现
    ///
    /// 返回该基本类型实现的标准 trait 列表
    /// trait 名称使用简单名称（如 "Sized"），与 rustdoc JSON 中的格式保持一致
    fn get_primitive_traits(primitive_name: &str) -> Vec<Arc<str>> {
        match primitive_name {
            // 整数类型（有符号和无符号）
            "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
            | "u128" | "usize" => {
                vec![
                    Arc::<str>::from("Copy"),
                    Arc::<str>::from("Clone"),
                    Arc::<str>::from("Debug"),
                    Arc::<str>::from("PartialEq"),
                    Arc::<str>::from("Eq"),
                    Arc::<str>::from("PartialOrd"),
                    Arc::<str>::from("Ord"),
                    Arc::<str>::from("Hash"),
                    Arc::<str>::from("Sized"),
                    Arc::<str>::from("Send"),
                    Arc::<str>::from("Sync"),
                ]
            }
            // 浮点数类型（不实现 Ord 和 Eq，因为 NaN 问题）
            "f32" | "f64" => {
                vec![
                    Arc::<str>::from("Copy"),
                    Arc::<str>::from("Clone"),
                    Arc::<str>::from("Debug"),
                    Arc::<str>::from("PartialEq"),
                    Arc::<str>::from("PartialOrd"),
                    Arc::<str>::from("Sized"),
                    Arc::<str>::from("Send"),
                    Arc::<str>::from("Sync"),
                ]
            }
            // bool 类型
            "bool" => {
                vec![
                    Arc::<str>::from("Copy"),
                    Arc::<str>::from("Clone"),
                    Arc::<str>::from("Debug"),
                    Arc::<str>::from("PartialEq"),
                    Arc::<str>::from("Eq"),
                    Arc::<str>::from("PartialOrd"),
                    Arc::<str>::from("Ord"),
                    Arc::<str>::from("Hash"),
                    Arc::<str>::from("Sized"),
                    Arc::<str>::from("Send"),
                    Arc::<str>::from("Sync"),
                ]
            }
            // char 类型
            "char" => {
                vec![
                    Arc::<str>::from("Copy"),
                    Arc::<str>::from("Clone"),
                    Arc::<str>::from("Debug"),
                    Arc::<str>::from("PartialEq"),
                    Arc::<str>::from("Eq"),
                    Arc::<str>::from("PartialOrd"),
                    Arc::<str>::from("Ord"),
                    Arc::<str>::from("Hash"),
                    Arc::<str>::from("Sized"),
                    Arc::<str>::from("Send"),
                    Arc::<str>::from("Sync"),
                ]
            }
            // str 类型（切片类型，不是 Sized）
            "str" => {
                vec![
                    Arc::<str>::from("Clone"),
                    Arc::<str>::from("Debug"),
                    Arc::<str>::from("PartialEq"),
                    Arc::<str>::from("Eq"),
                    Arc::<str>::from("PartialOrd"),
                    Arc::<str>::from("Ord"),
                    Arc::<str>::from("Hash"),
                    Arc::<str>::from("Send"),
                    Arc::<str>::from("Sync"),
                ]
            }
            _ => Vec::new(),
        }
    }

    /// 从 rustdoc JSON 中查询基本类型的额外 trait 实现
    ///
    /// 对于关联类型约束（如 T: Iterator<Item = u8>），需要查询实际的 impl 条目
    /// 返回的 trait 名称使用简单名称（如 "Sized"），与 rustdoc JSON 中的格式保持一致
    fn query_primitive_impls_from_rustdoc(&self, primitive_name: &str) -> Vec<Arc<str>> {
        let mut additional_traits = Vec::new();

        for item in self.crate_.index.values() {
            if let ItemEnum::Impl(impl_block) = &item.inner {
                // 检查是否为基本类型的 impl
                if let Type::Primitive(ref name) = impl_block.for_ {
                    if name == primitive_name {
                        // 如果有 trait，记录 trait 名称
                        if let Some(trait_path) = &impl_block.trait_ {
                            let trait_name = TypeFormatter::path_to_string(trait_path);
                            // 提取简单名称（JSON 中通常就是简单名称，但为了保险起见提取最后一部分）
                            let simple_name = trait_name.split("::").last().unwrap_or(&trait_name);
                            additional_traits.push(Arc::<str>::from(simple_name));
                        }
                    }
                }
            }
        }

        additional_traits
    }

    /// 创建所有基本类型的库所
    ///
    /// 使用先验知识预填常见 trait 实现，并查询 rustdoc JSON 获取额外的实现
    fn create_primitive_places(&mut self) {
        let primitives = vec![
            "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
            "f32", "f64", "bool", "char", "str",
        ];

        for primitive_name in primitives {
            let descriptor = TypeDescriptor::from_type(&Type::Primitive(primitive_name.into()));

            // 获取预定义的 trait 实现（先验知识）
            let mut implemented_traits = Self::get_primitive_traits(primitive_name);

            // 从 rustdoc JSON 中查询额外的 trait 实现
            let additional_traits = self.query_primitive_impls_from_rustdoc(primitive_name);
            implemented_traits.extend(additional_traits);

            // 去重并排序
            let unique_traits: std::collections::HashSet<_> =
                implemented_traits.into_iter().collect();
            let mut implemented_traits: Vec<_> = unique_traits.into_iter().collect();
            implemented_traits.sort();

            self.net.add_primitive_place(descriptor, implemented_traits);
        }
    }

    /// 收集泛型参数及其约束
    ///
    /// `context` 参数用于绑定泛型参数的上下文，格式如 "foo::T" 或 "Vec::T"
    /// 这样可以区分不同上下文中的同名泛型参数
    fn collect_generic_parameters(&mut self, generics: &rustdoc_types::Generics, context: &str) {
        for param in &generics.params {
            if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                // 使用带上下文的名称：context::param_name
                let scoped_param_name = format!("{}::{}", context, param.name);
                let trait_bounds: Vec<Arc<str>> = bounds
                    .iter()
                    .map(|bound| Arc::<str>::from(TypeFormatter::format_generic_bound(bound)))
                    .collect();

                // 合并已有的约束
                let existing_bounds = self
                    .generic_parameters
                    .entry(scoped_param_name)
                    .or_insert_with(Vec::new);

                for bound in trait_bounds {
                    if !existing_bounds.iter().any(|b| b.as_ref() == bound.as_ref()) {
                        existing_bounds.push(bound);
                    }
                }
            }
        }

        // 处理 where 子句中的约束
        for predicate in &generics.where_predicates {
            if let WherePredicate::BoundPredicate {
                type_,
                bounds,
                generic_params,
            } = predicate
            {
                if generic_params.is_empty() {
                    if let Type::Generic(param_name) = type_ {
                        // 使用带上下文的名称
                        let scoped_param_name = format!("{}::{}", context, param_name);
                        let existing_bounds = self
                            .generic_parameters
                            .entry(scoped_param_name)
                            .or_insert_with(Vec::new);

                        for bound in bounds {
                            let bound_str =
                                Arc::<str>::from(TypeFormatter::format_generic_bound(bound));
                            if !existing_bounds
                                .iter()
                                .any(|b| b.as_ref() == bound_str.as_ref())
                            {
                                existing_bounds.push(bound_str);
                            }
                        }
                    }
                }
            }
        }
    }

    /// 创建泛型参数库所
    fn create_generic_parameter_places(&mut self) {
        for (param_name, bounds) in &self.generic_parameters {
            let descriptor = TypeDescriptor::from_type(&Type::Generic(param_name.clone()));
            self.net
                .add_generic_parameter_place(descriptor, bounds.clone());
        }
    }

    /// 检查基本类型是否满足给定的 trait bound
    ///
    /// 对于简单的 trait bound（如 Clone），检查基本类型是否实现了该 trait
    /// 对于关联类型约束（如 Iterator<Item = u8>），需要更精确的检查
    fn check_trait_bound_satisfied(
        &self,
        primitive_name: &str,
        primitive_traits: &std::collections::HashSet<&str>,
        bound: &str,
    ) -> bool {
        // 解析 trait bound
        // 可能的格式：
        // - "core::clone::Clone" (简单 trait)
        // - "core::iter::Iterator" (简单 trait，可能有泛型参数)
        // - "core::iter::Iterator<Item = u8>" (有关联类型约束)

        // 提取 trait 路径（去掉可能的关联类型约束）
        let trait_path = if let Some(lt_pos) = bound.find('<') {
            &bound[..lt_pos]
        } else {
            bound
        };

        // 规范化 trait 路径（移除可能的 '?' 修饰符等）
        let trait_path = trait_path.trim_start_matches('?').trim();

        // 检查基本类型是否实现了该 trait
        // 支持多种路径格式：
        // - 完整路径：core::clone::Clone
        // - 短路径：Clone
        let trait_simple_name = trait_path.split("::").last().unwrap_or(trait_path);

        // 检查完整路径或简单名称是否匹配
        let is_implemented = primitive_traits.iter().any(|&t| {
            t == trait_path
                || t.ends_with(&format!("::{}", trait_simple_name))
                || t == trait_simple_name
        });

        if !is_implemented {
            return false;
        }

        // 如果有关联类型约束，需要更精确的检查
        // 例如：Iterator<Item = u8> 需要检查 Item 类型是否匹配
        if bound.contains('<') && bound.contains('=') {
            // 对于关联类型约束，我们需要查询 rustdoc JSON 中的 impl
            // 这里采用保守策略：如果没有找到精确匹配，返回 false
            // TODO: 可以扩展为查询 rustdoc JSON 中的 impl 条目来精确匹配
            return self.check_associated_type_bound(primitive_name, bound);
        }

        true
    }

    /// 检查关联类型约束（如 Iterator<Item = u8>）
    ///
    /// 对于复杂约束，查询 rustdoc JSON 中的 impl 条目
    fn check_associated_type_bound(&self, primitive_name: &str, bound: &str) -> bool {
        // 简化处理：如果 rustdoc JSON 中有对应的 impl，则认为满足
        // 否则保守地返回 false

        // 解析 trait 名称
        let trait_name = if let Some(lt_pos) = bound.find('<') {
            &bound[..lt_pos]
        } else {
            bound
        };
        let trait_name = trait_name.trim_start_matches('?').trim();

        // 查询 rustdoc JSON 中的 impl
        for item in self.crate_.index.values() {
            if let ItemEnum::Impl(impl_block) = &item.inner {
                // 检查是否为该基本类型的 impl
                if let Type::Primitive(ref name) = impl_block.for_ {
                    if name == primitive_name {
                        if let Some(trait_path) = &impl_block.trait_ {
                            let impl_trait_name = TypeFormatter::path_to_string(trait_path);

                            // 检查 trait 名称是否匹配
                            let impl_simple_name = impl_trait_name
                                .split("::")
                                .last()
                                .unwrap_or(&impl_trait_name);
                            let bound_simple_name =
                                trait_name.split("::").last().unwrap_or(trait_name);

                            if impl_trait_name == trait_name
                                || impl_simple_name == bound_simple_name
                            {
                                // 找到了对应的 impl，但还需要检查关联类型是否匹配
                                // 这里简化处理：如果找到了 impl 就认为可能满足
                                // TODO: 更精确地检查关联类型约束
                                return true;
                            }
                        }
                    }
                }
            }
        }

        // 如果没有找到对应的 impl，保守地返回 false
        false
    }

    /// 创建从基本类型到符合约束的泛型参数的变迁
    fn create_constraint_transitions(&mut self) {
        // 遍历所有基本类型
        let primitives = vec![
            "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
            "f32", "f64", "bool", "char", "str",
        ];

        for primitive_name in primitives {
            let primitive_descriptor =
                TypeDescriptor::from_type(&Type::Primitive(primitive_name.into()));
            let primitive_place_id =
                if let Some(place_id) = self.net.place_id(&primitive_descriptor) {
                    place_id
                } else {
                    continue;
                };

            // 先获取基本类型实现的 trait（避免后续借用冲突）
            let primitive_traits: std::collections::HashSet<_> = {
                let primitive_place = self
                    .net
                    .place(primitive_place_id)
                    .expect("primitive place should exist");
                primitive_place
                    .implemented_traits
                    .iter()
                    .map(|t| t.as_ref())
                    .collect()
            };

            // 收集所有需要创建的变迁（避免在循环中借用冲突）
            let mut transitions_to_create = Vec::new();

            // 遍历所有泛型参数
            for (generic_name, required_bounds) in &self.generic_parameters {
                let generic_descriptor =
                    TypeDescriptor::from_type(&Type::Generic(generic_name.clone()));
                let generic_place_id =
                    if let Some(place_id) = self.net.place_id(&generic_descriptor) {
                        place_id
                    } else {
                        continue;
                    };

                // 检查基本类型是否满足泛型参数的所有约束
                let mut satisfied_constraints = Vec::new();
                let mut all_satisfied = true;

                for bound in required_bounds {
                    if self.check_trait_bound_satisfied(
                        primitive_name,
                        &primitive_traits,
                        bound.as_ref(),
                    ) {
                        satisfied_constraints.push(bound.clone());
                    } else {
                        all_satisfied = false;
                        break; // 如果有一个约束不满足，就不需要检查其他的了
                    }
                }

                // 记录需要创建的变迁
                if all_satisfied && !required_bounds.is_empty() {
                    transitions_to_create.push((
                        primitive_place_id,
                        generic_place_id,
                        satisfied_constraints,
                    ));
                } else if required_bounds.is_empty() {
                    // 如果泛型参数没有约束，所有基本类型都可以连接
                    transitions_to_create.push((primitive_place_id, generic_place_id, Vec::new()));
                }
            }

            // 现在创建所有变迁（此时已经没有不可变借用了）
            for (from_place, to_place, constraints) in transitions_to_create {
                self.net
                    .add_constraint_transition(from_place, to_place, constraints);
            }
        }
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
        if let GenericParamDefKind::Type {
            bounds: param_bounds,
            ..
        } = &param.kind
        {
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
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {path}: {err}"));
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
