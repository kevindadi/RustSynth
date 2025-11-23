use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use log::{debug, info, warn};
use rustdoc_types::{
    Crate, Function, GenericParamDefKind, Id, Impl, Item, ItemEnum, Type};

use crate::petri::log::{analyze_generic_partial_order, generate_type_log};
use crate::petri::utils::{
    extract_borrow_info, format_path, format_type, is_standard_trait, is_std_library_type, resolve_std_trait_type, type_has_generic
};

use super::net::{PetriNet, PlaceId};
use super::structure::{BorrowKind, Flow, Place, PlaceKind, Transition, TransitionKind};

pub struct PetriNetBuilder<'a> {
    pub(super) crate_: &'a Crate,
    pub(super) net: PetriNet,
    /// 映射 Item ID 到 PlaceId,用于跟踪已创建的类型 Place
    pub(super) type_place_map: HashMap<Id, PlaceId>,
    /// 用于跟踪哪些函数是 impl 块中的方法,避免重复处理
    pub(super) impl_function_ids: HashSet<Id>,
    /// 用于跟踪已创建的 Type Place,避免重复创建
    /// 键是类型的字符串表示
    pub(super) type_cache: HashMap<String, PlaceId>,
    /// 用于跟踪泛型参数的占位符 Place
    /// 键是 (泛型名, 约束trait_id列表的排序后的元组)
    /// 无约束的泛型参数使用空列表,有约束的根据约束集合创建不同的库所
    pub(super) generic_param_cache: HashMap<(String, Vec<Id>), PlaceId>,
    /// 用于跟踪 Projection Place(QualifiedPath 的规范化表示)
    /// 键是 (self_type_id, trait_id, assoc_name),保证相同的 Projection 只创建一次
    pub(super) projection_cache: HashMap<(Id, Id, String), PlaceId>,
    /// 用于生成临时 ID 的计数器
    pub(super) next_temp_id: u32,
}

impl<'a> PetriNetBuilder<'a> {
    pub fn new(crate_: &'a Crate) -> Self {
        Self {
            crate_,
            net: PetriNet::new(),
            type_place_map: HashMap::new(),
            impl_function_ids: HashSet::new(),
            type_cache: HashMap::new(),
            generic_param_cache: HashMap::new(),
            projection_cache: HashMap::new(),
            next_temp_id: u32::MAX - 1_000_000, // 从一个大数字开始,避免与真实 ID 冲突
        }
    }

    pub fn from_crate(crate_: &'a Crate) -> PetriNet {
        Self::from_crate_with_log(crate_, None)
    }

    pub fn from_crate_with_log(crate_: &'a Crate, type_log_path: Option<&PathBuf>) -> PetriNet {
        Self::from_crate_with_logs(crate_, type_log_path, None)
    }

    pub fn from_crate_with_logs(
        crate_: &'a Crate,
        type_log_path: Option<&PathBuf>,
        generic_order_log_path: Option<&PathBuf>,
    ) -> PetriNet {
        let mut builder = Self::new(crate_);
        builder.ingest();

        // 生成类型日志
        if let Some(log_path) = type_log_path {
            if let Err(e) = generate_type_log(&builder, log_path) {
                warn!("⚠️  无法生成类型日志文件: {}", e);
            } else {
                info!("📋 类型日志已保存到: {}", log_path.display());
            }
        }

        // 生成泛型偏序关系分析日志
        if let Some(log_path) = generic_order_log_path {
            if let Err(e) = analyze_generic_partial_order(&builder, log_path) {
                warn!("⚠️  无法生成泛型偏序关系分析文件: {}", e);
            } else {
                info!("🔗 泛型偏序关系分析已保存到: {}", log_path.display());
            }
        }

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

        // Step 1: 遍历所有 Struct、Enum、Union、Trait、Variant 等类型定义,为它们创建 Place
        info!("📦 步骤 1/4: 创建类型定义的 Place (Struct/Enum/Union/Trait/Variant)");
        let mut type_count = 0;
        for item in self.crate_.index.values() {
            match &item.inner {
                ItemEnum::Struct(_)
                | ItemEnum::Enum(_)
                | ItemEnum::Union(_)
                | ItemEnum::Trait(_)
                | ItemEnum::Variant(_) => {
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

        // 首先,遍历所有 impl 块,为还没有创建 Place 的类型自动创建
        info!("   🔍 自动发现并创建有 impl 的类型...");
        let mut auto_created_count = 0;
        for item in self.crate_.index.values() {
            if let ItemEnum::Impl(impl_block) = &item.inner {
                // 检查 impl 的接收者类型
                if let Type::ResolvedPath(path) = &impl_block.for_ {
                    // 如果这个类型还没有创建 Place,自动创建一个
                    if !self.type_place_map.contains_key(&path.id) {
                        if let Some(type_item) = self.crate_.index.get(&path.id) {
                            // 尝试为这个类型创建 Place
                            let created = self.create_type_place_for_impl(type_item, impl_block);
                            if created {
                                auto_created_count += 1;
                                debug!(
                                    "   自动为类型 '{}' (ID: {:?}) 创建 Place",
                                    type_item.name.as_deref().unwrap_or("?"),
                                    type_item.id
                                );
                            }
                        } else {
                            // 类型不在 index 中,可能是标准库类型
                            if is_std_library_type(&path.path) {
                                // 创建标准库类型的 Place
                                let temp_id = self.generate_temp_id();
                                if let Some(_place_id) =
                                    self.handle_std_library_type(path, &temp_id, item.id)
                                {
                                    auto_created_count += 1;
                                    debug!(
                                        "   自动为标准库类型 '{}' (ID: {:?}) 创建 Place",
                                        path.path, path.id
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        if auto_created_count > 0 {
            info!(
                "   ✅ 自动创建了 {} 个有 impl 的类型 Place",
                auto_created_count
            );
        }

        // 现在收集所有需要处理的 impl 块
        // 直接遍历所有 impl 块,如果其接收者有 Place 或者是 blanket impl,就处理它
        let mut impl_items_to_process: Vec<(Id, &Impl)> = Vec::new();
        for item in self.crate_.index.values() {
            if let ItemEnum::Impl(impl_block) = &item.inner {
                // 检查 impl 的接收者是否有对应的 Place
                if let Type::ResolvedPath(path) = &impl_block.for_ {
                    if self.type_place_map.contains_key(&path.id) {
                        impl_items_to_process.push((item.id, impl_block));
                    }
                } else {
                    // 非 ResolvedPath 的 impl(blanket impl)也需要处理
                    impl_items_to_process.push((item.id, impl_block));
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

        // Step 4: 处理 Trait 方法（已跳过，trait 定义中的方法已在 create_trait_items_places 中处理）
        // trait 定义中的方法只保留函数签名，不创建独立的 Transition
        // 只有 impl 块中实现的方法才会创建 Transition
        info!("⚙️  步骤 4/5: 处理 Trait 方法（已跳过，trait 定义中的方法只保留签名）");

        // Step 5: 处理无约束函数
        info!("⚙️  步骤 5/5: 处理无约束函数");
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
        // 不再创建 wrapper transitions,因为不再需要基本类型
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
            // 处理 blanket impl(例如 impl<S: Trait> Trait for &mut S)
            // 对于泛型实现,我们需要为泛型参数创建库所
            info!(
                "🔍 发现 blanket impl (ID: {:?}), for_ = {:?}",
                item.id, impl_block.for_
            );
            self.handle_blanket_impl(item, impl_block);
            return;
        };

        // 检查是否实现了 trait,如果是,创建 impls 边
        if let Some(trait_path) = &impl_block.trait_ {
            // 检查 trait 是否有对应的 Place
            if let (Some(&impl_place_id), Some(&trait_place_id)) = (
                self.type_place_map.get(&receiver_id),
                self.type_place_map.get(&trait_path.id),
            ) {
                // 创建 impls 变迁:实现类型 -> trait
                self.create_impls_transition(
                    receiver_id,
                    trait_path.id,
                    impl_place_id,
                    trait_place_id,
                );
            }
        }

        let context = if let Some(trait_path) = &impl_block.trait_ {
            let trait_path_str = Arc::<str>::from(format_path(trait_path));
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

    /// 获取 Trait 的名称
    pub(super) fn get_trait_name(&self, trait_id: Id) -> String {
        self.crate_
            .index
            .get(&trait_id)
            .and_then(|item| item.name.clone())
            .unwrap_or_else(|| format!("Trait_{:?}", trait_id))
    }

  
 

    /// 获取类型的名称(用于显示)
    pub(super) fn get_type_name(&self, type_id: &Id) -> Option<String> {
        if let Some(item) = self.crate_.index.get(type_id) {
            item.name.clone()
        } else {
            None
        }
    }

    /// 处理 blanket impl(泛型实现)
    /// 例如 impl<S: StrConsumer> StrConsumer for &mut S
    fn handle_blanket_impl(&mut self, item: &Item, impl_block: &Impl) {
        info!("🔧 处理 blanket impl (ID: {:?})", item.id);

        // 找到主要的泛型参数(通常是第一个,或者是在 for_ 类型中使用的那个)
        let mut primary_generic_id = None;

        // 为 impl 的泛型参数创建 Place
        for param_def in &impl_block.generics.params {
            if let GenericParamDefKind::Type { bounds, .. } = &param_def.kind {
                let generic_name = param_def.name.clone();

                // 提取约束的 trait IDs
                let mut constraint_trait_ids = Vec::new();
                for bound in bounds {
                    if let rustdoc_types::GenericBound::TraitBound { trait_, .. } = bound {
                        constraint_trait_ids.push(trait_.id);
                    }
                }
                // 排序以保证相同约束集合使用相同的 key
                constraint_trait_ids.sort();

                // 使用 (generic_name, constraint_trait_ids) 作为 key
                let cache_key = (generic_name.clone(), constraint_trait_ids.clone());

                // 检查是否已经创建过这个约束集合的泛型参数 Place
                let generic_place_id = if let Some(&place_id) =
                    self.generic_param_cache.get(&cache_key)
                {
                    // 已存在，重用
                    place_id
                } else {
                    // 创建新的泛型参数占位符
                    let generic_id = self.generate_temp_id();
                    let constraint_str = if constraint_trait_ids.is_empty() {
                        "".to_string()
                    } else {
                        let trait_names: Vec<String> = constraint_trait_ids
                            .iter()
                            .filter_map(|trait_id| self.get_type_name(trait_id))
                            .collect();
                        if trait_names.is_empty() {
                            format!(": {:?}", constraint_trait_ids)
                        } else {
                            format!(": {}", trait_names.join(" + "))
                        }
                    };
                    let generic_place = Place::new(
                        generic_id,
                        format!("{}{}", generic_name, constraint_str),
                        format!(
                            "generic_param::{}::{:?}",
                            generic_name, constraint_trait_ids
                        ),
                        PlaceKind::GenericParam(generic_name.clone(), constraint_trait_ids.clone()),
                    );
                    let place_id = self.net.add_place_and_get_id(generic_place);
                    self.generic_param_cache.insert(cache_key, place_id);

                    // 为有约束的泛型参数创建 impls 变迁链接到 trait
                    for trait_id in &constraint_trait_ids {
                        if let Some(&trait_place_id) = self.type_place_map.get(trait_id) {
                            self.create_impls_transition(
                                generic_id,
                                *trait_id,
                                place_id,
                                trait_place_id,
                            );
                            debug!(
                                "✨ 创建 blanket impl 泛型 '{}' 实现 trait {:?}",
                                generic_name, trait_id
                            );
                        }
                    }

                    place_id
                };

                // 记录第一个泛型参数作为主要的 receiver
                if primary_generic_id.is_none() {
                    // 为泛型参数生成一个临时 ID 用于 receiver
                    let generic_id = self.generate_temp_id();
                    primary_generic_id = Some(generic_id);
                    // 同时存入 type_place_map,这样可以作为 receiver_id 使用
                    self.type_place_map.insert(generic_id, generic_place_id);
                }
            }
        }

        // 对于 blanket impl 中的方法,Self 应该指向被实现的 trait
        // 例如 impl<S: StrConsumer> StrConsumer for &mut S 中,
        // consume 方法的 self 应该指向 StrConsumer trait
        let receiver_id = if let Some(trait_path) = &impl_block.trait_ {
            // 检查 trait 是否存在 Place,如果不存在则创建
            if !self.type_place_map.contains_key(&trait_path.id) {
                // 尝试为 trait 创建 Place
                if let Some(trait_item) = self.crate_.index.get(&trait_path.id) {
                    self.create_type_place(trait_item);
                    info!(
                        "✨ 为 blanket impl 自动创建 trait Place: '{}' (ID: {:?})",
                        trait_path.path, trait_path.id
                    );
                }
            }
            Some(trait_path.id)
        } else {
            // 如果没有实现 trait,使用第一个泛型参数
            primary_generic_id
        };

        if let Some(receiver_id) = receiver_id {
            let context = if let Some(trait_path) = &impl_block.trait_ {
                let trait_path_str = Arc::<str>::from(format_path(trait_path));
                FunctionContext::TraitImplementation {
                    receiver_id,
                    trait_path: trait_path_str,
                }
            } else {
                FunctionContext::InherentMethod { receiver_id }
            };

            for item_id in &impl_block.items {
                if let Some(inner_item) = self.crate_.index.get(item_id) {
                    if let ItemEnum::Function(func) = &inner_item.inner {
                        self.impl_function_ids.insert(inner_item.id);
                        self.ingest_function_with_context(inner_item, func, context.clone());

                        debug!(
                            "✨ 处理 blanket impl 中的函数 '{}' (ID: {:?}),Self 指向 {:?}",
                            inner_item.name.as_deref().unwrap_or("?"),
                            inner_item.id,
                            receiver_id
                        );
                    }
                }
            }
        }
    }
 

    /// 判断是否应该跳过某个函数,不为其创建变迁
    ///
    /// 跳过的函数包括:type_id, fmt, fmt_debug, fmt_display 等
    fn should_skip_function(&self, func_name: &str) -> bool {
        matches!(func_name, "type_id" | "fmt" | "fmt_debug" | "fmt_display")
    }

    /// 生成临时 ID
    pub(super) fn generate_temp_id(&mut self) -> Id {
        let id = Id(self.next_temp_id);
        self.next_temp_id = self.next_temp_id.wrapping_sub(1);
        id
    }

    /// 推断自由函数的上下文(暂时返回 None)
    fn infer_free_function_context(&self, _item: &Item) -> Option<FunctionContext> {
        None
    }

    /// 处理函数,创建 transition
    fn ingest_function_with_context(
        &mut self,
        item: &Item,
        func: &Function,
        context: FunctionContext,
    ) {
        let func_name = item.name.as_deref().unwrap_or("anonymous");

        // 检查是否应该跳过此函数
        if self.should_skip_function(func_name) {
            debug!("⏭️  跳过函数 '{}' (ID: {:?})", func_name, item.id);
            return;
        }

        // 创建函数的 Transition
        let transition = Transition::new(
            item.id,
            func_name.to_string(),
            TransitionKind::Function(func.clone()),
        );
        let transition_id = self.net.add_transition_and_get_id(transition);

        // 获取 receiver_id 和 trait_path 用于解析 Self 和泛型
        let (receiver_id, trait_path) = match &context {
            FunctionContext::InherentMethod { receiver_id } => (Some(*receiver_id), None),
            FunctionContext::TraitImplementation {
                receiver_id,
                trait_path,
            } => (Some(*receiver_id), Some(trait_path.as_ref())),
            FunctionContext::FreeFunction => (None, None),
        };

        // 检查是否是标准库 trait 实现
        let is_std_trait = trait_path
            .map(|path| is_standard_trait(path, func_name))
            .unwrap_or(false);

        // 为函数的泛型参数创建占位符(如果需要且不是标准 trait)
        let func_generic_count = if !is_std_trait {
            self.create_function_generic_params(item.id, func, transition_id)
        } else {
            // 标准 trait 不创建泛型占位符,因为它们会被特殊处理
            0
        };

        // 自由函数的泛型参数现在可以正确建模了(通过 GenericParam Place 和 impls 变迁)
        if receiver_id.is_none() && func_generic_count > 0 {
            debug!(
                "✨ 自由函数 '{}' (ID: {:?}) 有 {} 个泛型参数,已创建 GenericParam Place",
                func_name, item.id, func_generic_count
            );
        }

        // 收集缺失的类型信息
        let mut missing_types = Vec::new();

        // 处理输入参数
        for (param_name, param_type) in &func.sig.inputs {
            // 提取借用信息和实际类型
            let (borrow_kind, actual_type) = extract_borrow_info(param_type);

            // 特殊处理标准 trait 的类型映射
            let resolved_type = if is_std_trait {
                resolve_std_trait_type(actual_type, func_name, receiver_id, trait_path)
            } else {
                actual_type
            };

            // 查找参数类型对应的 Place
            // 传入 receiver_id 用于解析 Self,传入 item.id 用于解析函数级别的泛型
            if let Some(place_id) =
                self.find_type_place_in_function(resolved_type, item.id, receiver_id)
            {
                // 创建 Place -> Transition 的边
                self.net.add_flow(
                    place_id,
                    transition_id,
                    Flow {
                        weight: 1,
                        param_type: param_name.clone(),
                        borrow_kind,
                    },
                );
            } else {
                // 类型对应的 Place 不存在,记录警告
                let type_str = format_type(actual_type);
                warn!(
                    "函数 '{}' (ID: {:?}) 的参数 '{}' 类型 '{}' 对应的 Place 不存在",
                    func_name, item.id, param_name, type_str
                );
                missing_types.push(format!("参数 '{}': {}", param_name, type_str));
            }
        }

        // 处理返回值
        if let Some(return_type) = &func.sig.output {
            // 返回值通常是 owned(除非是引用)
            let (borrow_kind, actual_type) = extract_borrow_info(return_type);

            // 特殊处理标准 trait 的类型映射
            let resolved_type = if is_std_trait {
                resolve_std_trait_type(actual_type, func_name, receiver_id, trait_path)
            } else {
                actual_type
            };

            // 查找返回值类型对应的 Place
            if let Some(place_id) =
                self.find_type_place_in_function(resolved_type, item.id, receiver_id)
            {
                // 创建 Transition -> Place 的边
                self.net.add_flow_from_transition(
                    transition_id,
                    place_id,
                    Flow {
                        weight: 1,
                        param_type: "return".to_string(),
                        borrow_kind,
                    },
                );
            } else {
                // 返回值类型对应的 Place 不存在,记录警告
                    let type_str = format_type(actual_type);
                warn!(
                    "函数 '{}' (ID: {:?}) 的返回值类型 '{}' 对应的 Place 不存在",
                    func_name, item.id, type_str
                );
                missing_types.push(format!("返回值: {}", type_str));
            }
        }

        // 如果有缺失类型,输出汇总信息
        if !missing_types.is_empty() {
            debug!(
                "函数 '{}' 有 {} 个缺失的类型: {}",
                func_name,
                missing_types.len(),
                missing_types.join(", ")
            );
        }
    }

    /// 为函数的泛型参数创建占位符
    /// 函数级别的泛型参数(如 `fn foo<T>()` 中的 T)会被创建为独立的 Place
    ///
    /// # 返回值
    /// 返回创建的泛型参数数量
    fn create_function_generic_params(
        &mut self,
        function_id: Id,
        func: &Function,
        transition_id: crate::petri::net::TransitionId,
    ) -> usize {
        let mut created_count = 0;

        for param_def in &func.generics.params {
            // 只处理类型参数
            if let GenericParamDefKind::Type { bounds, .. } = &param_def.kind {
                let generic_name = param_def.name.clone();

                // 提取约束的 trait IDs
                let mut constraint_trait_ids = Vec::new();
                for bound in bounds {
                    if let rustdoc_types::GenericBound::TraitBound { trait_, .. } = bound {
                        constraint_trait_ids.push(trait_.id);
                    }
                }
                // 排序以保证相同约束集合使用相同的 key
                constraint_trait_ids.sort();

                // 使用 (generic_name, constraint_trait_ids) 作为 key
                let cache_key = (generic_name.clone(), constraint_trait_ids.clone());

                // 检查是否已经创建过这个约束集合的泛型参数 Place
                let generic_place_id = if let Some(&place_id) =
                    self.generic_param_cache.get(&cache_key)
                {
                    // 已存在，重用
                    place_id
                } else {
                    // 创建新的泛型参数占位符 Place
                    let generic_id = self.generate_temp_id();
                    let constraint_str = if constraint_trait_ids.is_empty() {
                        "".to_string()
                    } else {
                        let trait_names: Vec<String> = constraint_trait_ids
                            .iter()
                            .filter_map(|trait_id| self.get_type_name(trait_id))
                            .collect();
                        if trait_names.is_empty() {
                            format!(": {:?}", constraint_trait_ids)
                        } else {
                            format!(": {}", trait_names.join(" + "))
                        }
                    };
                    let generic_place = Place::new(
                        generic_id,
                        format!("{}{}", generic_name, constraint_str),
                        format!(
                            "generic_param::{}::{:?}",
                            generic_name, constraint_trait_ids
                        ),
                        PlaceKind::GenericParam(generic_name.clone(), constraint_trait_ids.clone()),
                    );
                    let place_id = self.net.add_place_and_get_id(generic_place);

                    // 缓存泛型参数 Place
                    self.generic_param_cache.insert(cache_key, place_id);

                    // 为有约束的泛型参数创建 impls 变迁链接到 trait
                    for trait_id in &constraint_trait_ids {
                        if let Some(&trait_place_id) = self.type_place_map.get(trait_id) {
                            self.create_impls_transition(
                                generic_id,
                                *trait_id,
                                place_id,
                                trait_place_id,
                            );
                            debug!(
                                "✨ 函数 '{}' 的泛型 '{}' 约束于 trait {:?}",
                                function_id.0, generic_name, trait_id
                            );
                        }
                    }

                    place_id
                };

                created_count += 1;

                // 创建从 transition 到泛型参数的连接
                // 表示这个函数使用了这个泛型参数
                self.net.add_flow_from_transition(
                    transition_id,
                    generic_place_id,
                    Flow {
                        weight: 1,
                        param_type: format!("generic<{}>", generic_name),
                        borrow_kind: BorrowKind::Owned,
                    },
                );
            }
        }

        created_count
    }

   
}

/// 函数上下文,用于记录函数的类型信息
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
