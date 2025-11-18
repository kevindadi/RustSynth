use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use log::{debug, info};
use rustdoc_types::{
    Crate, Function, GenericParamDefKind, Id, Impl, Item, ItemEnum, Path as RustdocPath, Type,
};

use super::net::{FunctionContext, PetriNet, PlaceId};
use super::type_repr::TypeDescriptor;
use super::util::TypeFormatter;

pub struct PetriNetBuilder<'a> {
    crate_: &'a Crate,
    net: PetriNet,
    impl_function_ids: HashSet<Id>,
    // 记录类型 Item 的 id 到 PlaceId 的映射
    type_place_map: HashMap<Id, PlaceId>,
    // 记录每个 Option<T> 实例的完整类型描述符到内部类型的映射
    option_wrappers: HashMap<Arc<str>, String>,
    // 记录每个 Result<T, E> 实例的完整类型描述符到 (Ok类型, Err类型) 的映射
    result_wrappers: HashMap<Arc<str>, (String, String)>,
    // 记录结构泛型参数占位库所: (结构PlaceId, 泛型参数名) -> 占位库所PlaceId
    generic_placeholder_places: HashMap<(PlaceId, Arc<str>), PlaceId>,
    synthetic_id_counter: u32,
}

impl<'a> PetriNetBuilder<'a> {
    pub fn new(crate_: &'a Crate) -> Self {
        Self {
            crate_,
            net: PetriNet::new(),
            impl_function_ids: HashSet::new(),
            type_place_map: HashMap::new(),
            option_wrappers: HashMap::new(),
            result_wrappers: HashMap::new(),
            generic_placeholder_places: HashMap::new(),
            synthetic_id_counter: 0,
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
    /// 1. 首先对 Item 中的 Struct、Enum(Variant)、Union 等类型进行建模,创建 Place
    /// 2. 根据已创建的类型 Place 的 id,从 index 中查找对应的 impl 块,为方法创建变迁
    /// 3. 泛型约束作为变迁的 guard,不需要为泛型参数创建 Place
    pub fn ingest(&mut self) {
        info!("🔨 开始构建 Petri Net...");

        // Step 1: 遍历所有 Struct、Enum、Union 等类型定义,为它们创建 Place
        info!("📦 步骤 1/3: 创建类型定义的 Place (Struct/Enum/Union)");
        let mut type_count = 0;
        for item in self.crate_.index.values() {
            match &item.inner {
                ItemEnum::Struct(_) | ItemEnum::Enum(_) | ItemEnum::Union(_) => {
                    self.create_type_place(item);
                    type_count += 1;
                }
                _ => {}
            }
        }
        debug!("   创建了 {} 个类型定义的 Place", type_count);

        // Step 2: 根据已创建的类型 Place 的 id,查找对应的 impl 块,为方法创建变迁
        info!("⚙️  步骤 2/3: 处理 impl 块,为方法创建 Transition");
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
        // 不再创建 wrapper transitions，因为不再需要基本类型
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
        // 获取 Self 类型的 Item ID
        let receiver_id = if let Type::ResolvedPath(path) = &impl_block.for_ {
            path.id
        } else {
            // 如果不是 ResolvedPath，尝试从 type_place_map 查找
            // 对于基本类型，可能不在 type_place_map 中
            // 这里暂时跳过，后续可以改进
            return;
        };

        let context = if let Some(trait_path) = &impl_block.trait_ {
            let trait_path_str = Arc::<str>::from(TypeFormatter::path_to_string(trait_path));
            FunctionContext::TraitImplementation {
                receiver_id,
                trait_path: trait_path_str,
            }
        } else {
            FunctionContext::InherentMethod { receiver_id }
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

    /// 从 Type 中提取 Item ID（如果是 ResolvedPath）
    fn extract_item_id_from_type(&self, ty: &Type, receiver_id: Option<Id>) -> Option<Id> {
        match ty {
            Type::ResolvedPath(path) => {
                // 如果是 Self，使用 receiver_id
                if let Some(recv_id) = receiver_id {
                    // 检查是否是 Self 类型
                    if let Some(item) = self.crate_.index.get(&path.id) {
                        if item.name.as_deref() == Some("Self") {
                            return Some(recv_id);
                        }
                    }
                }
                Some(path.id)
            }
            Type::Generic(name) if name == "Self" => receiver_id.map(|id| id),
            _ => None,
        }
    }

    /// 参数 -> 输入 place, 返回值 -> 输出 place
    fn ingest_function_with_context(
        &mut self,
        item: &Item,
        func: &Function,
        context: FunctionContext,
        _impl_generics: Vec<String>,
        _impl_where: Vec<String>,
        _impl_trait_bounds: Vec<String>,
    ) {
        let receiver_id = context_receiver_id(&context);

        let mut input_types = Vec::new();
        let mut input_arcs = Vec::new();
        for (name, ty) in &func.sig.inputs {
            // 尝试从类型中提取 Item ID
            if let Some(type_id) = self.extract_item_id_from_type(ty, receiver_id) {
                // 查找对应的 Place
                if let Some(place_id) = self.net.place_id(type_id) {
                    input_types.push(type_id);
                    let param_name = (!name.is_empty()).then(|| Arc::<str>::from(name.as_str()));
                    input_arcs.push((place_id, param_name, type_id));
                }
            }
        }

        // 处理返回值
        let output_type = func
            .sig
            .output
            .as_ref()
            .and_then(|ty| self.extract_item_id_from_type(ty, receiver_id));

        let mut output_arcs = Vec::new();
        if let Some(output_id) = output_type {
            if let Some(place_id) = self.net.place_id(output_id) {
                output_arcs.push((place_id, output_id));
            }
        }

        let function_name = Arc::<str>::from(item.name.as_deref().unwrap_or("<anonymous>"));

        let transition_id = self.net.add_transition(
            item.id,
            function_name.clone(),
            context.clone(),
            Option::Some(input_types.clone()),
            output_type,
        );

        let context_str = match &context {
            FunctionContext::FreeFunction => "FreeFunction".to_string(),
            FunctionContext::InherentMethod { receiver_id } => {
                format!("InherentMethod(Item ID: {})", receiver_id.0)
            }
            FunctionContext::TraitImplementation {
                receiver_id,
                trait_path,
            } => {
                format!(
                    "TraitImplementation(Item ID: {}, {})",
                    receiver_id.0, trait_path
                )
            }
        };
        info!(
            "   🔄 [Transition] {} (Item ID: {}, Transition ID: {})",
            function_name,
            item.id.0,
            transition_id.0.index()
        );
        debug!("       Context: {}", context_str);

        let is_isolated = input_arcs.is_empty() && output_arcs.is_empty();

        for (place_id, param_name, type_id) in input_arcs {
            self.net
                .add_input_arc_from_place(place_id, transition_id, param_name, type_id);
        }

        for (place_id, type_id) in output_arcs {
            self.net
                .add_output_arc_to_place(transition_id, place_id, type_id);
        }

        if is_isolated {
            debug!(
                "       ⚠️  警告: 变迁 {} 是孤立节点(所有参数和返回值都是泛型参数)",
                function_name
            );
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

        match &owner_item.inner {
            ItemEnum::Struct(_) | ItemEnum::Enum(_) | ItemEnum::Union(_) | ItemEnum::Variant(_) => {
                Some(FunctionContext::InherentMethod {
                    receiver_id: owner_item.id,
                })
            }
            ItemEnum::Trait(_) => Some(FunctionContext::TraitImplementation {
                receiver_id: owner_item.id,
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

    /// 确保 Place 存在,通过 Item ID 查找或创建
    /// 如果类型是泛型则返回 None(泛型不创建 Place)
    fn ensure_place(&mut self, item_id: Id) -> Option<PlaceId> {
        // 先检查是否已存在
        if let Some(place_id) = self.net.place_id(item_id) {
            return Some(place_id);
        }

        // 如果不存在，尝试从 crate 中获取 Item 并创建 Place
        if let Some(item) = self.crate_.index.get(&item_id) {
            match &item.inner {
                ItemEnum::Struct(_) | ItemEnum::Enum(_) | ItemEnum::Union(_) => {
                    self.create_type_place(item);
                    self.net.place_id(item_id)
                }
                _ => None,
            }
        } else {
            None
        }
    }

    #[allow(dead_code)]
    fn ensure_place_old(&mut self, _descriptor: TypeDescriptor) -> Option<PlaceId> {
        // 不再需要这个方法，因为不再创建基本类型的 Place
        None
    }

    /// 为类型定义(Struct、Enum、Union、Variant)创建 Place
    /// 记录类型 Item 的 id 到 PlaceId 的映射
    /// 库所名称使用 item.name(类型名称),path 路径仅作为补充信息
    fn create_type_place(&mut self, item: &Item) {
        // 使用 item.name 作为库所的 key,path 仅作为补充信息
        let type_name = item
            .name
            .as_deref()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("Item{}", item.id.0));

        // 确定类型种类并创建对应的 Place
        let place_id = match &item.inner {
            ItemEnum::Struct(_) => {
                let variants = Vec::new();
                self.net
                    .add_composite_place(item.id, item.inner.clone(), variants)
            }
            ItemEnum::Enum(enum_def) => {
                let mut variants = Vec::new();
                for variant_id in &enum_def.variants {
                    if let Some(variant_item) = self.crate_.index.get(variant_id) {
                        if let ItemEnum::Variant(variant) = &variant_item.inner {
                            variants.push(variant.clone());
                        }
                    }
                }
                self.net
                    .add_composite_place(item.id, item.inner.clone(), variants)
            }
            ItemEnum::Union(_) => {
                let variants = Vec::new();
                self.net
                    .add_composite_place(item.id, item.inner.clone(), variants)
            }
            _ => {
                // 其他类型不应该到达这里
                return;
            }
        };

        let type_kind = match &item.inner {
            ItemEnum::Struct(_) => "Struct",
            ItemEnum::Enum(_) => "Enum",
            ItemEnum::Union(_) => "Union",
            ItemEnum::Variant(_) => "Variant",
            _ => "Unknown",
        };

        // 获取完整路径作为补充信息
        let full_path = self
            .crate_
            .paths
            .get(&item.id)
            .map(|summary| summary.path.join("::"))
            .unwrap_or_else(|| type_name.clone());

        info!(
            "   🎯 [Place] {} {} (Item ID: {}, Place ID: {}) [路径: {}]",
            type_kind,
            type_name,
            item.id.0,
            place_id.0.index(),
            full_path
        );

        // 记录类型 Item 的 id 到 PlaceId 的映射
        self.type_place_map.insert(item.id, place_id);
    }

    /// 将 Variant 的字段映射到 Enum 的 PlaceId
    /// Variant 的所有字段都映射到同一个 Enum Place
    /// 注意：现在不再需要别名映射，因为直接使用 Item ID
    fn register_variant_field_aliases(
        &mut self,
        _variant_item: &Item,
        _variant: &rustdoc_types::Variant,
        _enum_place_id: PlaceId,
    ) {
        // 不再需要别名映射，因为直接使用 Item ID
    }

    #[allow(dead_code)]
    fn create_option_wrapper_transitions(&mut self) {
        // 不再需要 wrapper transitions
    }

    #[allow(dead_code)]
    fn add_option_some_transition(
        &mut self,
        _option_place: PlaceId,
        _option_type_key: &str,
        _inner_type: &str,
    ) {
        // 不再需要 wrapper transitions
    }

    #[allow(dead_code)]
    fn add_option_none_transition(&mut self, _option_place: PlaceId, _option_type_key: &str) {
        // 不再需要 wrapper transitions
    }

    #[allow(dead_code)]
    fn create_result_wrapper_transitions(&mut self) {
        // 不再需要 wrapper transitions
    }

    #[allow(dead_code)]
    fn add_result_unwrap_transition(
        &mut self,
        _result_place: PlaceId,
        _result_type_key: &str,
        _ok_type: &str,
        _err_type: &str,
    ) {
        // 不再需要 wrapper transitions
    }

    #[allow(dead_code)]
    fn add_result_unwrap_single_transition(
        &mut self,
        _result_place: PlaceId,
        _result_type_key: &str,
        _ok_type: &str,
    ) {
        // 不再需要 wrapper transitions
    }

    fn next_synthetic_id(&mut self) -> Id {
        let id = Id(u32::MAX.saturating_sub(self.synthetic_id_counter));
        self.synthetic_id_counter = self.synthetic_id_counter.wrapping_add(1);
        id
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
    /// trait 名称使用简单名称(如 "Sized"),与 rustdoc JSON 中的格式保持一致
    fn get_primitive_traits(primitive_name: &str) -> Vec<Arc<str>> {
        match primitive_name {
            // 整数类型(有符号和无符号)
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
            // 浮点数类型(不实现 Ord 和 Eq,因为 NaN 问题)
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
            // str 类型(切片类型,不是 Sized)
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
    /// 对于关联类型约束(如 T: Iterator<Item = u8>),需要查询实际的 impl 条目
    /// 返回的 trait 名称使用简单名称(如 "Sized"),与 rustdoc JSON 中的格式保持一致
    fn query_primitive_impls_from_rustdoc(&self, primitive_name: &str) -> Vec<Arc<str>> {
        let mut additional_traits = Vec::new();

        for item in self.crate_.index.values() {
            if let ItemEnum::Impl(impl_block) = &item.inner {
                // 检查是否为基本类型的 impl
                if let Type::Primitive(ref name) = impl_block.for_ {
                    if name == primitive_name {
                        // 如果有 trait,记录 trait 名称
                        if let Some(trait_path) = &impl_block.trait_ {
                            let trait_name = TypeFormatter::path_to_string(trait_path);
                            // 提取简单名称(JSON 中通常就是简单名称,但为了保险起见提取最后一部分)
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
    /// 使用先验知识预填常见 trait 实现,并查询 rustdoc JSON 获取额外的实现
    /// 注意：现在不再需要基本类型，因为只创建 Struct、Enum、Union
    #[allow(dead_code)]
    fn create_primitive_places(&mut self) {
        let primitives = vec![
            "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
            "f32", "f64", "bool", "char", "str",
        ];

        for primitive_name in primitives {
            let _descriptor = TypeDescriptor::from_type(&Type::Primitive(primitive_name.into()));
            let mut implemented_traits = Self::get_primitive_traits(primitive_name);

            let additional_traits = self.query_primitive_impls_from_rustdoc(primitive_name);
            implemented_traits.extend(additional_traits);

            let unique_traits: std::collections::HashSet<_> =
                implemented_traits.into_iter().collect();
            let mut implemented_traits: Vec<_> = unique_traits.into_iter().collect();
            implemented_traits.sort();

            // 不再创建基本类型的 Place
            // let place_id = self
            //     .net
            //     .add_primitive_place(descriptor.clone(), implemented_traits.clone());
        }
    }
}

fn context_receiver_id(context: &FunctionContext) -> Option<Id> {
    match context {
        FunctionContext::InherentMethod { receiver_id } => Some(*receiver_id),
        FunctionContext::TraitImplementation { receiver_id, .. } => Some(*receiver_id),
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

        // 检查 Place 和 Transition 是否创建成功
        assert!(net.place_count() > 0, "应该创建了至少一个 Place");
        assert!(net.transition_count() > 0, "应该创建了至少一个 Transition");
    }
}
