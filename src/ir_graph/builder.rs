use petgraph::graph::NodeIndex;
///
/// 直接使用 ParsedCrate 引用，避免重复查询
use rustdoc_types::{GenericBound, GenericParamDefKind, Id, ItemEnum, Type};
use std::collections::{HashMap, HashSet};

use super::structure::{EdgeMode, IrGraph};
use super::type_cache::{TypeCache, TypeContext, TypeKey};
use crate::{ir_graph::structure::NodeType, parse::ParsedCrate};
use log::{debug, info};

pub struct IrGraphBuilder<'ir> {
    pub(crate) parsed: &'ir ParsedCrate,
    pub(crate) graph: IrGraph,
    pub(crate) type_node_maps: HashMap<Id, NodeIndex>,
    pub(crate) op_node_maps: HashMap<Id, NodeIndex>,
    pub(crate) generic_node_maps: HashMap<String, NodeIndex>,
    pub(crate) type_impls: HashMap<Id, HashSet<Id>>,
    pub(crate) trait_impls: HashMap<Id, HashSet<Id>>,
    pub(crate) method_impls: HashMap<Id, HashSet<Id>>,
    pub(crate) other_types: HashMap<String, NodeIndex>,
    pub(crate) generic_scopes: HashMap<Id, HashSet<String>>,
    pub(crate) generics_bounds: HashMap<String, Vec<Id>>,
    /// 类型缓存：确保类型节点的唯一性
    pub(crate) type_cache: TypeCache,
}

impl<'ir> IrGraphBuilder<'ir> {
    pub fn new(parsed: &'ir ParsedCrate) -> Self {
        Self {
            parsed,
            graph: IrGraph::new(),
            type_node_maps: HashMap::new(),
            op_node_maps: HashMap::new(),
            type_impls: HashMap::new(),
            other_types: HashMap::new(),
            generic_node_maps: HashMap::new(),
            trait_impls: HashMap::new(),
            method_impls: HashMap::new(),
            generic_scopes: HashMap::new(),
            generics_bounds: HashMap::new(),
            type_cache: TypeCache::new(),
        }
    }

    /// 构建 IR 图 - 按步骤执行
    pub fn build(mut self) -> IrGraph {
        info!("=== 开始构建 IR Graph ===");

        info!("第一步：处理类型节点及其字段/变体...");
        self.build_types();

        info!("第二步：处理 Trait 节点...");
        self.build_traits();

        info!("第三步：构建 Trait 定义的方法...");
        self.build_trait_defined_methods();

        info!("第四步：展开 impl 块为方法 ID...");
        self.expand_impl_blocks();

        info!("第五步：构建类型实现的方法节点...");
        self.build_impl_methods();

        info!("第七步：处理 Constant 和 Static...");
        self.build_constants_and_statics();

        info!("第八步：处理类型泛型参数（使用 TypeCache）...");
        self.build_type_generics();

        info!("=== IR Graph 构建完成 ===");
        self.graph
    }

    /// 展开 impl 块 ID 为方法 ID
    /// struct_data.impls 存的是 impl 块 ID，需要展开为方法 ID
    fn expand_impl_blocks(&mut self) {
        // 克隆 type_impls 以避免借用冲突
        let type_impls_clone: Vec<_> = self
            .type_impls
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect();

        // 清空，准备重新填充展开后的方法 ID
        self.type_impls.clear();

        // 追踪已添加的 Implements 关系，避免重复
        let mut impl_edges_added: HashSet<(Id, Id)> = HashSet::new();

        for (type_id, impl_ids) in type_impls_clone {
            for impl_id in impl_ids {
                // impl_id 是 impl 块的 ID，解析它获取方法
                if let Some(item) = self.parsed.crate_data.index.get(&impl_id) {
                    if let ItemEnum::Impl(impl_data) = &item.inner {
                        // 如果是 trait impl，只添加一次 Implements 边
                        if let Some(trait_ref) = &impl_data.trait_ {
                            let trait_id = trait_ref.id;

                            // 检查是否已添加过此 Implements 边
                            if !impl_edges_added.contains(&(type_id, trait_id)) {
                                if let (Some(&type_node), Some(&trait_node)) = (
                                    self.type_node_maps.get(&type_id),
                                    self.type_node_maps.get(&trait_id),
                                ) {
                                    self.graph.add_type_relation(
                                        type_node,
                                        trait_node,
                                        EdgeMode::Implements,
                                        None,
                                    );
                                    impl_edges_added.insert((type_id, trait_id));

                                    debug!(
                                        "创建 Implements 边: 类型 {} -> trait {}",
                                        type_id.0, trait_id.0
                                    );
                                }
                            }
                        }

                        // 遍历 impl 块中的所有方法
                        for &method_id in &impl_data.items {
                            if let Some(method_item) = self.parsed.crate_data.index.get(&method_id)
                            {
                                if let ItemEnum::Function(_) = &method_item.inner {
                                    // 记录到 type_impls（类型自己的方法）
                                    self.type_impls
                                        .entry(type_id)
                                        .or_insert_with(HashSet::new)
                                        .insert(method_id);

                                    debug!(
                                        "展开方法: 类型 {} 的方法: {}",
                                        type_id.0,
                                        method_item.name.as_deref().unwrap_or("?")
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        info!("展开后: {} 个类型的方法", self.type_impls.len());
        info!("添加了 {} 条 Implements 边", impl_edges_added.len());
        info!("Trait 定义的方法: {} 个", self.method_impls.len());
    }

    fn build_type_fields(&mut self) {
        // 处理 struct 字段
        for &field_id in &self.parsed.info.struct_fields {
            if let Some(item) = self.parsed.crate_data.index.get(&field_id) {
                if let ItemEnum::StructField(field_type) = &item.inner {
                    let field_name = item.name.as_deref().unwrap_or("unknown");

                    let context = TypeContext::new();
                    if let Some(type_key) = self.type_cache.create_type_key(field_type, &context) {
                        let node_index = if let Some(idx) = self.type_cache.get_node(&type_key) {
                            idx
                        } else {
                            let label = self.format_type_label(field_type, field_name);
                            let idx = self.graph.add_type_node(&label);

                            // 根据 TypeKey 设置节点类型
                            self.set_node_type_from_key(&type_key, idx);
                            self.type_cache.insert_node(type_key.clone(), idx);
                            idx
                        };

                        self.type_node_maps.insert(field_id, node_index);
                        debug!("处理 struct 字段: {} -> {:?}", field_name, type_key);
                    }
                }
            }
        }

        // 处理 enum variant 字段
        for &variant_id in &self.parsed.info.variant_fields {
            if let Some(item) = self.parsed.crate_data.index.get(&variant_id) {
                if let ItemEnum::Variant(variant) = &item.inner {
                    let variant_name = item.name.as_deref().unwrap_or("unknown");

                    match &variant.kind {
                        // Plain 变体：直接插入变体名字作为节点
                        rustdoc_types::VariantKind::Plain => {
                            let node_index = self.graph.add_type_node(variant_name);
                            self.graph.node_types.insert(node_index, NodeType::Variant);
                            self.type_node_maps.insert(variant_id, node_index);
                            debug!("处理 Plain variant: {}", variant_name);
                        }

                        // Tuple 变体：需要处理元组字段
                        rustdoc_types::VariantKind::Tuple(field_types) => {
                            // 为 tuple variant 创建一个节点
                            let node_index = self.graph.add_type_node(variant_name);
                            self.graph.node_types.insert(node_index, NodeType::Variant);
                            self.type_node_maps.insert(variant_id, node_index);

                            // 为每个元组字段创建类型节点并连接
                            let context = TypeContext::new();
                            for (idx, field_id_opt) in field_types.iter().enumerate() {
                                // field_types 是 Vec<Option<Id>>，每个 Id 指向 StructField
                                if let Some(field_id) = field_id_opt {
                                    if let Some(field_item) =
                                        self.parsed.crate_data.index.get(&field_id)
                                    {
                                        if let ItemEnum::StructField(field_type) = &field_item.inner
                                        {
                                            if let Some(type_key) = self
                                                .type_cache
                                                .create_type_key(field_type, &context)
                                            {
                                                let field_node = self.get_or_create_type_node(
                                                    &type_key,
                                                    field_type,
                                                    &format!("{}_{}", variant_name, idx),
                                                );

                                                // 连接 variant -> field
                                                self.graph.add_type_relation(
                                                    node_index,
                                                    field_node,
                                                    EdgeMode::Ref,
                                                    Some(format!("field_{}", idx)),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            debug!(
                                "处理 Tuple variant: {} ({} 字段)",
                                variant_name,
                                field_types.len()
                            );
                        }

                        // Struct 变体：跳过，后面会作为独立的结构体处理
                        rustdoc_types::VariantKind::Struct { .. } => {
                            debug!("跳过 Struct variant: {}，将在后续处理", variant_name);
                        }
                    }
                }
            }
        }
    }

    /// 从 TypeKey 获取或创建类型节点
    fn get_or_create_type_node(
        &mut self,
        type_key: &TypeKey,
        ty: &Type,
        label_context: &str,
    ) -> NodeIndex {
        if let Some(idx) = self.type_cache.get_node(type_key) {
            return idx;
        }

        // 创建新节点
        let label = self.format_type_label(ty, label_context);
        let idx = self.graph.add_type_node(&label);

        // 设置节点类型
        self.set_node_type_from_key(type_key, idx);

        // 缓存
        self.type_cache.insert_node(type_key.clone(), idx);

        idx
    }

    /// 根据 TypeKey 设置节点类型
    fn set_node_type_from_key(&mut self, type_key: &TypeKey, node_idx: NodeIndex) {
        let node_type = match type_key {
            TypeKey::Resolved(_) => NodeType::Struct, // 默认，后续会更新
            TypeKey::Primitive(_) => NodeType::Primitive,
            TypeKey::Generic { .. } => NodeType::Generic,
            TypeKey::Tuple(_) => NodeType::Tuple,
            TypeKey::FunctionPointer(_) => NodeType::Function,

            _ => NodeType::TypeAlias, // 其他类型暂时标记为 TypeAlias
        };

        self.graph.node_types.insert(node_idx, node_type);
    }

    fn format_type_label(&self, ty: &Type, context: &str) -> String {
        match ty {
            Type::Primitive(name) => name.clone(),
            Type::ResolvedPath(path) => {
                // 从 path.path 字符串中提取最后一段作为类型名
                path.path
                    .split("::")
                    .last()
                    .unwrap_or(&path.path)
                    .to_string()
            }
            Type::Generic(name) => format!("{}:{}", context, name),
            Type::BorrowedRef {
                type_, is_mutable, ..
            } => {
                let inner = self.format_type_label(type_, context);
                if *is_mutable {
                    format!("&mut {}", inner)
                } else {
                    format!("&{}", inner)
                }
            }
            Type::Slice(inner) => format!("[{}]", self.format_type_label(inner, context)),
            Type::Array { type_, len } => {
                format!("[{}; {}]", self.format_type_label(type_, context), len)
            }
            Type::Tuple(elems) => {
                let elem_strs: Vec<_> = elems
                    .iter()
                    .map(|e| self.format_type_label(e, context))
                    .collect();
                format!("({})", elem_strs.join(", "))
            }
            _ => format!("{:?}", ty),
        }
    }

    fn build_types(&mut self) {
        self.build_type_fields();
        for &type_id in &self.parsed.info.types {
            if let Some(item) = self.parsed.crate_data.index.get(&type_id) {
                match &item.inner {
                    ItemEnum::Struct(struct_data) => {
                        self.build_struct(type_id, struct_data);
                    }
                    ItemEnum::Enum(enum_data) => {
                        self.build_enum(type_id, enum_data);
                    }
                    ItemEnum::Union(union_data) => {
                        self.build_union(type_id, union_data);
                    }
                    ItemEnum::TypeAlias(_) => {
                        // TypeAlias 作为类型节点
                        // TODO: 太复杂,暂不考虑
                        let node_index = self
                            .graph
                            .add_type_node(item.name.as_deref().unwrap_or("unknown"));
                        self.type_node_maps.insert(type_id, node_index);
                    }
                    _ => {}
                }
            }
        }
    }

    fn build_struct(&mut self, struct_id: Id, struct_data: &rustdoc_types::Struct) -> NodeIndex {
        let struct_node_index = self.graph.add_type_node(
            self.parsed
                .crate_data
                .index
                .get(&struct_id)
                .unwrap()
                .name
                .as_deref()
                .unwrap_or("unknown"),
        );
        self.type_node_maps.insert(struct_id, struct_node_index);
        self.graph
            .node_types
            .insert(struct_node_index, NodeType::Struct);

        self.type_impls
            .entry(struct_id)
            .or_insert_with(HashSet::new)
            .extend(struct_data.impls.iter().map(|&id| id));
        debug!("构建 Struct: {:?}", self.get_name(&struct_id));

        match &struct_data.kind {
            rustdoc_types::StructKind::Unit => {
                // Unit struct: 自引用
                // self.add_relation(type_node.clone(), type_node.clone(), EdgeMode::Move);
                debug!("Unit struct: {:?}", self.get_name(&struct_id));
            }
            rustdoc_types::StructKind::Tuple(field_ids) => {
                for field_id_opt in field_ids {
                    if let Some(field_id) = field_id_opt {
                        let field_node_index =
                            self.type_node_maps.get(&field_id).expect("不可能没有");
                        self.graph.add_type_relation(
                            struct_node_index,
                            *field_node_index,
                            EdgeMode::Ref,
                            None,
                        );
                    }
                }
                debug!("Tuple struct: {:?}", self.get_name(&struct_id));
            }
            rustdoc_types::StructKind::Plain { fields, .. } => {
                for &field_id in fields {
                    let field_node_index = self.type_node_maps.get(&field_id).expect("不可能没有");
                    self.graph.add_type_relation(
                        struct_node_index,
                        *field_node_index,
                        EdgeMode::Ref,
                        None,
                    );
                }
                debug!("Plain struct: {:?}", self.get_name(&struct_id));
            }
        }

        struct_node_index
    }

    fn build_enum(&mut self, enum_id: Id, enum_data: &rustdoc_types::Enum) -> NodeIndex {
        let enum_node_index = self.graph.add_type_node(
            self.parsed
                .crate_data
                .index
                .get(&enum_id)
                .unwrap()
                .name
                .as_deref()
                .unwrap_or("unknown"),
        );
        self.type_node_maps.insert(enum_id, enum_node_index);
        self.graph
            .node_types
            .insert(enum_node_index, NodeType::Enum);
        self.type_impls
            .entry(enum_id)
            .or_insert_with(HashSet::new)
            .extend(enum_data.impls.iter().map(|&id| id));

        debug!("构建 Enum: {:?}", self.get_name(&enum_id));

        for &variant_id in &enum_data.variants {
            if let Some(variant_item) = self.parsed.crate_data.index.get(&variant_id) {
                if let ItemEnum::Variant(variant) = &variant_item.inner {
                    match &variant.kind {
                        rustdoc_types::VariantKind::Plain => {
                            let variant_node_index =
                                self.type_node_maps.get(&variant_id).expect("不可能没有");
                            self.graph.add_type_relation(
                                enum_node_index,
                                *variant_node_index,
                                EdgeMode::Move,
                                None,
                            );
                        }
                        rustdoc_types::VariantKind::Tuple(field_ids) => {
                            for field_id_opt in field_ids {
                                if let Some(_) = field_id_opt {
                                    let field_node_index =
                                        self.type_node_maps.get(&variant_id).expect("不可能没有");
                                    self.graph.add_type_relation(
                                        enum_node_index,
                                        *field_node_index,
                                        EdgeMode::Ref,
                                        None,
                                    );
                                }
                            }
                        }
                        rustdoc_types::VariantKind::Struct { fields, .. } => {
                            for &field_id in fields {
                                let field_node_index =
                                    self.type_node_maps.get(&field_id).expect("不可能没有");
                                self.graph.add_type_relation(
                                    enum_node_index,
                                    *field_node_index,
                                    EdgeMode::Ref,
                                    None,
                                );
                            }
                        }
                    }
                }
            }
        }

        enum_node_index
    }

    fn build_union(&mut self, union_id: Id, union_data: &rustdoc_types::Union) -> NodeIndex {
        let union_node_index = self.graph.add_type_node(
            self.parsed
                .crate_data
                .index
                .get(&union_id)
                .unwrap()
                .name
                .as_deref()
                .unwrap_or("unknown"),
        );
        self.type_node_maps.insert(union_id, union_node_index);
        self.graph
            .node_types
            .insert(union_node_index, NodeType::Union);
        self.type_impls
            .entry(union_id)
            .or_insert_with(HashSet::new)
            .extend(union_data.impls.iter().map(|&id| id));

        debug!("构建 Union: {:?}", self.get_name(&union_id));

        for &field_id in &union_data.fields {
            let field_node_index = self.type_node_maps.get(&field_id).expect("不可能没有");
            self.graph
                .add_type_relation(union_node_index, *field_node_index, EdgeMode::Ref, None);
        }

        union_node_index
    }

    // ========== 第二步：泛型参数 ==========

    fn build_type_generics(&mut self) {
        for &type_id in &self.parsed.info.types {
            if let Some(item) = self.parsed.crate_data.index.get(&type_id) {
                let generics = match &item.inner {
                    ItemEnum::Struct(s) => Some(&s.generics),
                    ItemEnum::Enum(e) => Some(&e.generics),
                    ItemEnum::Union(u) => Some(&u.generics),
                    ItemEnum::Trait(t) => Some(&t.generics),
                    ItemEnum::TypeAlias(t) => Some(&t.generics),
                    _ => None,
                };

                if let Some(generics) = generics {
                    let owner_name = item.name.as_deref().unwrap_or("unknown");
                    self.create_generics(type_id, generics, owner_name);
                }
            }
        }
    }

    fn create_generics(
        &mut self,
        owner_id: Id,
        generics: &rustdoc_types::Generics,
        owner_name: &str,
    ) {
        use super::type_cache::{GenericScope as CacheGenericScope, TypeKey};

        for param in &generics.params {
            if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                let generic_name = format!("{}:{}", owner_name, param.name);

                // 使用 TypeCache 创建泛型参数节点
                let type_key = TypeKey::Generic {
                    name: param.name.clone(),
                    scope: CacheGenericScope::Type(owner_id),
                };

                // 检查是否已存在
                let generic_node_index = if let Some(idx) = self.type_cache.get_node(&type_key) {
                    idx
                } else {
                    // 创建新节点
                    let idx = self.graph.add_type_node(&generic_name);
                    self.graph.node_types.insert(idx, NodeType::Generic);
                    self.type_cache.insert_node(type_key.clone(), idx);
                    idx
                };

                self.generic_scopes.insert(owner_id, HashSet::new());
                self.generic_scopes
                    .get_mut(&owner_id)
                    .unwrap()
                    .insert(param.name.clone());
                self.generic_node_maps
                    .insert(param.name.clone(), generic_node_index);

                debug!(
                    "创建泛型参数: {} for {} (使用 TypeCache)",
                    param.name, owner_name
                );

                // 处理 Trait 约束，创建 Require 边
                for bound in bounds {
                    if let GenericBound::TraitBound { trait_, .. } = bound {
                        let trait_id = trait_.id;
                        if let Some(&trait_node) = self.type_node_maps.get(&trait_id) {
                            // 创建 Require 边：泛型参数 -> Trait
                            self.graph.add_type_relation(
                                generic_node_index,
                                trait_node,
                                EdgeMode::Require,
                                Some(format!("{} requires", param.name)),
                            );
                            debug!(
                                "泛型约束: {} requires trait {} ({})",
                                param.name,
                                trait_id.0,
                                self.get_name(&trait_id).unwrap_or("?")
                            );
                        } else {
                            debug!(
                                "警告: 泛型 {} 约束的 trait {} 未找到节点",
                                param.name, trait_id.0
                            );
                        }
                    }
                }
            }
        }
    }

    fn build_traits(&mut self) {
        for &trait_id in &self.parsed.info.traits {
            if let Some(item) = self.parsed.crate_data.index.get(&trait_id) {
                if let ItemEnum::Trait(trait_data) = &item.inner {
                    let trait_name = item.name.as_deref().unwrap_or("unknown");

                    if self.is_blacklisted_trait(trait_name) {
                        continue;
                    }

                    let trait_node = self.graph.add_type_node(trait_name);
                    self.graph.node_types.insert(trait_node, NodeType::Trait);
                    self.type_node_maps.insert(trait_id, trait_node);

                    // 创建 Trait 自身的泛型参数
                    self.create_generics(trait_id, &trait_data.generics, trait_name);

                    // 归一化方法级别的泛型：收集所有方法的泛型，合并同名且约束相同的
                    self.normalize_trait_method_generics(trait_id, trait_name, &trait_data.items);

                    // trait_data.items 是 trait 定义的方法，放到 method_impls
                    self.method_impls
                        .entry(trait_id)
                        .or_insert_with(HashSet::new)
                        .extend(trait_data.items.iter().map(|&id| id));

                    debug!(
                        "构建 Trait: {}, 方法数: {}, Trait 泛型数: {}",
                        trait_name,
                        trait_data.items.len(),
                        trait_data.generics.params.len()
                    );
                }
            }
        }
    }

    /// 归一化 Trait 方法的泛型参数
    /// 如果多个方法有同名泛型且约束相同，则合并为一个节点
    fn normalize_trait_method_generics(
        &mut self,
        trait_id: Id,
        trait_name: &str,
        method_ids: &[Id],
    ) {
        use super::type_cache::{GenericScope as CacheGenericScope, TypeKey};
        use std::collections::HashMap;

        // 收集所有方法的泛型：泛型名 -> (约束 trait IDs, 出现次数)
        let mut generic_info: HashMap<String, (Vec<Id>, usize)> = HashMap::new();

        for &method_id in method_ids {
            if let Some(method_item) = self.parsed.crate_data.index.get(&method_id) {
                if let ItemEnum::Function(func) = &method_item.inner {
                    for param in &func.generics.params {
                        if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                            let trait_bounds: Vec<Id> = bounds
                                .iter()
                                .filter_map(|bound| {
                                    if let GenericBound::TraitBound { trait_, .. } = bound {
                                        Some(trait_.id)
                                    } else {
                                        None
                                    }
                                })
                                .collect();

                            generic_info
                                .entry(param.name.clone())
                                .and_modify(|(bounds_vec, count)| {
                                    // 检查约束是否相同
                                    if bounds_vec.len() == trait_bounds.len()
                                        && bounds_vec.iter().all(|b| trait_bounds.contains(b))
                                    {
                                        *count += 1;
                                    }
                                })
                                .or_insert((trait_bounds, 1));
                        }
                    }
                }
            }
        }

        // 为出现次数 >= 2 的泛型创建归一化节点
        for (generic_name, (trait_bounds, count)) in generic_info {
            if count >= 2 {
                let normalized_name = format!("{}:{}", trait_name, generic_name);

                // 使用 TypeCache 创建归一化的泛型节点
                let type_key = TypeKey::Generic {
                    name: generic_name.clone(),
                    scope: CacheGenericScope::Trait(trait_id),
                };

                let generic_node_index = if let Some(idx) = self.type_cache.get_node(&type_key) {
                    idx
                } else {
                    let idx = self.graph.add_type_node(&normalized_name);
                    self.graph.node_types.insert(idx, NodeType::Generic);
                    self.type_cache.insert_node(type_key.clone(), idx);
                    idx
                };

                // 注册到 generic_node_maps，使用 trait_name:T 作为 key
                self.generic_node_maps
                    .insert(normalized_name.clone(), generic_node_index);

                // 创建 Require 边
                for &trait_id in &trait_bounds {
                    if let Some(&trait_node) = self.type_node_maps.get(&trait_id) {
                        self.graph.add_type_relation(
                            generic_node_index,
                            trait_node,
                            EdgeMode::Require,
                            Some(format!("{} requires", generic_name)),
                        );
                    }
                }

                debug!(
                    "归一化泛型: {} (出现 {} 次，约束: {} 个)",
                    normalized_name,
                    count,
                    trait_bounds.len()
                );
            }
        }
    }

    // ========== 第五步：Constant 和 Static ==========
    fn build_constants_and_statics(&mut self) {
        for &constant_id in &self.parsed.info.constants {
            if let Some(item) = self.parsed.crate_data.index.get(&constant_id) {
                if let ItemEnum::Constant { type_, const_, .. } = &item.inner {
                    let constant_name = item.name.as_deref().unwrap_or("unknown");
                    self.type_node_maps
                        .insert(constant_id, self.graph.add_type_node(constant_name));
                }
            }
        }
    }
    // ========== 第六步：泛型约束 ==========

    fn build_path(&self, id: &Id) -> String {
        if let Some(summary) = self.parsed.crate_data.paths.get(id) {
            summary.path.join("::")
        } else {
            format!("unknown_{:?}", id)
        }
    }

    fn get_name(&self, id: &Id) -> Option<&str> {
        self.parsed.crate_data.index.get(id)?.name.as_deref()
    }

    fn is_blacklisted_method(&self, name: &str) -> bool {
        crate::support_types::is_blacklisted_method(name)
    }

    fn is_blacklisted_trait(&self, name: &str) -> bool {
        use crate::support_types::TRAIT_BLACKLIST;
        TRAIT_BLACKLIST.contains(&name)
    }
}
