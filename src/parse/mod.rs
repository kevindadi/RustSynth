/// Parse 模块:负责解析 rustdoc JSON 输出并提取关键信息
///
/// 此模块是预处理层，最大程度保留 rustdoc_types 原始数据
/// 便于后续 IR Graph 构建使用
use rustdoc_types::{Crate, Id, Item, ItemEnum};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// 解析后的 Crate 信息（用于 IR Graph 构建）
#[derive(Debug, Clone)]
pub struct ParsedCrate {
    /// 原始的 Crate 数据
    pub crate_data: Crate,
    /// 预处理后的信息
    pub info: ParsedInfo,
}

/// 预处理信息
#[derive(Debug, Clone)]
pub struct ParsedInfo {
    /// StructFiled 和 Variant
    pub struct_fields: HashSet<Id>,
    pub variant_fields: HashSet<Id>,
    /// 类型集合（Struct, Enum, Union, TypeAlias）
    pub types: HashSet<Id>,
    /// Trait 集合
    pub traits: HashSet<Id>,
    /// 顶层函数集合（不在 impl/trait 中的）
    pub functions: HashSet<Id>,
    /// 常量集合
    pub constants: HashSet<Id>,
    /// 静态变量集合
    pub statics: HashSet<Id>,
    /// USE 重导出集合
    pub uses: HashSet<Id>,
    /// Impl 块集合
    pub impls: HashSet<Id>,

    /// Trait 实现映射: 类型 Id -> 实现的 Trait Id 列表
    pub trait_impls: HashMap<Id, Vec<Id>>,
    /// USE 解析缓存: Use Id -> 解析后的目标 Id
    pub use_resolutions: HashMap<Id, Id>,
}

impl ParsedCrate {
    /// 从 JSON 文件加载并解析
    pub fn from_json_file<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let krate: Crate = serde_json::from_reader(reader)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        Ok(Self::from_crate(krate))
    }

    /// 从 Crate 对象解析
    pub fn from_crate(krate: Crate) -> Self {
        let info = Self::extract_info(&krate);

        ParsedCrate {
            crate_data: krate,
            info,
        }
    }

    fn extract_info(krate: &Crate) -> ParsedInfo {
        let mut info = ParsedInfo {
            struct_fields: HashSet::new(),
            variant_fields: HashSet::new(),
            types: HashSet::new(),
            traits: HashSet::new(),
            functions: HashSet::new(),
            constants: HashSet::new(),
            statics: HashSet::new(),
            uses: HashSet::new(),
            impls: HashSet::new(),
            trait_impls: HashMap::new(),
            use_resolutions: HashMap::new(),
        };

        // 收集所有 impl 块和 trait 中的方法 ID
        let mut impl_trait_methods = HashSet::new();
        for item in krate.index.values() {
            match &item.inner {
                ItemEnum::Impl(_) => {
                    impl_trait_methods.extend(Self::get_impl_items(item));
                }
                ItemEnum::Trait(trait_data) => {
                    impl_trait_methods.extend(trait_data.items.iter());
                }
                _ => {}
            }
        }

        // 分类所有 Item
        for (&id, item) in &krate.index {
            match &item.inner {
                ItemEnum::StructField(_) => {
                    info.struct_fields.insert(id);
                }
                ItemEnum::Variant(_) => {
                    info.variant_fields.insert(id);
                }
                ItemEnum::Struct(_)
                | ItemEnum::Enum(_)
                | ItemEnum::Union(_)
                | ItemEnum::TypeAlias(_) => {
                    info.types.insert(id);
                }
                ItemEnum::Trait(_) => {
                    info.traits.insert(id);
                }
                ItemEnum::Function(_) => {
                    // 只收集顶层函数
                    if !impl_trait_methods.contains(&id) {
                        info.functions.insert(id);
                    }
                }
                ItemEnum::Constant { .. } => {
                    info.constants.insert(id);
                }
                ItemEnum::Static(_) => {
                    info.statics.insert(id);
                }
                ItemEnum::Use(_) => {
                    info.uses.insert(id);
                }
                ItemEnum::Impl(_) => {
                    info.impls.insert(id);
                }
                _ => {}
            }
        }

        // 构建 Trait 实现映射
        for (&_impl_id, item) in &krate.index {
            if let ItemEnum::Impl(impl_data) = &item.inner {
                if let Some(trait_ref) = &impl_data.trait_ {
                    if let Some(for_type_id) = Self::extract_type_id(&impl_data.for_) {
                        info.trait_impls
                            .entry(for_type_id)
                            .or_insert_with(Vec::new)
                            .push(trait_ref.id);
                    }
                }
            }
        }

        // 预解析所有 USE
        for &use_id in &info.uses {
            if let Some(item) = krate.index.get(&use_id) {
                if let ItemEnum::Use(use_item) = &item.inner {
                    if let Some(target_id) = use_item.id {
                        let resolved = Self::resolve_use_chain(&krate.index, target_id);
                        info.use_resolutions.insert(use_id, resolved);
                    }
                }
            }
        }

        info
    }

    /// 获取 impl 块中的所有 item ID
    fn get_impl_items(item: &Item) -> Vec<Id> {
        if let ItemEnum::Impl(impl_data) = &item.inner {
            impl_data.items.clone()
        } else {
            Vec::new()
        }
    }

    /// 从 Type 提取类型 ID
    fn extract_type_id(ty: &rustdoc_types::Type) -> Option<Id> {
        match ty {
            rustdoc_types::Type::ResolvedPath(path) => Some(path.id),
            _ => None,
        }
    }

    /// 解析 USE 链到最终定义
    fn resolve_use_chain(index: &HashMap<Id, Item>, mut current_id: Id) -> Id {
        const MAX_DEPTH: usize = 20;

        for _ in 0..MAX_DEPTH {
            match index.get(&current_id) {
                Some(item) => {
                    if let ItemEnum::Use(use_item) = &item.inner {
                        if let Some(target_id) = use_item.id {
                            current_id = target_id;
                            continue;
                        }
                    }
                    return current_id;
                }
                None => return current_id,
            }
        }

        current_id
    }

    /// 根据 ID 获取 Item
    #[allow(dead_code)]
    pub fn get_item(&self, id: &Id) -> Option<&Item> {
        self.crate_data.index.get(id)
    }

    /// 根据 ID 获取类型名称
    #[allow(dead_code)]
    pub fn get_type_name(&self, id: &Id) -> Option<&str> {
        self.crate_data
            .index
            .get(id)
            .and_then(|item| item.name.as_deref())
    }

    /// 解析 ID 到其规范定义（跟随 pub use 链）
    #[allow(dead_code)]
    pub fn resolve_root_id(&self, id: Id) -> Id {
        // 先查缓存
        if let Some(&resolved) = self.info.use_resolutions.get(&id) {
            return resolved;
        }

        // 不在缓存中，实时解析
        Self::resolve_use_chain(&self.crate_data.index, id)
    }

    /// 获取 Item 的种类
    #[allow(dead_code)]
    pub fn get_item_kind(&self, id: &Id) -> Option<&str> {
        self.crate_data.index.get(id).map(|item| match &item.inner {
            ItemEnum::Struct(_) => "struct",
            ItemEnum::Enum(_) => "enum",
            ItemEnum::Union(_) => "union",
            ItemEnum::Trait(_) => "trait",
            ItemEnum::Function(_) => "function",
            ItemEnum::TypeAlias(_) => "type_alias",
            ItemEnum::Constant { .. } => "constant",
            ItemEnum::Static(_) => "static",
            ItemEnum::Module(_) => "module",
            ItemEnum::Use(_) => "use",
            ItemEnum::Impl(_) => "impl",
            _ => "other",
        })
    }

    /// 检查某个 ID 是否是 Trait 中定义的方法
    #[allow(dead_code)]
    pub fn is_trait_method(&self, id: &Id) -> bool {
        for &trait_id in &self.info.traits {
            if let Some(item) = self.crate_data.index.get(&trait_id) {
                if let ItemEnum::Trait(trait_data) = &item.inner {
                    if trait_data.items.contains(id) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// 获取类型实现的所有 Trait
    #[allow(dead_code)]
    pub fn get_implemented_traits(&self, type_id: &Id) -> Option<&Vec<Id>> {
        self.info.trait_impls.get(type_id)
    }

    pub fn print_stats(&self) {
        println!("=== Rustdoc 解析统计 ===");
        println!("总 Item 数: {}", self.crate_data.index.len());
        println!("类型数: {}", self.info.types.len());

        // 按类型细分
        let mut struct_count = 0;
        let mut enum_count = 0;
        let mut union_count = 0;
        let mut alias_count = 0;

        for &id in &self.info.types {
            if let Some(kind) = self.get_item_kind(&id) {
                match kind {
                    "struct" => struct_count += 1,
                    "enum" => enum_count += 1,
                    "union" => union_count += 1,
                    "type_alias" => alias_count += 1,
                    _ => {}
                }
            }
        }

        println!("  - Struct: {}", struct_count);
        println!("  - Enum: {}", enum_count);
        println!("  - Union: {}", union_count);
        println!("  - TypeAlias: {}", alias_count);
        println!("Trait 数: {}", self.info.traits.len());
        println!("顶层函数数: {}", self.info.functions.len());
        println!("常量数: {}", self.info.constants.len());
        println!("静态变量数: {}", self.info.statics.len());
        println!("USE 重导出数: {}", self.info.uses.len());
        println!("Impl 块数: {}", self.info.impls.len());
        println!("Trait 实现关系数: {}", self.info.trait_impls.len());

        // USE 解析统计
        let resolved_uses = self.info.use_resolutions.len();
        println!("  - 已解析 USE: {}", resolved_uses);
    }
}
