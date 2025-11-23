use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use log::{debug, info, warn};
use rustdoc_types::{
    Crate, Function, GenericParamDefKind, Id, Impl, Item, ItemEnum, Path as RustdocPath,
    StructKind, Type, VariantKind,
};

use super::net::{PetriNet, PlaceId};
use super::structure::{BorrowKind, Flow, Place, PlaceKind, Transition, TransitionKind};

pub struct PetriNetBuilder<'a> {
    crate_: &'a Crate,
    net: PetriNet,
    /// 映射 Item ID 到 PlaceId,用于跟踪已创建的类型 Place
    type_place_map: HashMap<Id, PlaceId>,
    /// 用于跟踪哪些函数是 impl 块中的方法,避免重复处理
    impl_function_ids: HashSet<Id>,
    /// 用于跟踪已创建的 Type Place,避免重复创建
    /// 键是类型的字符串表示
    type_cache: HashMap<String, PlaceId>,
    /// 用于跟踪泛型参数的占位符 Place
    /// 键是 (泛型名, 约束trait_id列表的排序后的元组)
    /// 无约束的泛型参数使用空列表,有约束的根据约束集合创建不同的库所
    generic_param_cache: HashMap<(String, Vec<Id>), PlaceId>,
    /// 用于跟踪 Projection Place(QualifiedPath 的规范化表示)
    /// 键是 (self_type_id, trait_id, assoc_name),保证相同的 Projection 只创建一次
    projection_cache: HashMap<(Id, Id, String), PlaceId>,
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
            generic_param_cache: HashMap::new(),
            projection_cache: HashMap::new(),
            next_temp_id: u32::MAX - 1_000_000, // 从一个大数字开始,避免与真实 ID 冲突
        }
    }

    /// 标准库/核心库类型白名单
    /// 这些类型会自动创建 Place
    fn is_std_library_type(&self, path: &str) -> bool {
        matches!(
            path,
            // 基本字符串类型
            "String" | "alloc::string::String" | "std::string::String"
            | "str" | "alloc::str" | "std::str"
            // 集合类型
            | "Vec" | "alloc::vec::Vec" | "std::vec::Vec"
            | "VecDeque" | "alloc::collections::VecDeque" | "std::collections::VecDeque"
            | "LinkedList" | "alloc::collections::LinkedList" | "std::collections::LinkedList"
            | "HashMap" | "std::collections::HashMap"
            | "HashSet" | "std::collections::HashSet"
            | "BTreeMap" | "alloc::collections::BTreeMap" | "std::collections::BTreeMap"
            | "BTreeSet" | "alloc::collections::BTreeSet" | "std::collections::BTreeSet"
            // 智能指针
            | "Box" | "alloc::boxed::Box" | "std::boxed::Box"
            | "Rc" | "alloc::rc::Rc" | "std::rc::Rc"
            | "Arc" | "alloc::sync::Arc" | "std::sync::Arc"
            | "Cow" | "alloc::borrow::Cow" | "std::borrow::Cow"
            // 基本数值类型(虽然是 Primitive,但可能以 Path 形式出现)
            | "bool" | "u8" | "u16" | "u32" | "u64" | "u128" | "usize"
            | "i8" | "i16" | "i32" | "i64" | "i128" | "isize"
            | "f32" | "f64" | "char"
            // IO 类型
            | "io::Error" | "std::io::Error"
            | "io::Result" | "std::io::Result"
            // 格式化类型
            | "fmt::Error" | "std::fmt::Error" | "core::fmt::Error"
            | "fmt::Result" | "std::fmt::Result" | "core::fmt::Result"
            | "fmt::Formatter" | "std::fmt::Formatter" | "core::fmt::Formatter"
            | "fmt::Arguments" | "std::fmt::Arguments" | "core::fmt::Arguments"
            // 其他常用类型
            | "PathBuf" | "std::path::PathBuf"
            | "Path" | "std::path::Path"
            | "OsString" | "std::ffi::OsString"
            | "OsStr" | "std::ffi::OsStr"
            | "CString" | "std::ffi::CString"
            | "CStr" | "std::ffi::CStr"
            // 时间类型
            | "Duration" | "std::time::Duration" | "core::time::Duration"
            | "Instant" | "std::time::Instant"
            | "SystemTime" | "std::time::SystemTime"
            // 错误处理
            | "Error" | "std::error::Error"
            // 类型相关
            | "TypeId" | "std::any::TypeId" | "core::any::TypeId"
            | "PhantomData" | "std::marker::PhantomData" | "core::marker::PhantomData"
        )
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
            if let Err(e) = builder.generate_type_log(log_path) {
                warn!("⚠️  无法生成类型日志文件: {}", e);
            } else {
                info!("📋 类型日志已保存到: {}", log_path.display());
            }
        }

        // 生成泛型偏序关系分析日志
        if let Some(log_path) = generic_order_log_path {
            if let Err(e) = builder.analyze_generic_partial_order(log_path) {
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
                            if self.is_std_library_type(&path.path) {
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

        // Step 4: 处理 Trait 方法
        info!("⚙️  步骤 4/5: 处理 Trait 方法");
        let mut trait_method_count = 0;
        for item in self.crate_.index.values() {
            if let ItemEnum::Trait(trait_def) = &item.inner {
                let trait_id = item.id;
                for method_id in &trait_def.items {
                    if let Some(method_item) = self.crate_.index.get(method_id) {
                        if let ItemEnum::Function(func) = &method_item.inner {
                            // Trait 方法的上下文:receiver_id 是 trait 本身
                            let context = FunctionContext::InherentMethod {
                                receiver_id: trait_id,
                            };
                            self.impl_function_ids.insert(*method_id);
                            self.ingest_function_with_context(method_item, func, context);
                            trait_method_count += 1;
                        }
                    }
                }
            }
        }
        debug!("   处理了 {} 个 Trait 方法", trait_method_count);

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

    /// 生成类型日志文件,记录所有类型及其泛型约束
    pub fn generate_type_log(&self, log_path: &PathBuf) -> std::io::Result<()> {
        let mut file = File::create(log_path)?;
        writeln!(file, "📋 Rust 类型清单\n")?;

        // 收集所有类型
        let mut structs = Vec::new();
        let mut enums = Vec::new();
        let mut unions = Vec::new();
        let mut traits = Vec::new();
        let mut variants = Vec::new();
        let mut generics = Vec::new();
        let mut primitives = Vec::new();
        let mut others = Vec::new();

        for item in self.crate_.index.values() {
            let name = item.name.as_deref().unwrap_or("(匿名)");
            let id_str = format!("{:?}", item.id);

            match &item.inner {
                ItemEnum::Struct(s) => {
                    let generics_info = self.format_generics(&s.generics);
                    structs.push((name.to_string(), id_str, generics_info));
                }
                ItemEnum::Enum(e) => {
                    let generics_info = self.format_generics(&e.generics);
                    enums.push((name.to_string(), id_str, generics_info));
                }
                ItemEnum::Union(u) => {
                    let generics_info = self.format_generics(&u.generics);
                    unions.push((name.to_string(), id_str, generics_info));
                }
                ItemEnum::Trait(t) => {
                    let generics_info = self.format_generics(&t.generics);
                    traits.push((name.to_string(), id_str, generics_info));
                }
                ItemEnum::Variant(_v) => {
                    // Variant 没有自己的 generics,它继承自 Enum
                    variants.push((name.to_string(), id_str, String::new()));
                }
                ItemEnum::Primitive(_) => {
                    primitives.push((name.to_string(), id_str, String::new()));
                }
                _ => {
                    others.push((name.to_string(), id_str, String::new()));
                }
            }
        }

        // 收集泛型参数(从 generic_param_cache)
        for ((generic_name, constraint_trait_ids), _place_id) in &self.generic_param_cache {
            // 格式化约束信息
            let constraints = if constraint_trait_ids.is_empty() {
                String::new()
            } else {
                let trait_names: Vec<String> = constraint_trait_ids
                    .iter()
                    .filter_map(|trait_id| {
                        if let Some(item) = self.crate_.index.get(trait_id) {
                            item.name.clone()
                        } else {
                            None
                        }
                    })
                    .collect();
                if trait_names.is_empty() {
                    format!(": {:?}", constraint_trait_ids)
                } else {
                    format!(": {}", trait_names.join(" + "))
                }
            };

            generics.push((
                format!("{}{}", generic_name, constraints),
                format!("{:?}", constraint_trait_ids),
                constraints,
            ));
        }

        // 写入 Struct 类型
        if !structs.is_empty() {
            writeln!(file, "📦 Struct 类型 (共 {} 个):\n", structs.len())?;
            for (name, id, generics_info) in &structs {
                writeln!(file, "  • {} (ID: {})", name, id)?;
                if !generics_info.is_empty() {
                    writeln!(file, "    {}", generics_info)?;
                }
            }
            writeln!(file)?;
        }

        // 写入 Enum 类型
        if !enums.is_empty() {
            writeln!(file, "🔢 Enum 类型 (共 {} 个):\n", enums.len())?;
            for (name, id, generics_info) in &enums {
                writeln!(file, "  • {} (ID: {})", name, id)?;
                if !generics_info.is_empty() {
                    writeln!(file, "    {}", generics_info)?;
                }
            }
            writeln!(file)?;
        }

        // 写入 Union 类型
        if !unions.is_empty() {
            writeln!(file, "🔗 Union 类型 (共 {} 个):\n", unions.len())?;
            for (name, id, generics_info) in &unions {
                writeln!(file, "  • {} (ID: {})", name, id)?;
                if !generics_info.is_empty() {
                    writeln!(file, "    {}", generics_info)?;
                }
            }
            writeln!(file)?;
        }

        // 写入 Trait 类型
        if !traits.is_empty() {
            writeln!(file, "🎯 Trait 类型 (共 {} 个):\n", traits.len())?;
            for (name, id, generics_info) in &traits {
                writeln!(file, "  • {} (ID: {})", name, id)?;
                if !generics_info.is_empty() {
                    writeln!(file, "    {}", generics_info)?;
                }
            }
            writeln!(file)?;
        }

        // 写入 Variant 类型
        if !variants.is_empty() {
            writeln!(file, "🔀 Variant 类型 (共 {} 个):\n", variants.len())?;
            for (name, id, generics_info) in &variants {
                writeln!(file, "  • {} (ID: {})", name, id)?;
                if !generics_info.is_empty() {
                    writeln!(file, "    {}", generics_info)?;
                }
            }
            writeln!(file)?;
        }

        // 写入泛型参数
        if !generics.is_empty() {
            writeln!(file, "🔧 泛型参数 (共 {} 个):\n", generics.len())?;
            for (name, id, constraints) in &generics {
                writeln!(file, "  • {} (Owner ID: {})", name, id)?;
                if !constraints.is_empty() {
                    writeln!(file, "    约束: {}", constraints)?;
                } else {
                    writeln!(file, "    约束: 无")?;
                }
            }
            writeln!(file)?;
        }

        // 写入 Primitive 类型
        if !primitives.is_empty() {
            writeln!(file, "⚡ Primitive 类型 (共 {} 个):\n", primitives.len())?;
            for (name, id, _) in &primitives {
                writeln!(file, "  • {} (ID: {})", name, id)?;
            }
            writeln!(file)?;
        }

        // 写入其他类型
        if !others.is_empty() {
            writeln!(file, "📝 其他类型 (共 {} 个):\n", others.len())?;
            for (name, id, _) in &others {
                writeln!(file, "  • {} (ID: {})", name, id)?;
            }
            writeln!(file)?;
        }

        writeln!(
            file,
            "================================================================================\n"
        )?;
        writeln!(file, "📊 统计信息:\n")?;
        writeln!(file, "  • Struct:  {} 个", structs.len())?;
        writeln!(file, "  • Enum:    {} 个", enums.len())?;
        writeln!(file, "  • Union:   {} 个", unions.len())?;
        writeln!(file, "  • Trait:   {} 个", traits.len())?;
        writeln!(file, "  • Variant: {} 个", variants.len())?;
        writeln!(file, "  • 泛型参数: {} 个", generics.len())?;
        writeln!(file, "  • Primitive: {} 个", primitives.len())?;
        writeln!(file, "  • 其他:     {} 个", others.len())?;
        writeln!(
            file,
            "  • 总计:    {} 个类型\n",
            structs.len()
                + enums.len()
                + unions.len()
                + traits.len()
                + variants.len()
                + generics.len()
                + primitives.len()
                + others.len()
        )?;
        writeln!(
            file,
            "================================================================================\n"
        )?;

        Ok(())
    }

    /// 格式化泛型参数信息
    fn format_generics(&self, generics: &rustdoc_types::Generics) -> String {
        if generics.params.is_empty() {
            return String::new();
        }

        let mut parts = Vec::new();
        for param in &generics.params {
            match &param.kind {
                GenericParamDefKind::Type { bounds, .. } => {
                    let mut constraint_str = String::new();
                    if !bounds.is_empty() {
                        let bound_strs: Vec<String> = bounds
                            .iter()
                            .filter_map(|bound| {
                                if let rustdoc_types::GenericBound::TraitBound { trait_, .. } =
                                    bound
                                {
                                    Some(trait_.path.clone())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        if !bound_strs.is_empty() {
                            constraint_str = format!(": {}", bound_strs.join(" + "));
                        }
                    }
                    parts.push(format!("{}{}", param.name, constraint_str));
                }
                GenericParamDefKind::Lifetime { .. } => {
                    parts.push(format!("'{}", param.name));
                }
                GenericParamDefKind::Const { .. } => {
                    parts.push(format!("const {}", param.name));
                }
            }
        }

        if parts.is_empty() {
            String::new()
        } else {
            format!("泛型参数: <{}>", parts.join(", "))
        }
    }

    /// 查找泛型参数的约束 trait IDs
    fn find_generic_constraint_trait_ids(&self, owner_id: Id, generic_name: &str) -> Vec<Id> {
        // 查找 owner 的 generics 定义
        if let Some(item) = self.crate_.index.get(&owner_id) {
            let generics = match &item.inner {
                ItemEnum::Struct(s) => &s.generics,
                ItemEnum::Enum(e) => &e.generics,
                ItemEnum::Union(u) => &u.generics,
                ItemEnum::Trait(t) => &t.generics,
                ItemEnum::Function(f) => &f.generics,
                ItemEnum::Impl(i) => &i.generics,
                // Variant 没有自己的 generics,它继承自 Enum
                _ => return Vec::new(),
            };

            // 查找匹配的泛型参数
            for param in &generics.params {
                if param.name == generic_name {
                    if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                        let mut trait_ids = Vec::new();
                        for bound in bounds {
                            if let rustdoc_types::GenericBound::TraitBound { trait_, .. } = bound {
                                trait_ids.push(trait_.id);
                            }
                        }
                        trait_ids.sort();
                        return trait_ids;
                    }
                }
            }
        }

        Vec::new()
    }

    /// 分析泛型偏序关系
    /// 生成一个报告,显示哪些泛型可以使用哪些泛型,以及约束之间的层级关系
    pub fn analyze_generic_partial_order(&self, log_path: &PathBuf) -> std::io::Result<()> {
        use std::collections::{HashMap, HashSet};

        let mut file = File::create(log_path)?;
        writeln!(file, "🔗 泛型偏序关系分析\n")?;

        // Rust 编译器默认实现的 trait
        let default_traits: HashSet<&str> = [
            "Send",
            "Sync",
            "Sized",
            "Copy",
            "Clone",
            "Debug",
            "Display",
            "PartialEq",
            "Eq",
            "PartialOrd",
            "Ord",
            "Hash",
            "Default",
            "Drop",
            "Unpin",
            "UnwindSafe",
            "RefUnwindSafe",
        ]
        .iter()
        .cloned()
        .collect();

        // 收集所有泛型参数及其约束
        #[derive(Debug, Clone)]
        struct GenericInfo {
            #[allow(unused)]
            owner_id: Id,
            owner_name: String,
            generic_name: String,
            constraints: Vec<String>, // trait 路径列表
        }

        let mut generic_infos: Vec<GenericInfo> = Vec::new();

        // 从类型定义中收集泛型
        for item in self.crate_.index.values() {
            let owner_name = item.name.as_deref().unwrap_or("(匿名)").to_string();
            let generics = match &item.inner {
                ItemEnum::Struct(s) => Some(&s.generics),
                ItemEnum::Enum(e) => Some(&e.generics),
                ItemEnum::Union(u) => Some(&u.generics),
                ItemEnum::Trait(t) => Some(&t.generics),
                _ => None,
            };

            if let Some(generics) = generics {
                for param in &generics.params {
                    if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                        let constraints: Vec<String> = bounds
                            .iter()
                            .filter_map(|bound| {
                                if let rustdoc_types::GenericBound::TraitBound { trait_, .. } =
                                    bound
                                {
                                    Some(trait_.path.clone())
                                } else {
                                    None
                                }
                            })
                            .collect();

                        generic_infos.push(GenericInfo {
                            owner_id: item.id,
                            owner_name: owner_name.clone(),
                            generic_name: param.name.clone(),
                            constraints,
                        });
                    }
                }
            }
        }

        // 从函数和 impl 中收集泛型
        for item in self.crate_.index.values() {
            let owner_name = item.name.as_deref().unwrap_or("(匿名)").to_string();
            let generics = match &item.inner {
                ItemEnum::Function(f) => Some(&f.generics),
                ItemEnum::Impl(i) => Some(&i.generics),
                _ => None,
            };

            if let Some(generics) = generics {
                for param in &generics.params {
                    if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                        let constraints: Vec<String> = bounds
                            .iter()
                            .filter_map(|bound| {
                                if let rustdoc_types::GenericBound::TraitBound { trait_, .. } =
                                    bound
                                {
                                    Some(trait_.path.clone())
                                } else {
                                    None
                                }
                            })
                            .collect();

                        generic_infos.push(GenericInfo {
                            owner_id: item.id,
                            owner_name: owner_name.clone(),
                            generic_name: param.name.clone(),
                            constraints,
                        });
                    }
                }
            }
        }

        // 构建约束图:trait -> 满足该约束的泛型列表
        let mut trait_to_generics: HashMap<String, Vec<&GenericInfo>> = HashMap::new();
        for info in &generic_infos {
            for constraint in &info.constraints {
                trait_to_generics
                    .entry(constraint.clone())
                    .or_insert_with(Vec::new)
                    .push(info);
            }
        }

        // 分析偏序关系
        writeln!(file, "📊 泛型约束统计:\n")?;
        writeln!(file, "  总泛型参数数量: {}\n", generic_infos.len())?;

        // 按约束分组显示
        writeln!(file, "🎯 按 Trait 约束分组:\n")?;
        let mut sorted_traits: Vec<_> = trait_to_generics.iter().collect();
        sorted_traits.sort_by_key(|(trait_name, _)| *trait_name);

        for (trait_name, generics) in &sorted_traits {
            let is_default = default_traits.contains(trait_name.as_str());
            let default_marker = if is_default { " (默认 trait)" } else { "" };
            writeln!(file, "  • {}:{}", trait_name, default_marker)?;
            for info in *generics {
                writeln!(file, "    └─ {}.{}", info.owner_name, info.generic_name)?;
            }
            writeln!(file)?;
        }

        // 分析哪些泛型可以用于哪些约束
        writeln!(file, "🔗 泛型可用性分析:\n")?;
        writeln!(
            file,
            "  说明: 如果泛型 T 满足约束 A + B,那么 T 可以用于需要 A 或 B 的地方\n"
        )?;

        for info in &generic_infos {
            if info.constraints.is_empty() {
                writeln!(
                    file,
                    "  • {}.{}: 无约束 (可用于任何地方)",
                    info.owner_name, info.generic_name
                )?;
            } else {
                writeln!(
                    file,
                    "  • {}.{}: 满足 {}",
                    info.owner_name,
                    info.generic_name,
                    info.constraints.join(" + ")
                )?;
                writeln!(file, "    可以用于需要以下任一约束的地方:")?;
                for constraint in &info.constraints {
                    writeln!(file, "      - {}", constraint)?;
                }
            }
            writeln!(file)?;
        }

        // 分析约束层级关系(如果 T: A + B,且 A: C,那么 T 也满足 C)
        writeln!(file, "📈 约束层级关系:\n")?;
        writeln!(
            file,
            "  说明: 如果 T: A,且 A: B,那么 T 也满足 B (传递性)\n"
        )?;

        // 查找 trait 之间的继承关系(通过 impl 块)
        let mut trait_supertraits: HashMap<String, Vec<String>> = HashMap::new();
        for item in self.crate_.index.values() {
            if let ItemEnum::Trait(trait_def) = &item.inner {
                let trait_name = item.name.as_deref().unwrap_or("(匿名)").to_string();
                let supertraits: Vec<String> = trait_def
                    .bounds
                    .iter()
                    .filter_map(|bound| {
                        if let rustdoc_types::GenericBound::TraitBound { trait_, .. } = bound {
                            Some(trait_.path.clone())
                        } else {
                            None
                        }
                    })
                    .collect();

                if !supertraits.is_empty() {
                    trait_supertraits.insert(trait_name, supertraits);
                }
            }
        }

        if !trait_supertraits.is_empty() {
            writeln!(file, "  Trait 继承关系:\n")?;
            for (trait_name, supertraits) in &trait_supertraits {
                writeln!(file, "    {}: {:?}", trait_name, supertraits)?;
            }
            writeln!(file)?;
        } else {
            writeln!(file, "  (未发现显式的 trait 继承关系)\n")?;
        }

        // 分析同级别约束(满足相同约束集合的泛型)
        writeln!(file, "⚖️  同级别约束分析:\n")?;
        writeln!(file, "  说明: 满足相同约束集合的泛型被视为同级别\n")?;

        let mut constraint_groups: HashMap<Vec<String>, Vec<&GenericInfo>> = HashMap::new();
        for info in &generic_infos {
            let mut constraints = info.constraints.clone();
            constraints.sort();
            constraint_groups
                .entry(constraints)
                .or_insert_with(Vec::new)
                .push(info);
        }

        let mut sorted_groups: Vec<_> = constraint_groups.iter().collect();
        sorted_groups.sort_by_key(|(constraints, _)| constraints.len());
        sorted_groups.reverse(); // 从约束最多的开始

        for (constraints, generics) in &sorted_groups {
            if generics.len() > 1 {
                writeln!(file, "  约束集合: {}", constraints.join(" + "))?;
                writeln!(file, "  满足该约束的泛型 (共 {} 个):", generics.len())?;
                for info in *generics {
                    writeln!(file, "    • {}.{}", info.owner_name, info.generic_name)?;
                }
                writeln!(file)?;
            }
        }

        // 默认 trait 说明
        writeln!(file, "🔧 Rust 默认 Trait 说明:\n")?;
        writeln!(
            file,
            "  以下 trait 由 Rust 编译器自动实现(如果类型满足条件):\n"
        )?;
        for trait_name in &default_traits {
            writeln!(file, "    • {}", trait_name)?;
        }
        writeln!(file)?;
        writeln!(
            file,
            "  注意: 这些 trait 可能不会在约束中显式出现,但类型可能自动满足它们\n"
        )?;

        writeln!(
            file,
            "================================================================================\n"
        )?;

        Ok(())
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

    /// 为 Struct/Enum/Union/Variant 类型创建 Place
    fn create_type_place(&mut self, item: &Item) {
        let place_kind = match &item.inner {
            ItemEnum::Struct(s) => PlaceKind::Struct(s.clone()),
            ItemEnum::Enum(e) => PlaceKind::Enum(e.clone()),
            ItemEnum::Union(u) => PlaceKind::Union(u.clone()),
            ItemEnum::Trait(t) => PlaceKind::Trait(t.clone()),
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

        self.create_generic_param_places(item.id, &item.inner, place_id);

        // 如果是 Trait,还要处理关联类型
        if let ItemEnum::Trait(_) = &item.inner {
            self.create_associated_type_places(item.id, &item.inner, place_id);
        }
    }

    /// 为有 impl 的类型创建 Place(如果类型支持的话)
    ///
    /// 这个方法用于自动发现并创建那些有 impl 块但还没有创建 Place 的类型
    /// 主要用于处理标准库类型(如 String、Vec 等)和类型别名
    ///
    /// 返回是否成功创建了 Place
    fn create_type_place_for_impl(&mut self, item: &Item, impl_block: &Impl) -> bool {
        // 如果已经存在,不重复创建
        if self.type_place_map.contains_key(&item.id) {
            return false;
        }

        let place_kind = match &item.inner {
            ItemEnum::Struct(s) => PlaceKind::Struct(s.clone()),
            ItemEnum::Enum(e) => PlaceKind::Enum(e.clone()),
            ItemEnum::Union(u) => PlaceKind::Union(u.clone()),
            ItemEnum::Variant(v) => PlaceKind::Variant(v.clone()),
            // 对于 Primitive 和 TypeAlias,我们也可以创建 Place
            ItemEnum::Primitive(_) => {
                let name = item
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("primitive_{:?}", item.id));
                let place = Place::new(
                    item.id,
                    name.clone(),
                    format!("primitive::{}", name),
                    PlaceKind::Primitive(name),
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_place_map.insert(item.id, place_id);
                return true;
            }
            // 其他类型暂不支持自动创建
            _ => {
                return false;
            }
        };

        let name = item
            .name
            .clone()
            .unwrap_or_else(|| format!("anonymous_{:?}", item.id));
        let path = item.name.clone().unwrap_or_default();

        let place = Place::new(item.id, name, path, place_kind);
        let place_id = self.net.add_place_and_get_id(place);

        self.type_place_map.insert(item.id, place_id);

        // 为类型的泛型参数创建占位符 Place
        // 使用 impl 块的泛型信息
        self.create_generic_param_places_from_impl(item.id, impl_block, place_id);

        true
    }

    /// 从 impl 块的泛型信息创建泛型参数占位符
    fn create_generic_param_places_from_impl(
        &mut self,
        _type_id: Id,
        impl_block: &Impl,
        _type_place_id: PlaceId,
    ) {
        // 为 impl 块的泛型参数创建 Place
        // 使用与 create_generic_param_places 相同的逻辑，基于约束创建
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
                constraint_trait_ids.sort();
                
                // 使用 (generic_name, constraint_trait_ids) 作为 key
                let cache_key = (generic_name.clone(), constraint_trait_ids.clone());

                // 如果已存在则重用，否则创建（在 create_generic_param_places 中会创建）
                // 这里只是确保 impl 块的泛型参数也被考虑
                if !self.generic_param_cache.contains_key(&cache_key) {
                    // 如果不存在，会在其他地方创建，这里不做处理
                    debug!("Impl 块的泛型参数 '{}' 将在需要时创建", generic_name);
                }
            }
        }
    }

    /// 为类型的泛型参数创建占位符 Place,并建立 holds 关系
    fn create_generic_param_places(
        &mut self,
        type_id: Id,
        item_inner: &ItemEnum,
        type_place_id: PlaceId,
    ) {
        let generics = match item_inner {
            ItemEnum::Struct(s) => &s.generics,
            ItemEnum::Enum(e) => &e.generics,
            ItemEnum::Union(u) => &u.generics,
            ItemEnum::Trait(t) => &t.generics,
            _ => return,
        };

        for param_def in &generics.params {
            // 只处理类型参数(Type),不处理生命周期(Lifetime)和常量(Const)
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
                let generic_place_id = if let Some(&place_id) = self.generic_param_cache.get(&cache_key) {
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
                        format!("generic_param::{}::{:?}", generic_name, constraint_trait_ids),
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
                                "✨ 泛型参数 '{}' 约束于 trait {:?}",
                                generic_name, trait_id
                            );
                        }
                    }

                    place_id
                };

                // 创建 holds 关系:类型 -> 泛型参数
                let dummy_member_id = self.generate_temp_id();
                self.create_holds_transition(
                    type_id,
                    dummy_member_id,
                    type_place_id,
                    generic_place_id,
                );
            }
        }
    }

    /// 为 Trait 的关联类型创建 Place
    fn create_associated_type_places(
        &mut self,
        trait_id: Id,
        item_inner: &ItemEnum,
        trait_place_id: PlaceId,
    ) {
        let trait_def = match item_inner {
            ItemEnum::Trait(t) => t,
            _ => return,
        };

        // 遍历 trait 的所有 items,找到关联类型
        for item_id in &trait_def.items {
            if let Some(assoc_item) = self.crate_.index.get(item_id) {
                if let ItemEnum::AssocType {
                    generics: _,
                    bounds,
                    type_: _,
                } = &assoc_item.inner
                {
                    let assoc_type_name = assoc_item.name.as_deref().unwrap_or("UnnamedAssocType");

                    // 提取 bounds 中的 trait 约束
                    let bound_names: Vec<String> = bounds
                        .iter()
                        .filter_map(|bound| {
                            if let rustdoc_types::GenericBound::TraitBound { trait_, .. } = bound {
                                Some(trait_.path.clone())
                            } else {
                                None
                            }
                        })
                        .collect();

                    // 创建关联类型 Place
                    let assoc_place = Place::new(
                        *item_id,
                        format!("{}::{}", self.get_trait_name(trait_id), assoc_type_name),
                        format!("assoc_type::{}::{}", trait_id.0, assoc_type_name),
                        PlaceKind::AssocType(
                            trait_id,
                            assoc_type_name.to_string(),
                            bound_names.clone(),
                        ),
                    );
                    let assoc_place_id = self.net.add_place_and_get_id(assoc_place);

                    // 将关联类型也存入 type_place_map,以便后续查找
                    self.type_place_map.insert(*item_id, assoc_place_id);

                    // 创建 holds 关系:trait -> 关联类型
                    self.create_holds_transition(
                        trait_id,
                        *item_id,
                        trait_place_id,
                        assoc_place_id,
                    );

                    // 如果有 bound 约束,创建 AliasType 变迁连接到约束的 trait
                    for bound_name in &bound_names {
                        // 尝试找到约束的 trait Place
                        if let Some(bound_trait_id) = self.find_trait_by_name(bound_name) {
                            if let Some(&bound_trait_place_id) =
                                self.type_place_map.get(&bound_trait_id)
                            {
                                self.create_alias_type_transition(
                                    *item_id,
                                    bound_trait_id,
                                    assoc_place_id,
                                    bound_trait_place_id,
                                );
                            }
                        }
                    }

                    debug!(
                        "✨ 创建关联类型 '{}::{}' (ID: {:?}),约束: {:?}",
                        self.get_trait_name(trait_id),
                        assoc_type_name,
                        item_id,
                        bound_names
                    );
                }
            }
        }
    }

    /// 获取 Trait 的名称
    fn get_trait_name(&self, trait_id: Id) -> String {
        self.crate_
            .index
            .get(&trait_id)
            .and_then(|item| item.name.clone())
            .unwrap_or_else(|| format!("Trait_{:?}", trait_id))
    }

    /// 根据名称查找 Trait ID
    fn find_trait_by_name(&self, name: &str) -> Option<Id> {
        // 简单的名称匹配(可能需要改进以处理完整路径)
        for (id, item) in &self.crate_.index {
            if let ItemEnum::Trait(_) = &item.inner {
                if let Some(item_name) = &item.name {
                    if item_name == name || name.ends_with(&format!("::{}", item_name)) {
                        return Some(*id);
                    }
                }
            }
        }
        None
    }

    /// 构建类型成员之间的 holds 关系
    /// 1. 处理 Struct 的字段
    /// 2. 处理 Enum 的变体
    /// 3. 处理 Union 的字段
    /// 4. 处理 Variant 的字段(Tuple 和 Struct)
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
                        // Enum 到 Variant 的关系特殊处理
                        // Variant 已经在 Step 1 创建了 Place,直接建立 holds 关系
                        for variant_id in &enum_def.variants {
                            if let Some(&variant_place_id) = self.type_place_map.get(variant_id) {
                                if let Some(&enum_place_id) = self.type_place_map.get(type_id) {
                                    self.create_holds_transition(
                                        *type_id,
                                        *variant_id,
                                        enum_place_id,
                                        variant_place_id,
                                    );
                                }
                            }
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
            // 为字段类型创建 Place(如果需要)
            // 传入 owner_id 作为上下文,用于解析泛型参数
            let member_place_id = self.create_or_get_type_place(&field_type, &member_id, owner_id);

            // 创建 holds transition 连接 owner 和 member
            if let (Some(owner_place_id), Some(member_place_id)) =
                (self.type_place_map.get(&owner_id), member_place_id)
            {
                self.create_holds_transition(owner_id, member_id, *owner_place_id, member_place_id);
            }
        }
    }

    /// 为类型创建或获取 Place,避免重复创建
    /// 返回 PlaceId,如果类型无需创建 Place 则返回 None
    ///
    /// # 参数
    /// - `ty`: 要创建 Place 的类型
    /// - `field_id`: 字段的 ID(用于生成唯一 ID)
    /// - `owner_type_id`: 拥有此字段的类型的 ID(用于解析泛型参数)
    fn create_or_get_type_place(
        &mut self,
        ty: &Type,
        field_id: &Id,
        owner_type_id: Id,
    ) -> Option<PlaceId> {
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
                        self.create_or_get_type_place(inner_type, &dummy_id, owner_type_id)
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
                let inner_place_id =
                    self.create_or_get_type_place(inner_type, &dummy_id, owner_type_id);

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
                let inner_place_id = self.create_or_get_type_place(type_, &dummy_id, owner_type_id);

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
                let inner_place_id = self.create_or_get_type_place(type_, &dummy_id, owner_type_id);

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
                let inner_place_id = self.create_or_get_type_place(type_, &dummy_id, owner_type_id);

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
                // 检查是否是 Result<T, E> 或 Option<T>
                // 支持: "Result", "io::Result", "core::result::Result" 等
                if (path.path == "Result" || path.path.ends_with("::Result")) && path.args.is_some()
                {
                    return self.handle_result_type(path, field_id, owner_type_id);
                } else if (path.path == "Option" || path.path.ends_with("::Option"))
                    && path.args.is_some()
                {
                    return self.handle_option_type(path, field_id, owner_type_id);
                }

                // 如果是已知的类型,直接返回其 PlaceId
                if let Some(&place_id) = self.type_place_map.get(&path.id) {
                    return Some(place_id);
                }

                // 检查是否是标准库类型,如果是则自动创建
                if self.is_std_library_type(&path.path) {
                    return self.handle_std_library_type(path, field_id, owner_type_id);
                }

                None
            }
            Type::Generic(generic_name) => {
                // 查找对应的泛型参数占位符
                // 根据约束查找对应的库所
                let constraint_trait_ids = self.find_generic_constraint_trait_ids(owner_type_id, generic_name);
                let cache_key = (generic_name.clone(), constraint_trait_ids);
                self.generic_param_cache.get(&cache_key).copied()
            }
            Type::QualifiedPath {
                name,
                self_type,
                trait_,
                ..
            } => {
                info!("处理 QualifiedPath: {:?}", ty);
                // 按照 Scheme B 规则处理 QualifiedPath:转换为 Projection-Type Place
                self.handle_qualified_path(name, self_type, trait_, field_id, owner_type_id)
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

        // 添加边:owner -> transition -> member
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

    /// 创建 impls transition 连接实现类型和 trait
    fn create_impls_transition(
        &mut self,
        impl_type_id: Id,
        trait_id: Id,
        impl_place_id: PlaceId,
        trait_place_id: PlaceId,
    ) {
        let transition_id_val = self.generate_temp_id();
        let transition = Transition::new(
            transition_id_val,
            format!("impls"),
            TransitionKind::Impls(impl_type_id, trait_id),
        );
        let transition_id = self.net.add_transition_and_get_id(transition);

        // 添加边:impl_type -> transition -> trait
        self.net.add_flow(
            impl_place_id,
            transition_id,
            Flow {
                weight: 1,
                param_type: "impl_type".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );
        self.net.add_flow_from_transition(
            transition_id,
            trait_place_id,
            Flow {
                weight: 1,
                param_type: "trait".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );

        debug!(
            "✨ 创建 impls 关系: 类型 {:?} 实现 trait {:?}",
            impl_type_id, trait_id
        );
    }

    /// 处理 QualifiedPath,将其转换为 Projection-Type Place
    /// 
    /// 根据规则文档,每个 QualifiedPath 必须转换为一个 Projection-Type Place,
    /// 使用规范化的身份标识:Projection(self=<SelfTypeCanonicalID>, trait=<TraitCanonicalID>, assoc=<AssocName>)
    fn handle_qualified_path(
        &mut self,
        assoc_name: &str,
        self_type: &Type,
        trait_: &Option<RustdocPath>,
        field_id: &Id,
        owner_type_id: Id,
    ) -> Option<PlaceId> {
        // 1. 解析 self_type 到 canonical ID
        let self_type_id = match self_type {
            Type::ResolvedPath(path) => {
                // 如果是 ResolvedPath,使用 path.id 作为 canonical ID
                path.id
            }
            Type::Generic(gen_name) => {
                // 如果是 Generic("Self"),需要从上下文解析
                if gen_name == "Self" {
                    // 在 impl 块中,Self 就是 receiver_id
                    // 但这里我们可能没有 receiver_id,需要从 owner_type_id 推断
                    // 如果 owner_type_id 是一个类型,就使用它
                    owner_type_id
                } else {
                    // 其他泛型参数,查找对应的泛型参数 Place
                    let constraint_trait_ids = self.find_generic_constraint_trait_ids(owner_type_id, gen_name);
                    let cache_key = (gen_name.clone(), constraint_trait_ids);
                    if self.generic_param_cache.contains_key(&cache_key) {
                        // 对于泛型参数,我们需要一个唯一的 ID 来表示它
                        // 我们可以使用泛型参数 Place 的 ID,但这不够准确
                        // 更好的方法是使用一个规范化的 ID 生成策略
                        // 这里我们使用 owner_type_id 和泛型名的组合作为代理 ID
                        // 但为了 Projection 的规范化,我们需要一个稳定的标识符
                        // 暂时使用 owner_type_id 作为 self_type_id,但这不是最优解
                        // TODO: 改进泛型参数的规范化 ID 生成策略
                        owner_type_id
                    } else {
                        // 泛型参数未找到,尝试在函数级别查找
                        // 这可能发生在函数泛型参数中
                        // 我们需要一个更好的上下文传递机制
                        warn!(
                            "QualifiedPath 中的 self_type 是未找到的泛型参数 {} (owner_type_id: {:?}),尝试使用 owner_type_id 作为代理",
                            gen_name, owner_type_id
                        );
                        // 使用 owner_type_id 作为代理,虽然不够准确,但至少可以创建 Projection
                        owner_type_id
                    }
                }
            }
            _ => {
                warn!("QualifiedPath 中的 self_type 类型不支持: {:?}", self_type);
                return None;
            }
        };

        // 2. 解析 trait 到 canonical ID
        let trait_id = match trait_ {
            Some(trait_path) => trait_path.id,
            None => {
                warn!("QualifiedPath 缺少 trait 信息");
                return None;
            }
        };

        // 3. 构建规范化的 key: (self_type_id, trait_id, assoc_name)
        let projection_key = (self_type_id, trait_id, assoc_name.to_string());

        // 4. 检查是否已存在,如果存在则重用
        if let Some(&projection_place_id) = self.projection_cache.get(&projection_key) {
            return Some(projection_place_id);
        }

        // 5. 获取 self_type 的 Place(如果不存在,尝试创建)
        let self_place_id = if let Some(&place_id) = self.type_place_map.get(&self_type_id) {
            place_id
        } else {
            // 如果 self_type 是泛型参数,尝试查找对应的泛型参数 Place
            if let Type::Generic(gen_name) = self_type {
                let constraint_trait_ids = self.find_generic_constraint_trait_ids(owner_type_id, gen_name);
                let cache_key = (gen_name.clone(), constraint_trait_ids);
                if let Some(&generic_place_id) = self.generic_param_cache.get(&cache_key) {
                    // 使用泛型参数 Place 作为 self_place_id
                    generic_place_id
                } else {
                    // 泛型参数 Place 不存在,尝试创建
                    if let Some(place_id) = self.create_or_get_type_place(self_type, field_id, owner_type_id) {
                        place_id
                    } else {
                        // 如果还是无法创建,尝试使用 owner_type_id 的类型 Place 作为后备
                        if let Some(&owner_place_id) = self.type_place_map.get(&owner_type_id) {
                            warn!(
                                "无法为 QualifiedPath 的 self_type (泛型参数 '{}') 创建 Place,使用 owner_type_id {:?} 的类型 Place 作为后备",
                                gen_name, owner_type_id
                            );
                            owner_place_id
                        } else {
                            warn!("无法为 QualifiedPath 的 self_type 创建 Place: {:?}", self_type);
                            return None;
                        }
                    }
                }
            } else {
                // 尝试解析并创建 self_type 的 Place
                if let Some(place_id) = self.create_or_get_type_place(self_type, field_id, owner_type_id) {
                    place_id
                } else {
                    warn!("无法为 QualifiedPath 的 self_type 创建 Place: {:?}", self_type);
                    return None;
                }
            }
        };

        // 6. 创建 Projection Place
        let projection_id = self.generate_temp_id();
        let projection_name = format!("<{} as {}>::{}", 
            self.format_type(self_type),
            self.get_type_name(&trait_id).unwrap_or_else(|| format!("{:?}", trait_id)),
            assoc_name
        );
        let projection_path = format!("projection::self={:?}::trait={:?}::assoc={}", 
            self_type_id, trait_id, assoc_name);
        
        let projection_place = Place::new(
            projection_id,
            projection_name.clone(),
            projection_path,
            PlaceKind::Projection(self_type_id, trait_id, assoc_name.to_string()),
        );
        let projection_place_id = self.net.add_place_and_get_id(projection_place);

        // 7. 缓存 Projection Place
        self.projection_cache.insert(projection_key, projection_place_id);

        // 8. 创建 Projection transition: Place(SelfType) --Projection--> Place(Projection)
        let transition_id_val = self.generate_temp_id();
        let transition = Transition::new(
            transition_id_val,
            "projection".to_string(),
            TransitionKind::Projection(self_type_id, trait_id, assoc_name.to_string()),
        );
        let transition_id = self.net.add_transition_and_get_id(transition);

        // 添加边:self_type -> transition -> projection
        self.net.add_flow(
            self_place_id,
            transition_id,
            Flow {
                weight: 1,
                param_type: "self_type".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );
        self.net.add_flow_from_transition(
            transition_id,
            projection_place_id,
            Flow {
                weight: 1,
                param_type: "projection".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );

        debug!(
            "✨ 创建 Projection: <{:?} as {:?}>::{}",
            self_type_id, trait_id, assoc_name
        );

        Some(projection_place_id)
    }

    /// 获取类型的名称(用于显示)
    fn get_type_name(&self, type_id: &Id) -> Option<String> {
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
                let generic_place_id = if let Some(&place_id) = self.generic_param_cache.get(&cache_key) {
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
                        format!("generic_param::{}::{:?}", generic_name, constraint_trait_ids),
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
                let trait_path_str = Arc::<str>::from(Self::format_path(trait_path));
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

    /// 创建 AliasType transition 连接关联类型和它的约束
    fn create_alias_type_transition(
        &mut self,
        assoc_type_id: Id,
        target_trait_id: Id,
        assoc_place_id: PlaceId,
        target_place_id: PlaceId,
    ) {
        let transition_id_val = self.generate_temp_id();
        let transition = Transition::new(
            transition_id_val,
            format!("alias_type"),
            TransitionKind::AliasType(assoc_type_id, target_trait_id),
        );
        let transition_id = self.net.add_transition_and_get_id(transition);

        // 添加边:assoc_type -> transition -> target_trait
        self.net.add_flow(
            assoc_place_id,
            transition_id,
            Flow {
                weight: 1,
                param_type: "assoc_type".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );
        self.net.add_flow_from_transition(
            transition_id,
            target_place_id,
            Flow {
                weight: 1,
                param_type: "bound".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );

        debug!(
            "✨ 创建 alias_type 关系: 关联类型 {:?} 约束于 trait {:?}",
            assoc_type_id, target_trait_id
        );
    }

    /// 处理 Result<T, E> 类型
    /// 为 Result 创建 Place,提取 T 和 E,并创建 unwrap 变迁连接到 T 和 E
    fn handle_result_type(
        &mut self,
        path: &RustdocPath,
        field_id: &Id,
        owner_type_id: Id,
    ) -> Option<PlaceId> {
        use rustdoc_types::GenericArgs;

        // 提取泛型参数 T 和 E
        let args = path.args.as_ref()?;
        let generic_args = match args.as_ref() {
            GenericArgs::AngleBracketed { args, .. } => args,
            _ => return None,
        };

        // 处理 Result 类型的泛型参数
        // 标准 Result<T, E> 有 2 个参数,但类型别名可能只有 1 个(错误类型是固定的)
        let (ok_type, err_type) = if generic_args.len() >= 2 {
            // 标准 Result<T, E>
            let ok = match &generic_args[0] {
                rustdoc_types::GenericArg::Type(ty) => ty,
                _ => return None,
            };
            let err = match &generic_args[1] {
                rustdoc_types::GenericArg::Type(ty) => ty,
                _ => return None,
            };
            (ok, err)
        } else if generic_args.len() == 1 {
            // 类型别名 Result<T>(错误类型固定)
            // 例如 io::Result<T> = Result<T, io::Error>
            let ok = match &generic_args[0] {
                rustdoc_types::GenericArg::Type(ty) => ty,
                _ => return None,
            };
            // 为错误类型创建一个占位符(使用 Infer 类型)
            let err = &Type::Infer;
            (ok, err)
        } else {
            warn!(
                "Result 类型应该有 1-2 个泛型参数,但找到 {} 个",
                generic_args.len()
            );
            return None;
        };

        // 生成唯一的类型键
        let type_key = format!(
            "result:{}<{}, {}>",
            self.format_type(&Type::ResolvedPath(path.clone())),
            self.format_type(ok_type),
            self.format_type(err_type)
        );

        // 检查是否已创建
        if let Some(&place_id) = self.type_cache.get(&type_key) {
            return Some(place_id);
        }

        // 为 T 和 E 创建 Place
        let ok_id = self.generate_temp_id();
        let err_id = self.generate_temp_id();
        let ok_place_id = self.create_or_get_type_place(ok_type, &ok_id, owner_type_id);
        let err_place_id = self.create_or_get_type_place(err_type, &err_id, owner_type_id);

        // 创建 Result<T, E> 的 Place
        let result_place = Place::new(
            *field_id,
            format!(
                "Result<{}, {}>",
                self.format_type(ok_type),
                self.format_type(err_type)
            ),
            format!(
                "result::Result<{}, {}>",
                self.format_type(ok_type),
                self.format_type(err_type)
            ),
            PlaceKind::Result(Box::new(ok_type.clone()), Box::new(err_type.clone())),
        );
        let result_place_id = self.net.add_place_and_get_id(result_place);
        self.type_cache.insert(type_key, result_place_id);

        // 创建 unwrap 变迁: Result<T, E> -> T 和 E
        let unwrap_transition_id = self.generate_temp_id();
        let unwrap_transition = Transition::new(
            unwrap_transition_id,
            format!("unwrap_result"),
            TransitionKind::Unwrap,
        );
        let unwrap_transition_id = self.net.add_transition_and_get_id(unwrap_transition);

        // Result -> unwrap
        self.net.add_flow(
            result_place_id,
            unwrap_transition_id,
            Flow {
                weight: 1,
                param_type: "result".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );

        // unwrap -> T (Ok)
        if let Some(ok_place_id) = ok_place_id {
            self.net.add_flow_from_transition(
                unwrap_transition_id,
                ok_place_id,
                Flow {
                    weight: 1,
                    param_type: "ok".to_string(),
                    borrow_kind: BorrowKind::Owned,
                },
            );
        } else {
            warn!(
                "Result 类型的 Ok 类型 '{}' 无法创建 Place,unwrap 变迁无法连接到 Ok 类型",
                self.format_type(ok_type)
            );
        }

        // unwrap -> E (Err)
        if let Some(err_place_id) = err_place_id {
            self.net.add_flow_from_transition(
                unwrap_transition_id,
                err_place_id,
                Flow {
                    weight: 1,
                    param_type: "err".to_string(),
                    borrow_kind: BorrowKind::Owned,
                },
            );
        } else {
            warn!(
                "Result 类型的 Err 类型 '{}' 无法创建 Place,unwrap 变迁无法连接到 Err 类型",
                self.format_type(err_type)
            );
        }

        debug!(
            "✨ 创建 Result<{}, {}> 及其 unwrap 变迁",
            self.format_type(ok_type),
            self.format_type(err_type)
        );

        Some(result_place_id)
    }

    /// 处理标准库类型
    /// 为标准库类型(String, Vec, bool 等)自动创建 Place
    fn handle_std_library_type(
        &mut self,
        path: &RustdocPath,
        field_id: &Id,
        owner_type_id: Id,
    ) -> Option<PlaceId> {
        // 生成类型键(使用完整路径以区分 String 和 alloc::string::String)
        let type_key = format!("std_lib:{}", path.path);

        // 检查是否已创建
        if let Some(&place_id) = self.type_cache.get(&type_key) {
            return Some(place_id);
        }

        // 处理泛型参数(如果有)
        let mut generic_info = Vec::new();
        if let Some(args) = &path.args {
            use rustdoc_types::GenericArgs;
            if let GenericArgs::AngleBracketed { args, .. } = args.as_ref() {
                for arg in args {
                    if let rustdoc_types::GenericArg::Type(ty) = arg {
                        let dummy_id = self.generate_temp_id();
                        if let Some(inner_place_id) =
                            self.create_or_get_type_place(ty, &dummy_id, owner_type_id)
                        {
                            generic_info.push((dummy_id, inner_place_id));
                        }
                    }
                }
            }
        }

        // 创建标准库类型的 Place(使用 Primitive 或 Struct)
        // 对于简单类型用 Primitive,对于复杂类型可以扩展
        let display_name = if generic_info.is_empty() {
            format!("{}", path.path)
        } else {
            format!("{}<...>", path.path)
        };

        let place = Place::new(
            *field_id,
            display_name.clone(),
            format!("std_lib::{}", path.path),
            PlaceKind::Primitive(path.path.clone()), // 使用 Primitive 存储标准库类型
        );
        let place_id = self.net.add_place_and_get_id(place);
        self.type_cache.insert(type_key.clone(), place_id);

        // 同时存入 type_place_map,使用 rustdoc 的 ID
        // 这样 impl 块和其他地方可以通过 rustdoc ID 找到这个 Place
        self.type_place_map.insert(path.id, place_id);

        // 如果有泛型参数,创建 holds 关系
        for (dummy_id, inner_place_id) in generic_info {
            self.create_holds_transition(*field_id, dummy_id, place_id, inner_place_id);
        }

        debug!("✨ 自动创建标准库类型 '{}' Place", path.path);

        Some(place_id)
    }

    /// 处理 Option<T> 类型
    /// 为 Option 创建 Place,提取 T,并创建 ok 变迁连接到 T
    fn handle_option_type(
        &mut self,
        path: &RustdocPath,
        field_id: &Id,
        owner_type_id: Id,
    ) -> Option<PlaceId> {
        use rustdoc_types::GenericArgs;

        // 提取泛型参数 T
        let args = path.args.as_ref()?;
        let generic_args = match args.as_ref() {
            GenericArgs::AngleBracketed { args, .. } => args,
            _ => return None,
        };

        if generic_args.is_empty() {
            warn!("Option 类型应该有 1 个泛型参数,但没有找到");
            return None;
        }

        // 获取 T 的类型
        let some_type = match &generic_args[0] {
            rustdoc_types::GenericArg::Type(ty) => ty,
            _ => return None,
        };

        // 生成唯一的类型键
        let type_key = format!(
            "option:{}<{}>",
            self.format_type(&Type::ResolvedPath(path.clone())),
            self.format_type(some_type)
        );

        // 检查是否已创建
        if let Some(&place_id) = self.type_cache.get(&type_key) {
            return Some(place_id);
        }

        // 为 T 创建 Place
        let some_id = self.generate_temp_id();
        let some_place_id = self.create_or_get_type_place(some_type, &some_id, owner_type_id);

        // 创建 Option<T> 的 Place
        let option_place = Place::new(
            *field_id,
            format!("Option<{}>", self.format_type(some_type)),
            format!("option::Option<{}>", self.format_type(some_type)),
            PlaceKind::Option(Box::new(some_type.clone())),
        );
        let option_place_id = self.net.add_place_and_get_id(option_place);
        self.type_cache.insert(type_key, option_place_id);

        // 创建 ok/unwrap 变迁: Option<T> -> T
        let ok_transition_id = self.generate_temp_id();
        let ok_transition = Transition::new(
            ok_transition_id,
            format!("unwrap_option"),
            TransitionKind::Ok,
        );
        let ok_transition_id = self.net.add_transition_and_get_id(ok_transition);

        // Option -> ok
        self.net.add_flow(
            option_place_id,
            ok_transition_id,
            Flow {
                weight: 1,
                param_type: "option".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );

        // ok -> T (Some)
        if let Some(some_place_id) = some_place_id {
            self.net.add_flow_from_transition(
                ok_transition_id,
                some_place_id,
                Flow {
                    weight: 1,
                    param_type: "some".to_string(),
                    borrow_kind: BorrowKind::Owned,
                },
            );
        }

        debug!(
            "✨ 创建 Option<{}> 及其 ok 变迁",
            self.format_type(some_type)
        );

        Some(option_place_id)
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

    /// 判断是否应该跳过某个函数,不为其创建变迁
    ///
    /// 跳过的函数包括:type_id, fmt, fmt_debug, fmt_display 等
    fn should_skip_function(&self, func_name: &str) -> bool {
        matches!(func_name, "type_id" | "fmt" | "fmt_debug" | "fmt_display")
    }

    /// 检查是否是标准库 trait 实现
    ///
    /// 这些 trait 的泛型参数需要特殊处理:
    /// - Borrow<T>: T 就是 Self
    /// - BorrowMut<T>: T 就是 Self
    /// - AsRef<T>: T 是目标类型
    /// - AsMut<T>: T 是目标类型
    /// - From<T>: T 是输入类型,Self 是输出
    /// - Into<U>: U 是目标类型,Self 是输入
    /// - TryFrom<T>: T 是输入类型
    /// - TryInto<U>: U 是目标类型
    /// - ToOwned: 返回类型通常是 Self::Owned
    fn is_standard_trait(&self, trait_path: &str, _func_name: &str) -> bool {
        // 检查常见的标准 trait
        matches!(
            trait_path,
            "Borrow"
                | "BorrowMut"
                | "AsRef"
                | "AsMut"
                | "From"
                | "Into"
                | "TryFrom"
                | "TryInto"
                | "ToOwned"
                | "Clone"
                | "Default"
                | "Any"
        ) || trait_path.ends_with("::Borrow")
            || trait_path.ends_with("::BorrowMut")
            || trait_path.ends_with("::AsRef")
            || trait_path.ends_with("::AsMut")
            || trait_path.ends_with("::From")
            || trait_path.ends_with("::Into")
            || trait_path.ends_with("::TryFrom")
            || trait_path.ends_with("::TryInto")
            || trait_path.ends_with("::ToOwned")
            || trait_path.ends_with("::Clone")
            || trait_path.ends_with("::Default")
            || trait_path.ends_with("::Any")
    }

    /// 解析标准 trait 的泛型类型到实际类型
    ///
    /// # 参数
    /// - `ty`: 原始类型(可能包含泛型参数)
    /// - `func_name`: 函数名
    /// - `receiver_id`: 接收者类型 ID
    /// - `trait_path`: Trait 路径
    ///
    /// # 返回值
    /// 返回解析后的类型(将泛型参数映射到实际类型)
    fn resolve_std_trait_type<'t>(
        &self,
        ty: &'t Type,
        func_name: &str,
        _receiver_id: Option<Id>,
        trait_path: Option<&str>,
    ) -> &'t Type {
        // 如果不是泛型类型,直接返回
        let Type::Generic(generic_name) = ty else {
            return ty;
        };

        // 如果是 Self,不需要特殊处理
        if generic_name == "Self" {
            return ty;
        }

        let Some(trait_name) = trait_path else {
            return ty;
        };

        // 根据 trait 和方法名决定如何映射泛型
        let should_map_to_self = match (trait_name, func_name) {
            // Borrow<T> 的 borrow() 返回 &T,T 就是 Self
            (t, "borrow") if t.contains("Borrow") && !t.contains("BorrowMut") => {
                generic_name == "T"
            }
            // BorrowMut<T> 的 borrow_mut() 返回 &mut T,T 就是 Self
            (t, "borrow_mut") if t.contains("BorrowMut") => generic_name == "T",
            // AsRef<T> 的 as_ref() 返回 &T
            (t, "as_ref") if t.contains("AsRef") => generic_name == "T",
            // AsMut<T> 的 as_mut() 返回 &mut T
            (t, "as_mut") if t.contains("AsMut") => generic_name == "T",
            // ToOwned 的 to_owned() 返回 Self::Owned,但 T 在 clone_into 中是 Self
            (t, "to_owned" | "clone_into") if t.contains("ToOwned") => generic_name == "T",
            // Clone 的 clone() 和 clone_from() 的 T 就是 Self
            (t, "clone" | "clone_from") if t.contains("Clone") => generic_name == "T",
            // Any 的 type_id(),返回 TypeId(不需要映射)
            (t, "type_id") if t.contains("Any") => false,
            _ => false,
        };

        if should_map_to_self {
            // 对于这些情况,泛型参数 T 实际上就是 Self
            // 但我们不能修改 ty,所以返回原类型
            // 在 find_type_place_in_function 中会特殊处理
            return ty;
        }

        // From<T> 和 Into<U> 的泛型参数不是 Self,保持原样
        // 它们会在查找时失败,这是预期行为
        ty
    }

    /// 检查类型是否包含指定的泛型参数
    fn type_has_generic(&self, item_inner: &ItemEnum, generic_name: &str) -> bool {
        let generics = match item_inner {
            ItemEnum::Struct(s) => &s.generics,
            ItemEnum::Enum(e) => &e.generics,
            ItemEnum::Union(u) => &u.generics,
            _ => return false,
        };

        generics.params.iter().any(|param| {
            param.name == generic_name && matches!(param.kind, GenericParamDefKind::Type { .. })
        })
    }

    /// 生成临时 ID
    fn generate_temp_id(&mut self) -> Id {
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
            .map(|path| self.is_standard_trait(path, func_name))
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
            let (borrow_kind, actual_type) = self.extract_borrow_info(param_type);

            // 特殊处理标准 trait 的类型映射
            let resolved_type = if is_std_trait {
                self.resolve_std_trait_type(actual_type, func_name, receiver_id, trait_path)
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
                let type_str = self.format_type(actual_type);
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
            let (borrow_kind, actual_type) = self.extract_borrow_info(return_type);

            // 特殊处理标准 trait 的类型映射
            let resolved_type = if is_std_trait {
                self.resolve_std_trait_type(actual_type, func_name, receiver_id, trait_path)
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
                let type_str = self.format_type(actual_type);
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

    /// 提取类型的借用信息,返回 (借用类型, 实际类型)
    ///
    /// 例如:
    /// - `&T` -> (Borrowed, T)
    /// - `&mut T` -> (BorrowedMut, T)
    /// - `*const T` -> (Borrowed, T) - 原始指针视为借用(不可变)
    /// - `*mut T` -> (BorrowedMut, T) - 可变原始指针
    /// - `T` -> (Owned, T)
    fn extract_borrow_info<'t>(&self, ty: &'t Type) -> (BorrowKind, &'t Type) {
        match ty {
            Type::BorrowedRef {
                is_mutable, type_, ..
            } => {
                if *is_mutable {
                    (BorrowKind::BorrowedMut, type_.as_ref())
                } else {
                    (BorrowKind::Borrowed, type_.as_ref())
                }
            }
            Type::RawPointer { is_mutable, type_ } => {
                // 原始指针也表示一种借用关系
                if *is_mutable {
                    (BorrowKind::BorrowedMut, type_.as_ref())
                } else {
                    (BorrowKind::Borrowed, type_.as_ref())
                }
            }
            _ => (BorrowKind::Owned, ty),
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
                let generic_place_id = if let Some(&place_id) = self.generic_param_cache.get(&cache_key) {
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
                        format!("generic_param::{}::{:?}", generic_name, constraint_trait_ids),
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

    /// 在函数上下文中查找类型对应的 Place
    ///
    /// 与 find_type_place 的区别:
    /// - 支持 Self 类型解析(通过 receiver_id)
    /// - 优先查找函数级别的泛型参数,然后查找类型级别的泛型参数
    ///
    /// # 参数
    /// - `ty`: 要查找的类型
    /// - `function_id`: 函数的 ID(用于查找函数级别的泛型)
    /// - `receiver_id`: impl 块的接收者 ID(用于解析 Self 和类型级别的泛型)
    fn find_type_place_in_function(
        &mut self,
        ty: &Type,
        function_id: Id,
        receiver_id: Option<Id>,
    ) -> Option<PlaceId> {
        match ty {
            Type::Generic(generic_name) => {
                // 特殊处理 Self
                if generic_name == "Self" {
                    if let Some(receiver_id) = receiver_id {
                        if let Some(&place_id) = self.type_place_map.get(&receiver_id) {
                            return Some(place_id);
                        } else {
                            warn!(
                                "❌ 错误:Self 类型在函数 {:?} 中使用,但接收者类型 {:?} 的 Place 不存在",
                                function_id, receiver_id
                            );
                            return None;
                        }
                    }
                    // Self 但没有 receiver_id,这是无约束函数
                    if let Some(item) = self.crate_.index.get(&function_id) {
                        let func_name = item.name.as_deref().unwrap_or("(匿名)");
                        warn!(
                            "⚠️  无约束函数 '{}' (ID: {:?}) 的返回值包含 Self 类型,但无法确定 Self 的具体类型(函数不在 impl 块中)",
                            func_name, function_id
                        );
                    } else {
                        warn!(
                            "⚠️  无约束函数 (ID: {:?}) 的返回值包含 Self 类型,但无法确定 Self 的具体类型(函数不在 impl 块中)",
                            function_id
                        );
                    }
                    return None;
                }

                // 特殊处理:标准 trait 的泛型参数 T/U 映射到 Self
                // 例如 Borrow<T> 的 T, BorrowMut<T> 的 T, ToOwned 的 T
                if (generic_name == "T" || generic_name == "U") && receiver_id.is_some() {
                    let receiver_id_val = receiver_id.unwrap();
                    // 先检查是否真的有这个泛型参数
                    let func_constraints = self.find_generic_constraint_trait_ids(function_id, generic_name);
                    let type_constraints = self.find_generic_constraint_trait_ids(receiver_id_val, generic_name);

                    let func_cache_key = (generic_name.clone(), func_constraints);
                    let type_cache_key = (generic_name.clone(), type_constraints);

                    let has_func_generic = self.generic_param_cache.contains_key(&func_cache_key);
                    let has_type_generic = self.generic_param_cache.contains_key(&type_cache_key);

                    // 如果既没有函数级泛型,也没有类型级泛型,可能是标准 trait
                    if !has_func_generic && !has_type_generic {
                        // 尝试映射到 Self
                        if let Some(&place_id) = self.type_place_map.get(&receiver_id_val) {
                            return Some(place_id);
                        }
                    }
                }

                // 先查找函数级别的泛型参数
                let func_constraints = self.find_generic_constraint_trait_ids(function_id, generic_name);
                let func_cache_key = (generic_name.clone(), func_constraints);
                if let Some(&place_id) = self.generic_param_cache.get(&func_cache_key) {
                    return Some(place_id);
                }

                // 再查找类型级别的泛型参数(如果在 impl 块中)
                if let Some(receiver_id) = receiver_id {
                    let type_constraints = self.find_generic_constraint_trait_ids(receiver_id, generic_name);
                    let type_cache_key = (generic_name.clone(), type_constraints);
                    if let Some(&place_id) = self.generic_param_cache.get(&type_cache_key) {
                        return Some(place_id);
                    }

                    // 如果在 impl 块中但找不到泛型参数,检查类型是否有这个泛型
                    if let Some(type_item) = self.crate_.index.get(&receiver_id) {
                        let has_generic = self.type_has_generic(&type_item.inner, generic_name);
                        if !has_generic {
                            // 不报告错误,可能是标准 trait 的泛型(已经映射到 Self)
                            debug!(
                                "泛型参数 '{}' 在函数 {:?} 中使用,但类型 '{}' (ID: {:?}) 不包含此泛型参数(可能是标准 trait)",
                                generic_name,
                                function_id,
                                type_item.name.as_deref().unwrap_or("?"),
                                receiver_id
                            );
                        } else {
                            // 类型有这个泛型参数,但未创建 Place
                            // 尝试链接到 receiver_id 的类型 Place 作为后备
                            if let Some(&receiver_place_id) = self.type_place_map.get(&receiver_id) {
                                warn!(
                                    "⚠️  泛型参数 '{}' 在函数 {:?} 中使用,类型 '{}' (ID: {:?}) 有此泛型但未创建 Place.将链接到类型 Place 作为后备.",
                                    generic_name,
                                    function_id,
                                    type_item.name.as_deref().unwrap_or("?"),
                                    receiver_id
                                );
                                return Some(receiver_place_id);
                            } else {
                                warn!(
                                    "❌ 错误:泛型参数 '{}' 在函数 {:?} 中使用,类型 '{}' (ID: {:?}) 有此泛型但未创建 Place,且类型 Place 也不存在",
                                    generic_name,
                                    function_id,
                                    type_item.name.as_deref().unwrap_or("?"),
                                    receiver_id
                                );
                            }
                        }
                    }
                } else {
                    // 自由函数中的泛型参数
                    if let Some(item) = self.crate_.index.get(&function_id) {
                        let func_name = item.name.as_deref().unwrap_or("(匿名)");
                        warn!(
                            "⚠️  无约束函数 '{}' (ID: {:?}) 的返回值包含泛型参数 '{}',但无法确定其具体类型(函数不在 impl 块中,且泛型参数未创建 Place)",
                            func_name, function_id, generic_name
                        );
                    } else {
                        warn!(
                            "⚠️  无约束函数 (ID: {:?}) 的返回值包含泛型参数 '{}',但无法确定其具体类型(函数不在 impl 块中,且泛型参数未创建 Place)",
                            function_id, generic_name
                        );
                    }
                }

                None
            }
            _ => {
                // 其他类型:先尝试查找,如果不存在则创建
                // 特别处理 Result 和 Option
                let owner_type_id = receiver_id.unwrap_or(function_id);

                // 先尝试查找
                if let Some(place_id) = self.find_type_place(ty, owner_type_id) {
                    return Some(place_id);
                }

                // 如果不存在,尝试创建(特别是 Result/Option/Primitive 等)
                // 为此类型生成一个临时 ID
                let temp_id = self.generate_temp_id();
                self.create_or_get_type_place(ty, &temp_id, owner_type_id)
            }
        }
    }

    /// 查找类型对应的 Place
    ///
    /// 支持以下类型:
    /// - ResolvedPath: 查找 type_place_map
    /// - Generic: 查找 generic_param_cache
    /// - Primitive, Tuple, Slice, Array 等: 查找 type_cache
    fn find_type_place(&self, ty: &Type, owner_type_id: Id) -> Option<PlaceId> {
        match ty {
            Type::ResolvedPath(path) => {
                // 查找结构体、枚举、联合体等
                self.type_place_map.get(&path.id).copied()
            }
            Type::Generic(generic_name) => {
                // 查找泛型参数占位符
                // 根据约束查找对应的库所
                let constraint_trait_ids = self.find_generic_constraint_trait_ids(owner_type_id, generic_name);
                let cache_key = (generic_name.clone(), constraint_trait_ids);
                self.generic_param_cache.get(&cache_key).copied()
            }
            Type::Primitive(name) => {
                // 查找基本类型
                let type_key = format!("primitive:{}", name);
                self.type_cache.get(&type_key).copied()
            }
            Type::Tuple(_) => {
                // 查找元组类型
                let type_key = format!("tuple:{}", self.format_type(ty));
                self.type_cache.get(&type_key).copied()
            }
            Type::Slice(_) => {
                // 查找切片类型
                let type_key = format!("slice:{}", self.format_type(ty));
                self.type_cache.get(&type_key).copied()
            }
            Type::Array { type_, len } => {
                // 查找数组类型
                let type_key = format!("array:{}:{}", self.format_type(type_), len);
                self.type_cache.get(&type_key).copied()
            }
            Type::RawPointer { is_mutable, type_ } => {
                // 查找原始指针类型
                let mutability = if *is_mutable { "mut" } else { "const" };
                let type_key = format!("rawptr:{}:{}", mutability, self.format_type(type_));
                self.type_cache.get(&type_key).copied()
            }
            Type::Infer => {
                // 查找推断类型
                let type_key = "infer:_".to_string();
                self.type_cache.get(&type_key).copied()
            }
            _ => {
                // 其他类型暂不支持
                None
            }
        }
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
