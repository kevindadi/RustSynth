use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use log::{debug, info};
use rustdoc_types::{
    Crate, Function, GenericParamDefKind, Id, Impl, Item, ItemEnum, Path as RustdocPath, Type,
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
    // 记录类型 Item 的 id 到 PlaceId 的映射
    type_place_map: HashMap<Id, PlaceId>,
    // StructFiled Type
    struct_field_type_map: HashMap<Id, (Id, TypeDescriptor)>,
}

impl<'a> PetriNetBuilder<'a> {
    pub fn new(crate_: &'a Crate) -> Self {
        Self {
            crate_,
            net: PetriNet::new(),
            impl_function_ids: HashSet::new(),
            type_place_map: HashMap::new(),
            struct_field_type_map: HashMap::new(),
        }
    }

    pub fn from_crate(crate_: &'a Crate) -> PetriNet {
        let mut builder = Self::new(crate_);
        builder.ingest();
        builder.finish()
    }

    /// 遍历 rustdoc 索引, 将所有ItemEnum::Function | ItemEnum::Impl 注册为变迁
    /// ItemEnum::Trait 注册为 Guard
    ///
    /// 1. 首先对 Item 中的 Struct、Enum、Union、Variant 等类型进行建模，创建 Place
    /// 2. 根据已创建的类型 Place 的 id，从 index 中查找对应的 impl 块，为方法创建变迁
    /// 3. 泛型约束作为变迁的 guard，不需要为泛型参数创建 Place
    /// 4. 输入边带上 borrow_kind 约束，输出边带上返回类型的 borrow_kind
    pub fn ingest(&mut self) {
        info!("🔨 开始构建 Petri Net...");

        // Step 1: 创建所有基本类型的库所
        info!("📦 步骤 1/4: 创建基本类型的 Place");
        self.create_primitive_places();

        // Step 2: 遍历所有 Struct、Enum、Union 等类型定义，为它们创建 Place
        info!("📦 步骤 2/4: 创建类型定义的 Place (Struct/Enum/Union)");
        let mut type_count = 0;
        for item in self.crate_.index.values() {
            match &item.inner {
                ItemEnum::Struct(_)
                | ItemEnum::StructField(_)
                | ItemEnum::Enum(_)
                | ItemEnum::Union(_)
                | ItemEnum::Variant(_) => {
                    self.create_type_place(item);
                    type_count += 1;
                }
                _ => {}
            }
        }
        debug!("   创建了 {} 个类型定义的 Place", type_count);

        // Step 3: 根据已创建的类型 Place 的 id，查找对应的 impl 块，为方法创建变迁
        info!("⚙️  步骤 3/4: 处理 impl 块，为方法创建 Transition");
        let mut impl_items_to_process: Vec<(Id, &Impl)> = Vec::new();
        for (type_id, _place_id) in &self.type_place_map {
            if let Some(type_item) = self.crate_.index.get(type_id) {
                match &type_item.inner {
                    ItemEnum::Struct(struct_def) => {
                        for impl_id in &struct_def.impls {
                            if let Some(impl_item) = self.crate_.index.get(impl_id) {
                                if let ItemEnum::Impl(impl_block) = &impl_item.inner {
                                    impl_items_to_process.push((impl_item.id, impl_block));
                                }
                            }
                        }
                    }
                    ItemEnum::Enum(enum_def) => {
                        for impl_id in &enum_def.impls {
                            if let Some(impl_item) = self.crate_.index.get(impl_id) {
                                if let ItemEnum::Impl(impl_block) = &impl_item.inner {
                                    impl_items_to_process.push((impl_item.id, impl_block));
                                }
                            }
                        }
                    }
                    ItemEnum::Union(union_def) => {
                        for impl_id in &union_def.impls {
                            if let Some(impl_item) = self.crate_.index.get(impl_id) {
                                if let ItemEnum::Impl(impl_block) = &impl_item.inner {
                                    impl_items_to_process.push((impl_item.id, impl_block));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let impl_count = impl_items_to_process.len();
        debug!("   找到 {} 个 impl 块需要处理", impl_count);
        for (impl_id, impl_block) in impl_items_to_process {
            if let Some(impl_item) = self.crate_.index.get(&impl_id) {
                self.ingest_impl(impl_item, impl_block);
            }
        }

        // Step 4: 处理无约束函数
        info!("⚙️  步骤 4/4: 处理无约束函数");
        let mut free_func_count = 0;
        for item in self.crate_.index.values() {
            if let ItemEnum::Function(func) = &item.inner {
                if self.impl_function_ids.contains(&item.id) {
                    continue;
                }
                if !func.has_body {
                    continue;
                }
                self.ingest_function(item, func, FunctionContext::FreeFunction);
                free_func_count += 1;
            }
        }
        debug!("   处理了 {} 个无约束函数", free_func_count);
    }

    pub fn finish(self) -> PetriNet {
        info!("📊 Petri Net 构建完成");
        info!("   ✅ 总共创建了 {} 个 Place", self.net.place_count());
        info!(
            "   ✅ 总共创建了 {} 个 Transition",
            self.net.transition_count()
        );
        self.net
    }

    /// 将 impl 块中的方法注册为变迁
    /// 接收者会记录在上下文, 以便后续把参数/返回中的 Self 替换成实际类型.
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
                    // 泛型约束会记录在 FunctionSummary 的 trait_bounds 中
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
            // 先检查原始类型是否为泛型
            let is_generic = matches!(ty, Type::Generic(_));
            let mut descriptor = TypeDescriptor::from_type(ty);
            if let Some(receiver) = receiver_descriptor {
                if let Some(replaced) = descriptor.replace_self(receiver) {
                    descriptor = replaced;
                }
            }
            // 如果是泛型类型，不创建 Place，但保留参数信息（用于 guard 检查）
            if !is_generic {
                if let Some(place_id) = self.ensure_place(descriptor.clone()) {
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
            } else {
                // 泛型参数，只记录在 summary 中，不创建边（guard 会在合成阶段检查）
                let parameter = ParameterSummary {
                    name: (!name.is_empty()).then(|| Arc::<str>::from(name.as_str())),
                    descriptor: descriptor.clone(),
                };
                summary_inputs.push(parameter);
            }
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
            // 检查返回值是否为泛型类型
            let is_generic = func
                .sig
                .output
                .as_ref()
                .map(|ty| matches!(ty, Type::Generic(_)))
                .unwrap_or(false);
            // 如果是泛型类型，不创建 Place
            if !is_generic {
                if let Some(place_id) = self.ensure_place(descriptor.clone()) {
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
            }
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

        let transition_id = self.net.add_transition(function_summary.clone());

        let context_str = match &function_summary.context {
            FunctionContext::FreeFunction => "FreeFunction".to_string(),
            FunctionContext::InherentMethod { receiver } => {
                format!("InherentMethod({})", receiver.display())
            }
            FunctionContext::TraitImplementation {
                receiver,
                trait_path,
            } => {
                format!(
                    "TraitImplementation({}, {})",
                    receiver.display(),
                    trait_path
                )
            }
        };
        info!(
            "   🔄 [Transition] {} (Item ID: {}, Transition ID: {})",
            function_summary.signature,
            function_summary.item_id.0,
            transition_id.0.index()
        );
        debug!("       Context: {}", context_str);
        if !function_summary.generics.is_empty() {
            debug!("       Generics: {}", function_summary.generics.join(", "));
        }
        if !function_summary.trait_bounds.is_empty() {
            debug!(
                "       Trait Bounds: {}",
                function_summary.trait_bounds.join(", ")
            );
        }

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
            ItemEnum::Struct(_) | ItemEnum::Enum(_) | ItemEnum::Union(_) | ItemEnum::Variant(_) => {
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

    /// 确保 Place 存在，但如果类型是泛型则返回 None（泛型不创建 Place）
    fn ensure_place(&mut self, descriptor: TypeDescriptor) -> Option<PlaceId> {
        // 检查是否为泛型类型
        let normalized = descriptor.normalized();
        let canonical = normalized.canonical();

        // 如果类型名是单个标识符且不是基本类型，可能是泛型参数
        // 更精确的检查：通过原始 Type 来判断
        // 但这里我们只能根据字符串来判断，保守处理
        // 如果类型名包含 "::" 或者是已知的类型，则不是泛型参数

        // 简单检查：如果规范化后的类型名不包含路径分隔符且不在基本类型列表中
        // 且不是 Self，可能是泛型参数
        if !canonical.contains("::")
            && !matches!(
                canonical.as_ref(),
                "i8" | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "isize"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "usize"
                    | "f32"
                    | "f64"
                    | "bool"
                    | "char"
                    | "str"
                    | "String"
                    | "Self"
            )
            && canonical.chars().all(|c| c.is_alphanumeric() || c == '_')
            && !canonical
                .chars()
                .next()
                .map(|c| c.is_uppercase())
                .unwrap_or(false)
        {
            // 可能是泛型参数（如 T, U），不创建 Place
            return None;
        }

        // 检查 Place 是否已存在（通过规范化后的描述符）
        let normalized_desc = descriptor.normalized();
        let existing_id = self.net.place_id(&normalized_desc);

        if let Some(place_id) = existing_id {
            // Place 已存在，返回已有 ID
            return Some(place_id);
        }

        // Place 不存在，创建新的并记录日志
        let place_id = self.net.add_place(descriptor.clone());
        debug!(
            "   📍 [Place] {} (Place ID: {}) [通过 ensure_place 创建]",
            descriptor.display(),
            place_id.0.index()
        );
        Some(place_id)
    }

    /// 为类型定义（Struct、Enum、Union）创建 Place
    /// 记录类型 Item 的 id 到 PlaceId 的映射
    fn create_type_place(&mut self, item: &Item) {
        // 构建类型的 Type 表示
        let type_path = if let Some(path_summary) = self.crate_.paths.get(&item.id) {
            let path_str = path_summary.path.join("::");
            rustdoc_types::Path {
                path: path_str.clone(),
                id: item.id,
                args: None, // 泛型参数会在后续处理
            }
        } else {
            // 如果没有路径信息，使用名称或 ID
            let name = item
                .name
                .as_deref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("Item{}", item.id.0));
            rustdoc_types::Path {
                path: name,
                id: item.id,
                args: None,
            }
        };

        let type_ty = Type::ResolvedPath(type_path);
        let descriptor = TypeDescriptor::from_type(&type_ty);

        // 创建类型定义的 Place（使用 Owned 作为默认借用类型）
        let place_id = self.net.add_place(descriptor.clone());

        let type_kind = match &item.inner {
            ItemEnum::Struct(_) => "Struct",
            ItemEnum::Enum(_) => "Enum",
            ItemEnum::Union(_) => "Union",
            ItemEnum::Variant(_) => "Variant",
            ItemEnum::StructField(_) => "StructField",
            _ => "Unknown",
        };
        info!(
            "   🎯 [Place] {} {} (Item ID: {}, Place ID: {})",
            type_kind,
            descriptor.display(),
            item.id.0,
            place_id.0.index()
        );

        // 记录类型 Item 的 id 到 PlaceId 的映射
        self.type_place_map.insert(item.id, place_id);
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

            let place_id = self
                .net
                .add_primitive_place(descriptor.clone(), implemented_traits.clone());
            debug!(
                "   📍 [Place] {} (Place ID: {}, Traits: [{}])",
                descriptor.display(),
                place_id.0.index(),
                implemented_traits.join(", ")
            );
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
