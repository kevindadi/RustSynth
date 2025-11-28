/// Parse 模块:负责解析 rustdoc JSON 输出
///
/// 该模块的职责是从 rustdoc 生成的 JSON 文件中提取所有相关信息,
/// 包括类型、函数、Trait 实现关系等,为 API 图构建提供数据基础.
use rustdoc_types::{Crate, GenericBound, Id, Item, ItemEnum, Type};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// 解析后的 Crate 信息
#[derive(Debug, Clone)]
pub struct ParsedCrate {
    /// 原始的 Crate 数据
    pub crate_data: Crate,
    /// 类型索引:Id -> Item
    pub type_index: HashMap<Id, Item>,
    /// Trait 实现映射:类型 Id -> 它实现的 Trait Id 列表
    pub trait_implementations: HashMap<Id, Vec<Id>>,
    /// 函数列表:(函数 Id, 函数信息)
    pub functions: Vec<FunctionInfo>,
    /// 类型列表:(类型 Id, 类型种类)
    pub types: Vec<TypeInfo>,
    /// Impl 块列表
    pub impl_blocks: Vec<ImplBlockInfo>,
    /// Trait 信息列表
    pub traits: Vec<TraitInfo>,
    /// Constant 列表
    pub constants: Vec<ConstantInfo>,
    /// Static 列表
    pub statics: Vec<StaticInfo>,
}

/// Impl 块信息
#[derive(Debug, Clone)]
pub struct ImplBlockInfo {
    /// Impl 块 Id
    pub id: Id,
    /// 实现的 Trait(如果是 trait impl)
    pub trait_id: Option<Id>,
    /// 实现的目标类型(Self 类型)
    pub for_type: Id,
    /// Impl 块中的方法/函数 Id 列表
    pub items: Vec<Id>,
    /// Impl 的泛型参数
    pub generics: Vec<String>,
    /// 关联类型映射 (关联类型名 -> 具体类型 ID)
    /// 例如: "Config" -> GeneralPurposeConfig 的 ID
    pub assoc_types: std::collections::HashMap<String, Id>,
}

/// Trait 信息
#[derive(Debug, Clone)]
pub struct TraitInfo {
    /// Trait Id
    pub id: Id,
    /// Trait 名称
    pub name: String,
    /// Trait 中定义的方法 Id 列表
    pub methods: Vec<Id>,
    /// Trait 的泛型参数
    pub generics: Vec<Id>, // TODO: 完整的泛型信息
}

/// 函数信息
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    pub id: Id,
    pub name: String,
    pub inputs: Vec<TypeRef>,
    pub output: Option<TypeRef>,
    pub generic_constraints: Vec<GenericConstraint>,
}

/// 类型引用
#[derive(Debug, Clone)]
pub enum TypeRef {
    /// 具体的已解析类型
    Resolved(Id),
    /// 泛型参数
    Generic(String),
    /// 原始类型
    Primitive(String),
    /// impl Trait
    ImplTrait(Vec<Id>), // Trait Id 列表
    /// 其他复合类型(元组、数组等)
    Composite(Vec<TypeRef>),
}

/// 泛型约束
#[derive(Debug, Clone)]
pub struct GenericConstraint {
    /// 泛型参数名
    pub param_name: String,
    /// 需要实现的 Trait Id
    pub required_trait: Id,
}

/// 类型信息
#[derive(Debug, Clone)]
pub struct TypeInfo {
    pub id: Id,
    pub name: String,
    pub kind: TypeKind,
    /// 结构体的公开字段(仅对 Struct 和 Union 有效)
    pub fields: Vec<FieldInfo>,
    /// 枚举的变体(仅对 Enum 有效)
    pub variants: Vec<VariantInfo>,
    /// 完整路径 (例如: "crate::module::Type")
    /// 后续可能用于 use 导入
    #[allow(unused)]
    pub path: Option<String>,
}

/// Constant 信息
#[derive(Debug, Clone)]
pub struct ConstantInfo {
    pub id: Id,
    pub name: String,
    pub type_id: Id,
    pub path: String,
}

/// Static 信息
#[derive(Debug, Clone)]
pub struct StaticInfo {
    pub id: Id,
    pub name: String,
    pub type_id: Id,
    pub path: String,
    pub is_mutable: bool,
}

/// 字段信息
#[derive(Debug, Clone)]
pub struct FieldInfo {
    /// 字段 Id(来自 rustdoc)
    pub id: Id,
    /// 字段名
    pub name: String,
    /// 字段类型(作为 TypeRef)
    pub field_type: TypeRef,
    /// 是否公开
    pub is_public: bool,
}

/// 枚举变体信息
#[derive(Debug, Clone)]
pub struct VariantInfo {
    /// 变体 Id(来自 rustdoc)
    pub id: Id,
    /// 变体名
    pub name: String,
    /// 变体的字段(如果是 struct-like 或 tuple-like)
    pub fields: Vec<FieldInfo>,
    /// 变体类型
    pub kind: VariantKindInfo,
}

/// 变体类型
#[derive(Debug, Clone, PartialEq)]
pub enum VariantKindInfo {
    /// 无字段变体:None
    Plain,
    /// 元组变体:Some(T)
    Tuple,
    /// 结构体变体:Point { x: i32, y: i32 }
    Struct,
}

/// 类型种类
#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    Struct,
    Enum,
    Trait,
    Union,
    TypeAlias,
}

impl ParsedCrate {
    /// 从 JSON 文件加载并解析
    pub fn from_json_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let krate: Crate = serde_json::from_reader(reader)?;

        Ok(Self::from_crate(krate))
    }

    /// 从 Crate 对象解析
    pub fn from_crate(krate: Crate) -> Self {
        let type_index = krate.index.clone();

        let mut parsed = ParsedCrate {
            crate_data: krate,
            type_index,
            trait_implementations: HashMap::new(),
            functions: Vec::new(),
            types: Vec::new(),
            impl_blocks: Vec::new(),
            traits: Vec::new(),
            constants: Vec::new(),
            statics: Vec::new(),
        };

        // 1. 提取 Trait 信息
        parsed.extract_traits();

        // 2. 提取 Trait 实现关系
        parsed.extract_trait_implementations();

        // 3. 提取函数信息
        parsed.extract_functions();

        // 4. 提取类型信息
        parsed.extract_types();

        // 5. 提取 Impl 块信息
        parsed.extract_impl_blocks();

        // 6. 提取 Constant 和 Static
        parsed.extract_constants_and_statics();

        parsed
    }

    /// 提取 Trait 信息
    fn extract_traits(&mut self) {
        for (&id, item) in &self.crate_data.index {
            if let ItemEnum::Trait(trait_item) = &item.inner {
                let name = item.name.as_deref().unwrap_or("anonymous").to_string();

                self.traits.push(TraitInfo {
                    id,
                    name,
                    methods: trait_item.items.clone(),
                    generics: Vec::new(),
                });
            }
        }
    }

    /// 提取 Impl 块信息
    fn extract_impl_blocks(&mut self) {
        for (&id, item) in &self.crate_data.index {
            if let ItemEnum::Impl(impl_item) = &item.inner {
                // 过滤:检查 Auto Trait 和 Blanket Implementation
                use crate::support_types::{TRAIT_BLACKLIST, should_filter_impl};
                if should_filter_impl(impl_item, item.crate_id, TRAIT_BLACKLIST) {
                    log::debug!("跳过 Auto Trait 或 Blanket Implementation: impl {:?}", id);
                    continue;
                }

                // 过滤:检查 Trait 黑名单(额外检查,should_filter_impl 已包含但更严格)
                if let Some(trait_ref) = &impl_item.trait_ {
                    if Self::is_blacklisted_trait(trait_ref) {
                        continue;
                    }
                }

                // 提取实现的目标类型
                if let Some(for_type_id) = Self::extract_type_id(&impl_item.for_) {
                    // 解析到规范 ID(跟随 pub use 链)
                    let canonical_for_type = self.resolve_root_id(for_type_id);
                    let trait_id = impl_item.trait_.as_ref().map(|t| t.id);

                    let generics: Vec<String> = impl_item
                        .generics
                        .clone()
                        .params
                        .iter()
                        .map(|g| g.name.clone())
                        .collect();

                    // 提取关联类型映射
                    let mut assoc_types = std::collections::HashMap::new();
                    for &item_id in &impl_item.items {
                        if let Some(item) = self.crate_data.index.get(&item_id) {
                            if let ItemEnum::AssocType { type_, .. } = &item.inner {
                                // type_ 是 Option<Type>，需要先解包
                                if let Some(actual_type) = type_ {
                                    if let Some(type_id) = Self::extract_type_id(actual_type) {
                                        let assoc_name = item
                                            .name
                                            .as_ref()
                                            .map(|s| s.as_str())
                                            .unwrap_or("unknown")
                                            .to_string();
                                        assoc_types.insert(assoc_name, type_id);
                                        log::debug!(
                                            "关联类型: {} -> {:?}",
                                            item.name.as_ref().unwrap(),
                                            type_id
                                        );
                                    }
                                }
                            }
                        }
                    }

                    self.impl_blocks.push(ImplBlockInfo {
                        id,
                        trait_id,
                        for_type: canonical_for_type,
                        items: impl_item.items.clone(),
                        generics: generics,
                        assoc_types,
                    });
                }
            }
        }
    }

    /// Pass 1: 构建 TraitMap (类型 -> 它实现的 Trait 列表)
    fn extract_trait_implementations(&mut self) {
        for (_id, item) in &self.crate_data.index {
            if let ItemEnum::Impl(impl_item) = &item.inner {
                use crate::support_types::{TRAIT_BLACKLIST, should_filter_impl};
                if should_filter_impl(impl_item, item.crate_id, TRAIT_BLACKLIST) {
                    continue;
                }

                // 只处理 Trait 实现(不是固有实现)
                if let Some(trait_ref) = &impl_item.trait_ {
                    if Self::is_blacklisted_trait(trait_ref) {
                        continue;
                    }

                    let trait_id = trait_ref.id;

                    if let Some(implementor_id) = Self::extract_type_id(&impl_item.for_) {
                        let canonical_implementor = self.resolve_root_id(implementor_id);

                        // 存储: 类型 -> 它实现的 Trait 列表
                        self.trait_implementations
                            .entry(canonical_implementor)
                            .or_insert_with(Vec::new)
                            .push(trait_id);
                    }
                }
            }
        }
    }

    /// 提取函数信息
    fn extract_functions(&mut self) {
        for (&id, item) in &self.crate_data.index {
            if let ItemEnum::Function(func) = &item.inner {
                let name = item.name.as_deref().unwrap_or("anonymous").to_string();

                if Self::is_blacklisted_method(&name) {
                    continue;
                }

                let inputs: Vec<TypeRef> = func
                    .sig
                    .inputs
                    .iter()
                    .map(|(_, ty)| Self::extract_type_ref(ty))
                    .collect();

                let output = func
                    .sig
                    .output
                    .as_ref()
                    .map(|ty| Self::extract_type_ref(ty));

                let generic_constraints = Self::extract_generic_constraints_from_item(item);

                self.functions.push(FunctionInfo {
                    id,
                    name,
                    inputs,
                    output,
                    generic_constraints,
                });
            }
        }
    }

    /// 提取类型信息
    fn extract_types(&mut self) {
        for (&id, item) in &self.crate_data.index {
            let (kind, fields, variants) = match &item.inner {
                ItemEnum::Struct(struct_item) => {
                    // 提取结构体字段(根据 StructKind)
                    let fields = match &struct_item.kind {
                        // Plain struct: struct Foo { a: T, b: U }
                        rustdoc_types::StructKind::Plain { fields, .. } => {
                            self.extract_struct_fields(fields)
                        }
                        // Tuple struct: struct Foo(T, U)
                        rustdoc_types::StructKind::Tuple(fields) => {
                            self.extract_tuple_fields(fields)
                        }
                        // Unit struct: struct Foo;
                        rustdoc_types::StructKind::Unit => Vec::new(),
                    };
                    (Some(TypeKind::Struct), fields, Vec::new())
                }
                ItemEnum::Enum(enum_item) => {
                    // 提取枚举变体
                    let variants = self.extract_enum_variants(&enum_item.variants);
                    (Some(TypeKind::Enum), Vec::new(), variants)
                }
                ItemEnum::Trait(_) => (Some(TypeKind::Trait), Vec::new(), Vec::new()),
                ItemEnum::Union(union_item) => {
                    // Union 的字段类似于 struct
                    let fields = self.extract_struct_fields(&union_item.fields);
                    (Some(TypeKind::Union), fields, Vec::new())
                }
                ItemEnum::TypeAlias(_) => (Some(TypeKind::TypeAlias), Vec::new(), Vec::new()),
                // Constant 和 Static 在单独的函数中处理
                ItemEnum::Constant { .. } | ItemEnum::Static(_) => (None, Vec::new(), Vec::new()),
                _ => (None, Vec::new(), Vec::new()),
            };

            if let Some(kind) = kind {
                let name = item.name.as_deref().unwrap_or("anonymous").to_string();

                let path = Some(self.build_item_path(id));

                self.types.push(TypeInfo {
                    id,
                    name,
                    kind,
                    fields,
                    variants,
                    path,
                });
            }
        }
    }

    /// 提取 Constant 和 Static
    fn extract_constants_and_statics(&mut self) {
        for (&id, item) in &self.crate_data.index {
            match &item.inner {
                ItemEnum::Constant { type_, .. } => {
                    if let Some(type_id) = Self::extract_type_id(type_) {
                        let name = item.name.as_deref().unwrap_or("anonymous").to_string();
                        let path = self.build_item_path(id);

                        log::debug!("提取 constant: {} -> {:?}", &path, type_id);

                        self.constants.push(ConstantInfo {
                            id,
                            name,
                            type_id,
                            path,
                        });
                    }
                }
                ItemEnum::Static(static_data) => {
                    if let Some(type_id) = Self::extract_type_id(&static_data.type_) {
                        let name = item.name.as_deref().unwrap_or("anonymous").to_string();
                        let path = self.build_item_path(id);

                        log::debug!("提取 static: {} -> {:?}", &path, type_id);

                        self.statics.push(StaticInfo {
                            id,
                            name,
                            type_id,
                            path,
                            is_mutable: static_data.is_mutable,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    /// 构建 Item 的完整路径
    fn build_item_path(&self, id: Id) -> String {
        // 尝试从 paths 中获取路径
        if let Some(summary) = self.crate_data.paths.get(&id) {
            return summary.path.join("::");
        }

        // 如果没有，尝试从 item 名称构建
        if let Some(item) = self.type_index.get(&id) {
            if let Some(name) = &item.name {
                return name.clone();
            }
        }

        format!("unknown_{:?}", id)
    }

    /// 提取 Plain struct 字段
    fn extract_struct_fields(&self, field_ids: &[Id]) -> Vec<FieldInfo> {
        field_ids
            .iter()
            .filter_map(|&field_id| {
                let field_item = self.crate_data.index.get(&field_id)?;
                // StructField 直接包含 Type
                if let ItemEnum::StructField(field_type) = &field_item.inner {
                    let is_public =
                        matches!(field_item.visibility, rustdoc_types::Visibility::Public);
                    Some(FieldInfo {
                        id: field_id,
                        name: field_item.name.as_ref()?.clone(),
                        field_type: Self::extract_type_ref(field_type),
                        is_public,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// 提取元组结构体字段
    /// Tuple struct: struct Point(pub f32, pub f32)
    fn extract_tuple_fields(&self, field_ids: &[Option<Id>]) -> Vec<FieldInfo> {
        field_ids
            .iter()
            .enumerate()
            .filter_map(|(idx, field_id_opt)| {
                let field_id = (*field_id_opt)?;
                let field_item = self.crate_data.index.get(&field_id)?;
                if let ItemEnum::StructField(field_type) = &field_item.inner {
                    let is_public =
                        matches!(field_item.visibility, rustdoc_types::Visibility::Public);
                    Some(FieldInfo {
                        id: field_id,
                        name: format!("{}", idx), // 元组字段用索引命名
                        field_type: Self::extract_type_ref(field_type),
                        is_public,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// 提取枚举变体
    fn extract_enum_variants(&self, variant_ids: &[Id]) -> Vec<VariantInfo> {
        variant_ids
            .iter()
            .filter_map(|&variant_id| {
                let variant_item = self.crate_data.index.get(&variant_id)?;
                if let ItemEnum::Variant(variant) = &variant_item.inner {
                    let (kind, fields) = match &variant.kind {
                        rustdoc_types::VariantKind::Plain => (VariantKindInfo::Plain, Vec::new()),
                        rustdoc_types::VariantKind::Tuple(field_ids) => {
                            (VariantKindInfo::Tuple, self.extract_tuple_fields(field_ids))
                        }
                        rustdoc_types::VariantKind::Struct { fields, .. } => {
                            (VariantKindInfo::Struct, self.extract_struct_fields(fields))
                        }
                    };

                    Some(VariantInfo {
                        id: variant_id,
                        name: variant_item.name.as_ref()?.clone(),
                        fields,
                        kind,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// 从 Type 提取类型引用
    fn extract_type_ref(ty: &Type) -> TypeRef {
        match ty {
            Type::ResolvedPath(path) => TypeRef::Resolved(path.id),
            Type::Generic(name) => TypeRef::Generic(name.clone()),
            Type::Primitive(name) => TypeRef::Primitive(name.clone()),
            Type::ImplTrait(bounds) => {
                let trait_ids: Vec<Id> = bounds
                    .iter()
                    .filter_map(|bound| {
                        if let GenericBound::TraitBound { trait_, .. } = bound {
                            Some(trait_.id)
                        } else {
                            None
                        }
                    })
                    .collect();
                TypeRef::ImplTrait(trait_ids)
            }
            Type::Tuple(elements) => {
                let refs: Vec<TypeRef> = elements.iter().map(Self::extract_type_ref).collect();
                TypeRef::Composite(refs)
            }
            Type::Array { type_, .. } => TypeRef::Composite(vec![Self::extract_type_ref(type_)]),
            Type::Slice(inner) => TypeRef::Composite(vec![Self::extract_type_ref(inner)]),
            Type::BorrowedRef { type_, .. } => Self::extract_type_ref(type_),
            Type::QualifiedPath { self_type, .. } => Self::extract_type_ref(self_type),
            _ => TypeRef::Primitive("unknown".to_string()),
        }
    }

    /// 从 Type 提取主要的类型 Id
    fn extract_type_id(ty: &Type) -> Option<Id> {
        match ty {
            Type::ResolvedPath(path) => Some(path.id),
            _ => None,
        }
    }

    /// 从 Item 提取泛型约束
    fn extract_generic_constraints_from_item(_item: &Item) -> Vec<GenericConstraint> {
        // rustdoc-types 0.57 中,泛型信息可能在不同位置
        // 暂时返回空列表,因为 FunctionSignature 可能没有直接的 generics 字段
        // 实际实现中,可能需要从函数的其他部分提取泛型信息
        Vec::new()
    }

    /// 获取类型名称
    pub fn get_type_name(&self, id: &Id) -> Option<&str> {
        self.type_index
            .get(id)
            .and_then(|item| item.name.as_deref())
    }

    /// 获取 Trait 的所有实现者 Trait implementations
    pub fn get_trait_implementors(&self, trait_id: &Id) -> Option<&Vec<Id>> {
        self.trait_implementations.get(trait_id)
    }

    /// 检查某个 ID 是否是 Trait 中定义的方法
    ///
    /// # 返回值
    /// - `true`: 该 ID 是某个 Trait 的方法定义(抽象方法)
    /// - `false`: 该 ID 不是 Trait 方法,可以创建操作节点
    pub fn is_trait_method(&self, id: &Id) -> bool {
        self.traits
            .iter()
            .any(|trait_info| trait_info.methods.contains(id))
    }

    /// 解析 ID 到其规范定义(跟随 pub use 链)
    ///
    /// # 示例
    /// ```ignore
    /// // Item A (ID: 100) 是 struct RealType
    /// // Item B (ID: 200) 是 pub use A;
    /// // resolve_root_id(Id(200)) -> Id(100)
    /// ```
    pub fn resolve_root_id(&self, id: Id) -> Id {
        const MAX_DEPTH: usize = 20;
        let mut current_id = id;

        for depth in 0..MAX_DEPTH {
            match self.type_index.get(&current_id) {
                Some(item) => {
                    match &item.inner {
                        // 如果是 Use(重导出),跟随到目标
                        ItemEnum::Use(use_item) => {
                            if let Some(target_id) = use_item.id {
                                log::trace!(
                                    "解析重导出 (深度 {}): {:?} -> {:?}",
                                    depth,
                                    current_id,
                                    target_id
                                );
                                current_id = target_id;
                                continue;
                            } else {
                                // 外部 crate 的重导出,无法继续跟随
                                log::debug!("遇到外部重导出(无 target ID): {:?}", current_id);
                                return current_id;
                            }
                        }
                        // 其他类型(Struct/Enum/Function 等)是实际定义
                        _ => {
                            if depth > 0 {
                                log::debug!(
                                    "解析完成 (深度 {}): {:?} -> {:?}",
                                    depth,
                                    id,
                                    current_id
                                );
                            }
                            return current_id;
                        }
                    }
                }
                None => {
                    // ID 不在索引中(外部类型)
                    log::debug!("ID 不在索引中: {:?}", current_id);
                    return current_id;
                }
            }
        }

        // 达到最大递归深度,可能是循环引用
        log::warn!(
            "达到最大解析深度 ({}),可能存在循环重导出: {:?}",
            MAX_DEPTH,
            id
        );
        current_id
    }

    pub fn print_stats(&self) {
        println!("=== Rustdoc 解析统计 ===");
        println!("总 Item 数: {}", self.type_index.len());
        println!("函数数: {}", self.functions.len());
        println!("类型数: {}", self.types.len());
        println!("Trait 数: {}", self.traits.len());
        println!("Impl 块数: {}", self.impl_blocks.len());
        println!("Trait 实现数: {}", self.trait_implementations.len());

        let struct_count = self
            .types
            .iter()
            .filter(|t| t.kind == TypeKind::Struct)
            .count();
        let enum_count = self
            .types
            .iter()
            .filter(|t| t.kind == TypeKind::Enum)
            .count();
        let trait_count = self
            .types
            .iter()
            .filter(|t| t.kind == TypeKind::Trait)
            .count();

        println!("  - Struct: {}", struct_count);
        println!("  - Enum: {}", enum_count);
        println!("  - Trait: {}", trait_count);

        // 统计 Trait 方法总数
        let trait_method_count: usize = self.traits.iter().map(|t| t.methods.len()).sum();
        println!("  - Trait 方法总数: {}", trait_method_count);
    }

    /// 打印类型和 Trait 实现的详细摘要
    pub fn print_type_trait_summary(&self) {
        println!("\n=== 类型和 Trait 实现摘要 ===\n");

        // 1. 打印所有 Struct
        println!("【结构体 (Struct)】");
        let structs: Vec<_> = self
            .types
            .iter()
            .filter(|t| t.kind == TypeKind::Struct)
            .collect();
        println!("总数: {}\n", structs.len());

        for type_info in structs.iter().take(20) {
            println!("  • {}", type_info.name);
            // 查找实现的 trait
            if let Some(trait_ids) = self.trait_implementations.get(&type_info.id) {
                for trait_id in trait_ids {
                    if let Some(trait_info) = self.traits.iter().find(|t| &t.id == trait_id) {
                        print!("    ├─ impl {}", trait_info.name);

                        // 查找 impl 块的详细信息
                        if let Some(impl_block) = self.impl_blocks.iter().find(|ib| {
                            ib.for_type == type_info.id && ib.trait_id == Some(*trait_id)
                        }) {
                            // 显示关联类型
                            if !impl_block.assoc_types.is_empty() {
                                print!(" {{");
                                let assoc_strs: Vec<String> = impl_block
                                    .assoc_types
                                    .iter()
                                    .map(|(name, type_id)| {
                                        let type_name = self.get_type_name(type_id).unwrap_or("?");
                                        format!("{} = {}", name, type_name)
                                    })
                                    .collect();
                                print!(" {} ", assoc_strs.join(", "));
                                print!("}}");
                            }
                            println!();

                            // 显示实现的方法（包括显式重写的和 provided 的）
                            let mut all_methods = Vec::new();

                            // 1. 显式重写的方法
                            let explicit_methods: Vec<String> = impl_block
                                .items
                                .iter()
                                .filter_map(|&item_id| {
                                    let item = self.type_index.get(&item_id)?;
                                    // 只显示函数（方法），跳过关联类型
                                    if matches!(item.inner, rustdoc_types::ItemEnum::Function(_)) {
                                        item.name.clone()
                                    } else {
                                        None
                                    }
                                })
                                .collect();

                            all_methods.extend(explicit_methods);

                            // 2. Trait provided 的默认方法
                            if let Some(impl_item) = self.type_index.get(&impl_block.id) {
                                if let rustdoc_types::ItemEnum::Impl(impl_data) = &impl_item.inner {
                                    all_methods.extend(impl_data.provided_trait_methods.clone());
                                }
                            }

                            if !all_methods.is_empty() {
                                let display_count = all_methods.len().min(10);
                                println!(
                                    "      方法 ({}个): {}",
                                    all_methods.len(),
                                    all_methods
                                        .iter()
                                        .take(display_count)
                                        .cloned()
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                );
                                if all_methods.len() > 10 {
                                    println!("      ... 还有 {} 个方法", all_methods.len() - 10);
                                }
                            }
                        } else {
                            println!();
                        }
                    }
                }
            }
        }
        if structs.len() > 20 {
            println!("  ... 还有 {} 个结构体未显示", structs.len() - 20);
        }

        // 2. 打印所有 Enum
        println!("\n【枚举 (Enum)】");
        let enums: Vec<_> = self
            .types
            .iter()
            .filter(|t| t.kind == TypeKind::Enum)
            .collect();
        println!("总数: {}\n", enums.len());

        for type_info in enums.iter().take(10) {
            println!("  • {}", type_info.name);
            // 查找实现的 trait
            if let Some(trait_ids) = self.trait_implementations.get(&type_info.id) {
                for trait_id in trait_ids {
                    if let Some(trait_info) = self.traits.iter().find(|t| &t.id == trait_id) {
                        println!("    ├─ impl {}", trait_info.name);
                    }
                }
            }
        }
        if enums.len() > 10 {
            println!("  ... 还有 {} 个枚举未显示", enums.len() - 10);
        }

        // 3. 打印所有 Trait
        println!("\n【Trait】");
        println!("总数: {}\n", self.traits.len());

        for trait_info in self.traits.iter().take(10) {
            println!("  • {}", trait_info.name);

            // 显示 trait 中定义的方法
            let trait_methods: Vec<String> = trait_info
                .methods
                .iter()
                .filter_map(|&method_id| {
                    let item = self.type_index.get(&method_id)?;
                    item.name.clone()
                })
                .take(8)
                .collect();

            if !trait_methods.is_empty() {
                println!("    定义的方法: {}", trait_methods.join(", "));
                if trait_info.methods.len() > 8 {
                    println!("    ... 还有 {} 个方法", trait_info.methods.len() - 8);
                }
            }

            // 查找实现此 trait 的类型
            let implementors: Vec<String> = self
                .trait_implementations
                .iter()
                .filter(|(_, traits)| traits.contains(&trait_info.id))
                .filter_map(|(type_id, _)| {
                    self.types
                        .iter()
                        .find(|t| &t.id == type_id)
                        .map(|t| t.name.clone())
                })
                .take(5)
                .collect();

            if !implementors.is_empty() {
                println!("    实现者: {}", implementors.join(", "));
                let total_implementors = self
                    .trait_implementations
                    .iter()
                    .filter(|(_, traits)| traits.contains(&trait_info.id))
                    .count();
                if total_implementors > 5 {
                    println!("    ... 还有 {} 个实现者", total_implementors - 5);
                }
            }
        }
        if self.traits.len() > 10 {
            println!("  ... 还有 {} 个 trait 未显示", self.traits.len() - 10);
        }

        // 4. 打印 Constants
        if !self.constants.is_empty() {
            println!("\n【Constants】");
            println!("总数: {}\n", self.constants.len());
            for const_info in self.constants.iter().take(10) {
                let type_name = self.get_type_name(&const_info.type_id).unwrap_or("?");
                println!("  • {} : {}", const_info.name, type_name);
                if const_info.path != const_info.name {
                    println!("    路径: {}", const_info.path);
                }
            }
            if self.constants.len() > 10 {
                println!("  ... 还有 {} 个常量未显示", self.constants.len() - 10);
            }
        }

        // 5. 打印 Statics
        if !self.statics.is_empty() {
            println!("\n【Statics】");
            println!("总数: {}\n", self.statics.len());
            for static_info in self.statics.iter().take(5) {
                let type_name = self.get_type_name(&static_info.type_id).unwrap_or("?");
                let mutability = if static_info.is_mutable { "mut " } else { "" };
                println!(
                    "  • {}static {} : {}",
                    mutability, static_info.name, type_name
                );
            }
            if self.statics.len() > 5 {
                println!("  ... 还有 {} 个静态变量未显示", self.statics.len() - 5);
            }
        }

        println!("\n=== 摘要结束 ===\n");
    }

    /// 检查方法是否在黑名单中
    fn is_blacklisted_method(name: &str) -> bool {
        crate::support_types::is_blacklisted_method(name)
    }

    /// 检查 Trait 是否在黑名单中
    fn is_blacklisted_trait(trait_path: &rustdoc_types::Path) -> bool {
        crate::support_types::is_blacklisted_trait_path(trait_path)
    }
}
