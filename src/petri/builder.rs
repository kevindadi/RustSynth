use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use log::{debug, info, warn};
use rustdoc_types::{
    Crate, Function, Id, Impl, Item, ItemEnum, Path as RustdocPath, StructKind, Type, VariantKind,
};

use super::net::{PetriNet, PlaceId};
use super::structure::{BorrowKind, Flow, Place, PlaceKind, Transition, TransitionKind};

pub struct PetriNetBuilder<'a> {
    crate_: &'a Crate,
    net: PetriNet,
    /// 映射 Item ID 到 PlaceId，用于跟踪已创建的类型 Place
    type_place_map: HashMap<Id, PlaceId>,
    /// 用于跟踪哪些函数是 impl 块中的方法，避免重复处理
    impl_function_ids: HashSet<Id>,
    /// 用于跟踪已创建的 Type Place，避免重复创建
    /// 键是类型的字符串表示
    type_cache: HashMap<String, PlaceId>,
    /// 用于生成临时 ID 的计数器
    next_temp_id: u32,
}

impl<'a> PetriNetBuilder<'a> {
    pub fn new(crate_: &'a Crate) -> Self {
        Self {
            crate_,
            net: PetriNet::new(),
            type_place_map: HashMap::new(),
            impl_function_ids: HashSet::new(),
            type_cache: HashMap::new(),
            next_temp_id: u32::MAX - 1_000_000, // 从一个大数字开始，避免与真实 ID 冲突
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

        // Step 1: 遍历所有 Struct、Enum、Union,Variant 等类型定义,为它们创建 Place
        info!("📦 步骤 1/4: 创建类型定义的 Place (Struct/Enum/Union/Variant)");
        let mut type_count = 0;
        for item in self.crate_.index.values() {
            match &item.inner {
                ItemEnum::Struct(_) | ItemEnum::Union(_) | ItemEnum::Variant(_) => {
                    self.create_type_place(item);
                    type_count += 1;
                }
                _ => {}
            }
        }
        debug!("   创建了 {} 个类型定义的 Place", type_count);

        // Step 2: 构建类型和其成员之间的 holds 关系
        info!("🧱 步骤 2/4: 连接类型成员 holds 关系");
        self.build_type_relationships();

        // Step 3: 根据已创建的类型 Place 的 id,查找对应的 impl 块,为方法创建变迁
        info!("⚙️  步骤 3/4: 处理 impl 块,为方法创建 Transition");
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
            let trait_path_str = Arc::<str>::from(Self::format_path(trait_path));
            FunctionContext::TraitImplementation {
                receiver_id,
                trait_path: trait_path_str,
            }
        } else {
            FunctionContext::InherentMethod { receiver_id }
        };

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
                    self.ingest_function_with_context(inner_item, func, context.clone());
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
                            self.ingest_function_with_context(item, func, context.clone());
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

        self.ingest_function_with_context(item, func, context);
    }

    /// 为 Struct/Union/Variant 类型创建 Place
    fn create_type_place(&mut self, item: &Item) {
        let place_kind = match &item.inner {
            ItemEnum::Struct(s) => PlaceKind::Struct(s.clone()),
            ItemEnum::Union(u) => PlaceKind::Union(u.clone()),
            ItemEnum::Variant(v) => PlaceKind::Variant(v.clone()),
            _ => return,
        };

        let name = item
            .name
            .clone()
            .unwrap_or_else(|| format!("anonymous_{:?}", item.id));
        let path = item.name.clone().unwrap_or_default();

        let place = Place::new(item.id, name, path, place_kind);
        let place_id = self.net.add_place_and_get_id(place);

        self.type_place_map.insert(item.id, place_id);
    }

    /// 构建类型成员之间的 holds 关系
    /// 1. 处理 Struct 的字段
    /// 2. 处理 Enum 的变体
    /// 3. 处理 Union 的字段
    /// 4. 处理 Variant 的字段（Tuple 和 Struct）
    fn build_type_relationships(&mut self) {
        // 收集需要处理的类型关系
        let mut relationships = Vec::new();

        for (type_id, _place_id) in &self.type_place_map.clone() {
            if let Some(item) = self.crate_.index.get(type_id) {
                match &item.inner {
                    ItemEnum::Struct(struct_def) => {
                        match &struct_def.kind {
                            StructKind::Plain { fields, .. } => {
                                for field_id in fields {
                                    if let Some(field_item) = self.crate_.index.get(field_id) {
                                        if let ItemEnum::StructField(field_type) = &field_item.inner
                                        {
                                            relationships.push((
                                                *type_id,
                                                *field_id,
                                                field_type.clone(),
                                            ));
                                        }
                                    }
                                }
                            }
                            StructKind::Tuple(field_ids) => {
                                for field_id in field_ids.iter().flatten() {
                                    if let Some(field_item) = self.crate_.index.get(field_id) {
                                        if let ItemEnum::StructField(field_type) = &field_item.inner
                                        {
                                            relationships.push((
                                                *type_id,
                                                *field_id,
                                                field_type.clone(),
                                            ));
                                        }
                                    }
                                }
                            }
                            StructKind::Unit => {
                                // Unit struct 没有字段
                            }
                        }
                    }
                    ItemEnum::Enum(enum_def) => {
                        for variant_id in &enum_def.variants {
                            relationships.push((*type_id, *variant_id, Type::Infer));
                        }
                    }
                    ItemEnum::Union(union_def) => {
                        for field_id in &union_def.fields {
                            if let Some(field_item) = self.crate_.index.get(field_id) {
                                if let ItemEnum::StructField(field_type) = &field_item.inner {
                                    relationships.push((*type_id, *field_id, field_type.clone()));
                                }
                            }
                        }
                    }
                    ItemEnum::Variant(variant) => {
                        match &variant.kind {
                            VariantKind::Plain => {
                                // Plain variant 不需要特殊处理
                            }
                            VariantKind::Tuple(field_ids) => {
                                for field_id in field_ids.iter().flatten() {
                                    if let Some(field_item) = self.crate_.index.get(field_id) {
                                        if let ItemEnum::StructField(field_type) = &field_item.inner
                                        {
                                            relationships.push((
                                                *type_id,
                                                *field_id,
                                                field_type.clone(),
                                            ));
                                        }
                                    }
                                }
                            }
                            VariantKind::Struct { fields, .. } => {
                                for field_id in fields {
                                    if let Some(field_item) = self.crate_.index.get(field_id) {
                                        if let ItemEnum::StructField(field_type) = &field_item.inner
                                        {
                                            relationships.push((
                                                *type_id,
                                                *field_id,
                                                field_type.clone(),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // 处理所有关系
        for (owner_id, member_id, field_type) in relationships {
            // 为字段类型创建 Place（如果需要）
            let member_place_id = self.create_or_get_type_place(&field_type, &member_id);

            // 创建 holds transition 连接 owner 和 member
            if let (Some(owner_place_id), Some(member_place_id)) =
                (self.type_place_map.get(&owner_id), member_place_id)
            {
                self.create_holds_transition(owner_id, member_id, *owner_place_id, member_place_id);
            }
        }
    }

    /// 为类型创建或获取 Place，避免重复创建
    /// 返回 PlaceId，如果类型无需创建 Place 则返回 None
    fn create_or_get_type_place(&mut self, ty: &Type, field_id: &Id) -> Option<PlaceId> {
        match ty {
            Type::Primitive(name) => {
                let type_key = format!("primitive:{}", name);
                if let Some(&place_id) = self.type_cache.get(&type_key) {
                    return Some(place_id);
                }

                let place = Place::new(
                    *field_id,
                    name.clone(),
                    format!("primitive::{}", name),
                    PlaceKind::Primitive(name.clone()),
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_cache.insert(type_key, place_id);
                Some(place_id)
            }
            Type::Tuple(types) => {
                let type_key = format!("tuple:{}", self.format_type(ty));
                if let Some(&place_id) = self.type_cache.get(&type_key) {
                    return Some(place_id);
                }

                // 递归为 tuple 中的每个类型创建 Place
                let mut inner_info = Vec::new();
                for inner_type in types.iter() {
                    let dummy_id = self.generate_temp_id();
                    if let Some(inner_place_id) =
                        self.create_or_get_type_place(inner_type, &dummy_id)
                    {
                        inner_info.push((dummy_id, inner_place_id));
                    }
                }

                let place = Place::new(
                    *field_id,
                    format!("Tuple{}", types.len()),
                    format!("tuple::({})", self.format_type(ty)),
                    PlaceKind::Tuple(types.clone()),
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_cache.insert(type_key.clone(), place_id);

                // 创建 tuple 到其元素的 holds 关系
                for (dummy_id, inner_place_id) in inner_info {
                    self.create_holds_transition(*field_id, dummy_id, place_id, inner_place_id);
                }

                Some(place_id)
            }
            Type::Slice(inner_type) => {
                let type_key = format!("slice:{}", self.format_type(ty));
                if let Some(&place_id) = self.type_cache.get(&type_key) {
                    return Some(place_id);
                }

                // 为 slice 的元素类型创建 Place
                let dummy_id = self.generate_temp_id();
                let inner_place_id = self.create_or_get_type_place(inner_type, &dummy_id);

                let place = Place::new(
                    *field_id,
                    format!("[{}]", self.format_type(inner_type)),
                    format!("slice::[{}]", self.format_type(inner_type)),
                    PlaceKind::Slice(inner_type.clone()),
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_cache.insert(type_key, place_id);

                // 创建 slice 到其元素的 holds 关系
                if let Some(inner_place_id) = inner_place_id {
                    self.create_holds_transition(*field_id, dummy_id, place_id, inner_place_id);
                }

                Some(place_id)
            }
            Type::Array { type_, len } => {
                let type_key = format!("array:{}:{}", self.format_type(type_), len);
                if let Some(&place_id) = self.type_cache.get(&type_key) {
                    return Some(place_id);
                }

                // 为 array 的元素类型创建 Place
                let dummy_id = self.generate_temp_id();
                let inner_place_id = self.create_or_get_type_place(type_, &dummy_id);

                let place = Place::new(
                    *field_id,
                    format!("[{}; {}]", self.format_type(type_), len),
                    format!("array::[{}; {}]", self.format_type(type_), len),
                    PlaceKind::Array(type_.clone(), len.clone()),
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_cache.insert(type_key, place_id);

                // 创建 array 到其元素的 holds 关系
                if let Some(inner_place_id) = inner_place_id {
                    self.create_holds_transition(*field_id, dummy_id, place_id, inner_place_id);
                }

                Some(place_id)
            }
            Type::Infer => {
                let type_key = "infer:_".to_string();
                if let Some(&place_id) = self.type_cache.get(&type_key) {
                    return Some(place_id);
                }

                let place = Place::new(
                    *field_id,
                    "_".to_string(),
                    "infer::_".to_string(),
                    PlaceKind::Infer,
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_cache.insert(type_key, place_id);
                Some(place_id)
            }
            Type::RawPointer { is_mutable, type_ } => {
                let mutability = if *is_mutable { "mut" } else { "const" };
                let type_key = format!("rawptr:{}:{}", mutability, self.format_type(type_));
                if let Some(&place_id) = self.type_cache.get(&type_key) {
                    return Some(place_id);
                }

                // 为指针的目标类型创建 Place
                let dummy_id = self.generate_temp_id();
                let inner_place_id = self.create_or_get_type_place(type_, &dummy_id);

                let place = Place::new(
                    *field_id,
                    format!("*{} {}", mutability, self.format_type(type_)),
                    format!("rawptr::*{} {}", mutability, self.format_type(type_)),
                    PlaceKind::RawPointer(type_.clone(), *is_mutable),
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_cache.insert(type_key, place_id);

                // 创建 rawptr 到其目标的 holds 关系
                if let Some(inner_place_id) = inner_place_id {
                    self.create_holds_transition(*field_id, dummy_id, place_id, inner_place_id);
                }

                Some(place_id)
            }
            Type::BorrowedRef {
                is_mutable,
                type_,
                lifetime,
            } => {
                let mutability = if *is_mutable { " mut" } else { "" };
                let lifetime_str = lifetime.as_deref().unwrap_or("");
                let type_key = format!(
                    "borrowedref:{}{}:{}",
                    lifetime_str,
                    mutability,
                    self.format_type(type_)
                );
                if let Some(&place_id) = self.type_cache.get(&type_key) {
                    return Some(place_id);
                }

                // 为引用的目标类型创建 Place
                let dummy_id = self.generate_temp_id();
                let inner_place_id = self.create_or_get_type_place(type_, &dummy_id);

                let place = Place::new(
                    *field_id,
                    format!(
                        "&{}{} {}",
                        lifetime_str,
                        mutability,
                        self.format_type(type_)
                    ),
                    format!(
                        "borrowedref::&{}{} {}",
                        lifetime_str,
                        mutability,
                        self.format_type(type_)
                    ),
                    PlaceKind::BorrowedRef(type_.clone(), *is_mutable, lifetime.clone()),
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_cache.insert(type_key, place_id);

                // 创建 borrowedref 到其目标的 holds 关系
                if let Some(inner_place_id) = inner_place_id {
                    self.create_holds_transition(*field_id, dummy_id, place_id, inner_place_id);
                }

                Some(place_id)
            }
            Type::ResolvedPath(path) => {
                // 如果是已知的类型，直接返回其 PlaceId
                self.type_place_map.get(&path.id).copied()
            }
            _ => {
                // 其他类型暂不处理
                warn!("未处理的类型: {:?}", ty);
                None
            }
        }
    }

    /// 创建 holds transition 连接 owner 和 member
    fn create_holds_transition(
        &mut self,
        owner_id: Id,
        member_id: Id,
        owner_place_id: PlaceId,
        member_place_id: PlaceId,
    ) {
        let transition_id_val = self.generate_temp_id();
        let transition = Transition::new(
            transition_id_val,
            format!("holds"),
            TransitionKind::Hold(owner_id, member_id),
        );
        let transition_id = self.net.add_transition_and_get_id(transition);

        // 添加边：owner -> transition -> member
        self.net.add_flow(
            owner_place_id,
            transition_id,
            Flow {
                weight: 1,
                param_type: "owner".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );
        self.net.add_flow_from_transition(
            transition_id,
            member_place_id,
            Flow {
                weight: 1,
                param_type: "member".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );
    }

    /// 将 Type 转换为字符串表示
    fn format_type(&self, ty: &Type) -> String {
        match ty {
            Type::Primitive(name) => name.clone(),
            Type::Tuple(types) => {
                let types_str = types
                    .iter()
                    .map(|t| self.format_type(t))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({})", types_str)
            }
            Type::Slice(inner) => format!("[{}]", self.format_type(inner)),
            Type::Array { type_, len } => format!("[{}; {}]", self.format_type(type_), len),
            Type::Infer => "_".to_string(),
            Type::RawPointer { is_mutable, type_ } => {
                let mutability = if *is_mutable { "mut" } else { "const" };
                format!("*{} {}", mutability, self.format_type(type_))
            }
            Type::BorrowedRef {
                is_mutable,
                type_,
                lifetime,
            } => {
                let mutability = if *is_mutable { " mut" } else { "" };
                let lifetime_str = lifetime.as_deref().unwrap_or("");
                format!(
                    "&{}{} {}",
                    lifetime_str,
                    mutability,
                    self.format_type(type_)
                )
            }
            Type::ResolvedPath(path) => Self::format_path(path),
            Type::Generic(name) => name.clone(),
            _ => format!("{:?}", ty),
        }
    }

    /// 将 Path 转换为字符串
    fn format_path(path: &RustdocPath) -> String {
        path.path.clone()
    }

    /// 生成临时 ID
    fn generate_temp_id(&mut self) -> Id {
        let id = Id(self.next_temp_id);
        self.next_temp_id = self.next_temp_id.wrapping_sub(1);
        id
    }

    /// 推断自由函数的上下文（暂时返回 None）
    fn infer_free_function_context(&self, _item: &Item) -> Option<FunctionContext> {
        None
    }

    /// 处理函数，创建 transition
    fn ingest_function_with_context(
        &mut self,
        _item: &Item,
        _func: &Function,
        _context: FunctionContext,
    ) {
        // TODO: 实现函数处理逻辑
        // 这里需要根据函数的参数和返回值创建 transition，并连接相应的 Place
    }
}

/// 函数上下文，用于记录函数的类型信息
#[derive(Clone, Debug)]
enum FunctionContext {
    FreeFunction,
    InherentMethod {
        receiver_id: Id,
    },
    TraitImplementation {
        receiver_id: Id,
        trait_path: Arc<str>,
    },
}
