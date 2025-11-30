use petgraph::graph::NodeIndex;
///
/// 直接使用 ParsedCrate 引用，避免重复查询
use rustdoc_types::{GenericBound, GenericParamDefKind, Id, ItemEnum, Type};
use std::collections::{HashMap, HashSet};

use super::node_info::{
    ConstantInfo, EnumInfo, FieldInfo, GenericInfo, NodeInfo, PathInfo, PrimitiveInfo, StaticInfo,
    StructInfo, TraitImplInfo, TraitInfo, UnionInfo, VariantInfo, VariantKind, Visibility,
};
use super::structure::{EdgeMode, IrGraph};
use super::type_cache::{TypeCache, TypeContext, TypeKey};
use crate::support_types::primitives::get_primitive_default_traits;
use crate::{ir_graph::structure::NodeType, parse::ParsedCrate};
use log::{debug, error, info};

pub struct IrGraphBuilder<'ir> {
    pub(crate) parsed: &'ir ParsedCrate,
    pub(crate) graph: IrGraph,
    /// 类型缓存：统一管理所有类型节点的索引
    /// 包括：类型节点、操作节点、泛型节点、关联类型节点、基本类型节点
    pub(crate) type_cache: TypeCache,
    /// 类型的 impl 块：类型 ID -> impl 块中的方法 ID 集合
    pub(crate) type_impls: HashMap<Id, HashSet<Id>>,
    /// Trait 定义的方法：Trait ID -> 方法 ID 集合
    pub(crate) method_impls: HashMap<Id, HashSet<Id>>,
    /// 泛型作用域：类型/Trait ID -> 该作用域内的泛型参数名集合
    pub(crate) generic_scopes: HashMap<Id, HashSet<String>>,
}

impl<'ir> IrGraphBuilder<'ir> {
    pub fn new(parsed: &'ir ParsedCrate) -> Self {
        Self {
            parsed,
            graph: IrGraph::new(),
            type_cache: TypeCache::new(),
            type_impls: HashMap::new(),
            method_impls: HashMap::new(),
            generic_scopes: HashMap::new(),
        }
    }

    /// 构建 IR 图 - 按步骤执行
    pub fn build(mut self) -> IrGraph {
        info!("=== 开始构建 IR Graph ===");

        info!("第一步：处理类型节点及其字段/变体...");
        self.build_types();

        info!("第二步：处理 Trait 节点...");
        self.build_traits_nodes();

        info!("第二步（续）：处理 Trait 的 Associated Type...");
        self.build_trait_assoc_types();

        info!("第三步：处理类型和 Trait 的泛型参数（使用 TypeCache)...");
        self.build_type_generics();

        info!("第四步：构建 Trait 定义的方法...");
        self.build_trait_defined_methods();

        info!("第五步：展开 impl 块为方法 ID...");
        self.expand_impl_blocks();

        info!("第六步：构建类型实现的方法节点...");
        self.build_impl_methods();

        info!("第七步：处理 Constant 和 Static...");
        self.build_constants_and_statics();

        info!("第八步：后处理 - Primitive 到 Trait 的 Implements 边...");
        self.postprocess_primitive_trait_edges();

        info!("第九步：后处理 - Generic 约束检查与 Instance 边...");
        self.postprocess_generic_constraints();

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
                                if let (Some(type_node), Some(trait_node)) = (
                                    self.type_cache.get_by_id(&type_id),
                                    self.type_cache.get_by_id(&trait_id),
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

                        // 遍历 impl 块中的所有 items（包括方法和 Associated Type）
                        for &item_id in &impl_data.items {
                            if let Some(item) = self.parsed.crate_data.index.get(&item_id) {
                                match &item.inner {
                                    ItemEnum::Function(_) => {
                                        // 记录到 type_impls（类型自己的方法）
                                        self.type_impls
                                            .entry(type_id)
                                            .or_insert_with(HashSet::new)
                                            .insert(item_id);

                                        debug!(
                                            "展开方法: 类型 {} 的方法: {}",
                                            type_id.0,
                                            item.name.as_deref().unwrap_or("?")
                                        );
                                    }
                                    ItemEnum::AssocType { type_, .. } => {
                                        // 处理 Associated Type 的重新定义
                                        if let Some(assoc_type_name) = &item.name {
                                            if let Some(trait_ref) = &impl_data.trait_ {
                                                let trait_id = trait_ref.id;

                                                // 解析关联类型的目标类型
                                                if let Some(assoc_type) = type_ {
                                                    match assoc_type {
                                                        Type::ResolvedPath(path) => {
                                                            if let Some(type_node) =
                                                                self.type_cache.get_by_id(&path.id)
                                                            {
                                                                // 获取类型和 Trait 的名称
                                                                let type_name = self
                                                                    .parsed
                                                                    .crate_data
                                                                    .index
                                                                    .get(&type_id)
                                                                    .and_then(|i| i.name.as_deref())
                                                                    .unwrap_or("unknown");

                                                                // 创建 Type.AssocType 节点
                                                                let assoc_type_label = format!(
                                                                    "{}.{}",
                                                                    type_name, assoc_type_name
                                                                );
                                                                let assoc_type_node =
                                                                    self.graph.add_type_node(
                                                                        &assoc_type_label,
                                                                    );
                                                                self.graph.node_types.insert(
                                                                    assoc_type_node,
                                                                    NodeType::TypeAlias,
                                                                );

                                                                // 存储到 type_cache
                                                                self.type_cache.insert_assoc_type(
                                                                    &type_name,
                                                                    assoc_type_name,
                                                                    assoc_type_node,
                                                                );
                                                                // 创建别名边：Type.AssocType -> TargetType
                                                                self.graph.add_type_relation(
                                                                    assoc_type_node,
                                                                    type_node,
                                                                    EdgeMode::Alias,
                                                                    Some(format!(
                                                                        "{} =",
                                                                        assoc_type_name
                                                                    )),
                                                                );

                                                                // 创建 Include 边：Type -> Type.AssocType
                                                                if let Some(source_node) = self
                                                                    .type_cache
                                                                    .get_by_id(&type_id)
                                                                {
                                                                    self.graph.add_type_relation(
                                                                        source_node,
                                                                        assoc_type_node,
                                                                        EdgeMode::Include,
                                                                        Some(format!(
                                                                            "has {}",
                                                                            assoc_type_name
                                                                        )),
                                                                    );
                                                                }
                                                            }
                                                        }
                                                        _ => {
                                                            continue;
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

                        self.type_cache.insert_type_by_id(field_id, node_index);
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
                            self.type_cache.insert_type_by_id(variant_id, node_index);
                            debug!("处理 Plain variant: {}", variant_name);
                        }

                        // Tuple 变体：需要处理元组字段
                        rustdoc_types::VariantKind::Tuple(field_types) => {
                            // 为 tuple variant 创建一个节点
                            let node_index = self.graph.add_type_node(variant_name);
                            self.graph.node_types.insert(node_index, NodeType::Variant);
                            self.type_cache.insert_type_by_id(variant_id, node_index);

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
        // 对于 Primitive 类型，先检查 primitive_to_node
        if let TypeKey::Primitive(name) = type_key {
            if let Some(&idx) = self.type_cache.primitive_to_node().get(name) {
                debug!(
                    "✓ 从 primitive_to_node 找到基本类型: {} -> NodeIndex({:?})",
                    name, idx
                );
                // 确保 PrimitiveInfo 存在
                if !self.graph.node_infos.contains_key(&idx) {
                    let default_traits = get_primitive_default_traits(name);
                    let primitive_info = PrimitiveInfo {
                        name: name.clone(),
                        default_traits,
                        trait_nodes: Vec::new(),
                    };
                    self.graph
                        .node_infos
                        .insert(idx, NodeInfo::Primitive(primitive_info));
                }
                return idx;
            }
        }

        // 从 TypeCache 查找
        // TODO: 全局构建类型缓存系统
        if let Some(idx) = self.type_cache.get_node(type_key) {
            debug!(
                "✓ 从 TypeCache 找到类型节点: {:?} -> NodeIndex({:?})",
                type_key, idx
            );
            // 对于 Primitive 类型，确保 PrimitiveInfo 存在
            if let TypeKey::Primitive(name) = type_key {
                if !self.graph.node_infos.contains_key(&idx) {
                    let default_traits = get_primitive_default_traits(name);
                    let primitive_info = PrimitiveInfo {
                        name: name.clone(),
                        default_traits,
                        trait_nodes: Vec::new(),
                    };
                    self.graph
                        .node_infos
                        .insert(idx, NodeInfo::Primitive(primitive_info));
                }
            }
            return idx;
        }

        let label = self.format_type_label(ty, label_context);
        let idx = self.graph.add_type_node(&label);

        self.set_node_type_from_key(type_key, idx);
        self.type_cache.insert_node(type_key.clone(), idx);

        // 对于 Primitive 类型，创建 PrimitiveInfo 并填充 default_traits
        if let TypeKey::Primitive(name) = type_key {
            let default_traits = get_primitive_default_traits(name);
            let primitive_info = PrimitiveInfo {
                name: name.clone(),
                default_traits,
                trait_nodes: Vec::new(), // 后续在后处理步骤中填充
            };
            self.graph
                .node_infos
                .insert(idx, NodeInfo::Primitive(primitive_info));
        }

        debug!(
            "✓ 创建新类型节点: {} (key: {:?}) -> NodeIndex({:?})",
            label, type_key, idx
        );

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

    pub(crate) fn format_type_label(&self, ty: &Type, context: &str) -> String {
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

        debug!(
            "开始构建类型节点，共 {} 个类型",
            self.parsed.info.types.len()
        );

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
                        self.type_cache.insert_type_by_id(type_id, node_index);
                    }
                    _ => {}
                }
            }
        }
    }

    fn build_struct(&mut self, struct_id: Id, struct_data: &rustdoc_types::Struct) -> NodeIndex {
        let item = self.parsed.crate_data.index.get(&struct_id).unwrap();
        let struct_name = item.name.as_deref().unwrap_or("unknown");

        // 检查是否已经存在
        if let Some(existing_node) = self.type_cache.get_by_id(&struct_id) {
            debug!(
                "⚠️  Struct {} (id: {}) 已存在节点 {:?}，跳过创建",
                struct_name, struct_id.0, existing_node
            );
            return existing_node;
        }

        let struct_node_index = self.graph.add_type_node(struct_name);
        self.type_cache
            .insert_type_by_id(struct_id, struct_node_index);
        self.graph
            .node_types
            .insert(struct_node_index, NodeType::Struct);

        self.type_impls
            .entry(struct_id)
            .or_insert_with(HashSet::new)
            .extend(struct_data.impls.iter().map(|&id| id));

        // 构建字段信息
        let mut fields = Vec::new();
        let (is_tuple_struct, is_unit_struct) = match &struct_data.kind {
            rustdoc_types::StructKind::Unit => (false, true),
            rustdoc_types::StructKind::Tuple(field_ids) => {
                for (idx, field_id_opt) in field_ids.iter().enumerate() {
                    if let Some(field_id) = field_id_opt {
                        let field_node_index =
                            self.type_cache.get_by_id(field_id).expect("不可能没有");
                        self.graph.add_type_relation(
                            struct_node_index,
                            field_node_index,
                            EdgeMode::Ref,
                            None,
                        );
                        fields.push(FieldInfo {
                            name: idx.to_string(),
                            type_node: Some(field_node_index),
                            type_str: self.graph.type_graph[field_node_index].clone(),
                            visibility: Visibility::Public,
                        });
                    }
                }
                (true, false)
            }
            rustdoc_types::StructKind::Plain {
                fields: field_ids, ..
            } => {
                for &field_id in field_ids {
                    let field_node_index =
                        self.type_cache.get_by_id(&field_id).expect("不可能没有");
                    self.graph.add_type_relation(
                        struct_node_index,
                        field_node_index,
                        EdgeMode::Ref,
                        None,
                    );
                    let field_name = self.get_name(&field_id).unwrap_or("unknown").to_string();
                    fields.push(FieldInfo {
                        name: field_name,
                        type_node: Some(field_node_index),
                        type_str: self.graph.type_graph[field_node_index].clone(),
                        visibility: Visibility::Public,
                    });
                }
                (false, false)
            }
        };

        // 创建 StructInfo 并插入
        let crate_name = self.get_crate_name();
        let struct_info = StructInfo {
            path: PathInfo::new(&format!("{}::{}", crate_name, struct_name), struct_name),
            fields,
            generics: Vec::new(),    // 后续在 build_type_generics 中填充
            trait_impls: Vec::new(), // 后续在 expand_impl_blocks 中填充
            methods: Vec::new(),     // 后续在 build_impl_methods 中填充
            is_tuple_struct,
            is_unit_struct,
            blacklisted_trait_impls: Vec::new(), // 后续在 build_impl_methods 中填充
        };
        self.graph
            .node_infos
            .insert(struct_node_index, NodeInfo::Struct(struct_info));

        debug!(
            "✓ 创建 Struct: {} (id: {}, node: {:?})",
            struct_name, struct_id.0, struct_node_index
        );

        struct_node_index
    }

    fn build_enum(&mut self, enum_id: Id, enum_data: &rustdoc_types::Enum) -> NodeIndex {
        let item = self.parsed.crate_data.index.get(&enum_id).unwrap();
        let enum_name = item.name.as_deref().unwrap_or("unknown");

        // 获取或创建 enum 节点
        let (enum_node_index, is_new) =
            if let Some(existing_node) = self.type_cache.get_by_id(&enum_id) {
                debug!(
                    "⚠️  Enum {} (id: {}) 已存在节点 {:?}，继续处理 variant",
                    enum_name, enum_id.0, existing_node
                );
                self.graph.node_types.insert(existing_node, NodeType::Enum);
                (existing_node, false)
            } else {
                let node_index = self.graph.add_type_node(enum_name);
                self.type_cache.insert_type_by_id(enum_id, node_index);
                self.graph.node_types.insert(node_index, NodeType::Enum);
                debug!(
                    "✓ 创建 Enum: {} (id: {}, node: {:?})",
                    enum_name, enum_id.0, node_index
                );
                (node_index, true)
            };

        self.type_impls
            .entry(enum_id)
            .or_insert_with(HashSet::new)
            .extend(enum_data.impls.iter().map(|&id| id));

        // 收集 variant 信息
        let mut variants = Vec::new();

        for &variant_id in &enum_data.variants {
            if let Some(variant_item) = self.parsed.crate_data.index.get(&variant_id) {
                let variant_name = variant_item.name.as_deref().unwrap_or("unknown");
                if let ItemEnum::Variant(variant) = &variant_item.inner {
                    let (variant_node_index, variant_kind) = match &variant.kind {
                        rustdoc_types::VariantKind::Plain => {
                            let variant_node_index =
                                self.type_cache.get_by_id(&variant_id).expect("不可能没有");
                            self.graph.add_type_relation(
                                enum_node_index,
                                variant_node_index,
                                EdgeMode::Move,
                                None,
                            );
                            self.graph.add_type_relation(
                                variant_node_index,
                                enum_node_index,
                                EdgeMode::Ref,
                                None,
                            );
                            debug!(
                                "  ✓ Plain variant: {} (id: {}) <-> {:?}",
                                variant_name, variant_id.0, variant_node_index
                            );
                            (variant_node_index, VariantKind::Unit)
                        }
                        rustdoc_types::VariantKind::Tuple(field_ids) => {
                            let variant_node_index =
                                self.type_cache.get_by_id(&variant_id).expect("不可能没有");
                            self.graph.add_type_relation(
                                enum_node_index,
                                variant_node_index,
                                EdgeMode::Move,
                                None,
                            );
                            self.graph.add_type_relation(
                                variant_node_index,
                                enum_node_index,
                                EdgeMode::Ref,
                                None,
                            );

                            let field_nodes: Vec<NodeIndex> = field_ids
                                .iter()
                                .filter_map(|fid_opt| {
                                    fid_opt
                                        .as_ref()
                                        .and_then(|fid| self.type_cache.get_by_id(fid))
                                })
                                .collect();
                            debug!(
                                "  ✓ Tuple variant: {} (id: {}) <-> {:?}",
                                variant_name, variant_id.0, variant_node_index
                            );
                            (variant_node_index, VariantKind::Tuple(field_nodes))
                        }
                        rustdoc_types::VariantKind::Struct {
                            fields: field_ids, ..
                        } => {
                            let variant_node_index =
                                if let Some(idx) = self.type_cache.get_by_id(&variant_id) {
                                    idx
                                } else {
                                    let idx = self.graph.add_type_node(variant_name);
                                    self.graph.node_types.insert(idx, NodeType::Variant);
                                    self.type_cache.insert_type_by_id(variant_id, idx);
                                    idx
                                };

                            self.graph.add_type_relation(
                                enum_node_index,
                                variant_node_index,
                                EdgeMode::Move,
                                None,
                            );
                            self.graph.add_type_relation(
                                variant_node_index,
                                enum_node_index,
                                EdgeMode::Ref,
                                None,
                            );

                            // 收集字段节点索引
                            let field_nodes: Vec<NodeIndex> = field_ids
                                .iter()
                                .map(|&fid| {
                                    let field_node_index =
                                        self.type_cache.get_by_id(&fid).expect("不可能没有");
                                    self.graph.add_type_relation(
                                        variant_node_index,
                                        field_node_index,
                                        EdgeMode::Ref,
                                        None,
                                    );
                                    field_node_index
                                })
                                .collect();
                            debug!(
                                "  ✓ Struct variant: {} (id: {}) <-> {:?} with {} fields",
                                variant_name,
                                variant_id.0,
                                variant_node_index,
                                field_nodes.len()
                            );
                            (variant_node_index, VariantKind::Struct(field_nodes))
                        }
                    };

                    // 为 variant 创建 NodeInfo
                    let variant_info = VariantInfo {
                        name: variant_name.to_string(),
                        parent_enum: Some(enum_node_index),
                        kind: variant_kind,
                        discriminant: variant.discriminant.as_ref().map(|d| d.value.clone()),
                    };
                    self.graph
                        .node_infos
                        .insert(variant_node_index, NodeInfo::Variant(variant_info));

                    variants.push(variant_node_index);
                }
            }
        }

        // 创建 EnumInfo（仅在新创建时）
        if is_new {
            let crate_name = self.get_crate_name();
            let enum_info = EnumInfo {
                path: PathInfo::new(&format!("{}::{}", crate_name, enum_name), enum_name),
                variants,
                generics: Vec::new(),
                trait_impls: Vec::new(),
                methods: Vec::new(),
                blacklisted_trait_impls: Vec::new(),
            };
            self.graph
                .node_infos
                .insert(enum_node_index, NodeInfo::Enum(enum_info));
        }

        enum_node_index
    }

    fn build_union(&mut self, union_id: Id, union_data: &rustdoc_types::Union) -> NodeIndex {
        let item = self.parsed.crate_data.index.get(&union_id).unwrap();
        let union_name = item.name.as_deref().unwrap_or("unknown");

        let union_node_index = self.graph.add_type_node(union_name);
        self.type_cache
            .insert_type_by_id(union_id, union_node_index);
        self.graph
            .node_types
            .insert(union_node_index, NodeType::Union);
        self.type_impls
            .entry(union_id)
            .or_insert_with(HashSet::new)
            .extend(union_data.impls.iter().map(|&id| id));

        debug!("构建 Union: {:?}", self.get_name(&union_id));

        // 收集字段信息
        let mut fields = Vec::new();
        for &field_id in &union_data.fields {
            let field_node_index = self.type_cache.get_by_id(&field_id).expect("不可能没有");
            self.graph
                .add_type_relation(union_node_index, field_node_index, EdgeMode::Ref, None);

            let field_name = self.get_name(&field_id).unwrap_or("unknown").to_string();
            fields.push(FieldInfo {
                name: field_name,
                type_node: Some(field_node_index),
                type_str: self.graph.type_graph[field_node_index].clone(),
                visibility: Visibility::Public,
            });
        }

        // 创建 UnionInfo
        let crate_name = self.get_crate_name();
        let union_info = UnionInfo {
            path: PathInfo::new(&format!("{}::{}", crate_name, union_name), union_name),
            fields,
            generics: Vec::new(),
            trait_impls: Vec::new(),
            methods: Vec::new(),
            blacklisted_trait_impls: Vec::new(),
        };
        self.graph
            .node_infos
            .insert(union_node_index, NodeInfo::Union(union_info));

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
                let (generic_node_index, is_new) =
                    if let Some(idx) = self.type_cache.get_node(&type_key) {
                        (idx, false)
                    } else {
                        // 创建新节点
                        let idx = self.graph.add_type_node(&generic_name);
                        self.graph.node_types.insert(idx, NodeType::Generic);
                        self.type_cache.insert_node(type_key.clone(), idx);
                        (idx, true)
                    };

                self.generic_scopes.insert(owner_id, HashSet::new());
                self.generic_scopes
                    .get_mut(&owner_id)
                    .unwrap()
                    .insert(param.name.clone());

                // 插入两个 key：完整名和短名
                // 使用 type_cache 的 insert_generic 方法
                self.type_cache.insert_generic(
                    generic_name.clone(),
                    Some(param.name.clone()),
                    generic_node_index,
                );

                debug!(
                    "创建泛型参数: {} (存储为 {} 和 {}), TypeCache key: {:?}",
                    generic_name, generic_name, param.name, type_key
                );

                // 收集 trait bounds
                let mut bound_nodes = Vec::new();

                // 创建 Include 边：类型/Trait -> 泛型参数
                let owner_node = self.type_cache.get_by_id(&owner_id);
                if let Some(owner) = owner_node {
                    self.graph.add_type_relation(
                        owner,
                        generic_node_index,
                        EdgeMode::Include,
                        Some(format!("has generic {}", param.name)),
                    );
                }

                // 处理 Trait 约束，创建 Require 边
                for bound in bounds {
                    if let GenericBound::TraitBound { trait_, .. } = bound {
                        let trait_id = trait_.id;

                        // 获取 Trait 的完整名称（包含具体类型参数）
                        let trait_full_name = Self::get_trait_full_name(trait_);
                        let (has_concrete_args, _) =
                            Self::check_trait_args_for_concrete_type(&trait_.args);

                        // 获取或创建 Trait 节点
                        // 对于带有具体类型参数的 Trait（如 AsRef<[u8]>），使用完整名称作为节点
                        let trait_node = if has_concrete_args {
                            // 带具体类型参数的 Trait，使用完整名称创建新节点
                            // 使用 TypeKey 来缓存，避免重复创建
                            let type_key = TypeKey::TraitWithArgs {
                                trait_id,
                                args_repr: trait_full_name.clone(),
                            };
                            if let Some(node) = self.type_cache.get_node(&type_key) {
                                node
                            } else {
                                let node = self.graph.add_type_node(&trait_full_name);
                                self.graph.node_types.insert(node, NodeType::Trait);
                                self.type_cache.insert_node(type_key, node);
                                debug!(
                                    "创建带参数的外部 Trait 节点: {} (id: {})",
                                    trait_full_name, trait_id.0
                                );
                                node
                            }
                        } else if let Some(node) = self.type_cache.get_by_id(&trait_id) {
                            node
                        } else {
                            // Trait 不在本 crate 中（外部 Trait），创建节点
                            let trait_name = trait_.path.split("::").last().unwrap_or(&trait_.path);
                            let node = self.graph.add_type_node(trait_name);
                            self.graph.node_types.insert(node, NodeType::Trait);
                            self.type_cache.insert_type_by_id(trait_id, node);

                            debug!("创建外部 Trait 节点: {} (id: {})", trait_name, trait_id.0);
                            node
                        };

                        bound_nodes.push(trait_node);

                        // 创建 Require 边：Trait -> 泛型参数
                        // 在 Petri 网语义中，实现了 Trait 的类型会向 Trait 库所发送 token，
                        // 然后这些 token 可以流向需要该 Trait 的泛型
                        self.graph.add_type_relation(
                            trait_node,
                            generic_node_index,
                            EdgeMode::Require,
                            Some(format!("required by {}", param.name)),
                        );
                        debug!(
                            "泛型约束: trait {} -> {} (Petri net flow)",
                            trait_full_name, param.name
                        );
                    }
                }

                // 创建 GenericInfo（仅在新创建时）
                if is_new {
                    let generic_info = GenericInfo {
                        name: param.name.clone(),
                        owner: owner_node,
                        bounds: bound_nodes,
                        default_type: None, // TODO: 处理默认类型
                    };
                    self.graph
                        .node_infos
                        .insert(generic_node_index, NodeInfo::Generic(generic_info));
                }
            }
        }
    }

    fn build_traits_nodes(&mut self) {
        let crate_name = self.get_crate_name();

        for &trait_id in &self.parsed.info.traits {
            if let Some(item) = self.parsed.crate_data.index.get(&trait_id) {
                if let ItemEnum::Trait(trait_data) = &item.inner {
                    let trait_name = item.name.as_deref().unwrap_or("unknown");

                    if self.is_blacklisted_trait(trait_name) {
                        continue;
                    }

                    let trait_node = self.graph.add_type_node(trait_name);
                    self.graph.node_types.insert(trait_node, NodeType::Trait);
                    self.type_cache.insert_type_by_id(trait_id, trait_node);

                    // 创建 TraitInfo
                    let trait_info = TraitInfo {
                        path: PathInfo::new(&format!("{}::{}", crate_name, trait_name), trait_name),
                        associated_types: Vec::new(), // 后续在 build_trait_assoc_types 中填充
                        associated_consts: Vec::new(),
                        methods: Vec::new(), // 后续在 build_trait_defined_methods 中填充
                        supertraits: Vec::new(), // TODO: 处理 supertrait bounds
                        generics: Vec::new(), // 后续在 create_generics 中填充
                        is_auto: trait_data.is_auto,
                        is_unsafe: trait_data.is_unsafe,
                    };
                    self.graph
                        .node_infos
                        .insert(trait_node, NodeInfo::Trait(trait_info));

                    // 创建 Trait 自身的泛型参数
                    self.create_generics(trait_id, &trait_data.generics, trait_name);

                    // 归一化方法级别的泛型：收集所有方法的泛型，合并同名且约束相同的
                    self.normalize_trait_method_generics(trait_id, trait_name, &trait_data.items);
                }
            }
        }
    }

    fn build_trait_assoc_types(&mut self) {
        for &trait_id in &self.parsed.info.traits {
            if let Some(item) = self.parsed.crate_data.index.get(&trait_id) {
                if let ItemEnum::Trait(trait_data) = &item.inner {
                    let trait_name = item.name.as_deref().unwrap_or("unknown");
                    let trait_node = self.type_cache.get_by_id(&trait_id).expect("不可能没有");
                    if self.is_blacklisted_trait(trait_name) {
                        continue;
                    }

                    // 处理 Trait 中定义的 Associated Type
                    for &item_id in &trait_data.items {
                        if let Some(item) = self.parsed.crate_data.index.get(&item_id) {
                            if let ItemEnum::AssocType { type_, bounds, .. } = &item.inner {
                                if let Some(assoc_type_name) = &item.name {
                                    // 创建 Trait.AssocType 节点（无论是否有默认值）
                                    let assoc_type_label =
                                        format!("{}.{}", trait_name, assoc_type_name);
                                    let assoc_type_node =
                                        self.graph.add_type_node(&assoc_type_label);
                                    self.graph
                                        .node_types
                                        .insert(assoc_type_node, NodeType::TypeAlias);

                                    // 存储到 type_cache
                                    self.type_cache.insert_assoc_type(
                                        trait_name,
                                        assoc_type_name,
                                        assoc_type_node,
                                    );

                                    // 创建 Include 边：Trait -> Trait.AssocType
                                    self.graph.add_type_relation(
                                        trait_node,
                                        assoc_type_node,
                                        EdgeMode::Include,
                                        Some(format!("has {}", assoc_type_name)),
                                    );

                                    // 如果有默认类型定义，创建别名边
                                    if let Some(assoc_type) = type_ {
                                        // 对于 ResolvedPath，直接从 type_cache 查找（内部类型）
                                        if let Type::ResolvedPath(path) = assoc_type {
                                            if let Some(target_node) =
                                                self.type_cache.get_by_id(&path.id)
                                            {
                                                // 创建别名边：Trait.AssocType -> TargetType
                                                self.graph.add_type_relation(
                                                    assoc_type_node,
                                                    target_node,
                                                    EdgeMode::Alias,
                                                    Some(format!("{} =", assoc_type_name)),
                                                );

                                                debug!(
                                                    "Trait {} 定义 Associated Type: {} = {} (id: {})",
                                                    trait_name,
                                                    assoc_type_name,
                                                    self.get_name(&path.id).unwrap_or("unknown"),
                                                    path.id.0
                                                );
                                            } else {
                                                error!(
                                                    "Trait {} 的 Associated Type {} 指向的类型 (id: {}) 未找到节点",
                                                    trait_name, assoc_type_name, path.id.0
                                                );
                                            }
                                        } else {
                                            // 对于其他类型（如泛型、基本类型等），使用 TypeCache
                                            use super::type_cache::TypeContext;
                                            let context = TypeContext::new();

                                            if let Some(type_key) = self
                                                .type_cache
                                                .create_type_key(assoc_type, &context)
                                            {
                                                let target_node = self.get_or_create_type_node(
                                                    &type_key,
                                                    assoc_type,
                                                    &assoc_type_label,
                                                );

                                                // 创建别名边：Trait.AssocType -> TargetType
                                                self.graph.add_type_relation(
                                                    assoc_type_node,
                                                    target_node,
                                                    EdgeMode::Alias,
                                                    Some(format!("{} =", assoc_type_name)),
                                                );

                                                debug!(
                                                    "Trait {} 定义 Associated Type: {} = {:?}",
                                                    trait_name, assoc_type_name, type_key
                                                );
                                            }
                                        }
                                    } else {
                                        // Trait 约束 TODO: 目前仅支持单一 Trait
                                        for bound in bounds.iter() {
                                            if let GenericBound::TraitBound { trait_, .. } = bound {
                                                let trait_id = trait_.id;
                                                if let Some(trait_node) =
                                                    self.type_cache.get_by_id(&trait_id)
                                                {
                                                    self.graph.add_type_relation(
                                                        assoc_type_node,
                                                        trait_node,
                                                        EdgeMode::Alias,
                                                        Some(format!("{} =", assoc_type_name)),
                                                    );
                                                }
                                            }
                                        }
                                        debug!(
                                            "Trait {} 定义 Associated Type: {} Trait Bound",
                                            trait_name, assoc_type_name
                                        );
                                    }
                                }
                            }
                        }

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
        use rustdoc_types::Path;
        use std::collections::HashMap;

        // 收集所有方法的泛型：泛型名 -> (约束 trait Paths, 出现次数)
        let mut generic_info: HashMap<String, (Vec<Path>, usize)> = HashMap::new();

        for &method_id in method_ids {
            if let Some(method_item) = self.parsed.crate_data.index.get(&method_id) {
                if let ItemEnum::Function(func) = &method_item.inner {
                    for param in &func.generics.params {
                        if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                            let trait_bounds: Vec<Path> = bounds
                                .iter()
                                .filter_map(|bound| {
                                    if let GenericBound::TraitBound { trait_, .. } = bound {
                                        Some(trait_.clone())
                                    } else {
                                        None
                                    }
                                })
                                .collect();

                            generic_info
                                .entry(param.name.clone())
                                .and_modify(|(bounds_vec, count)| {
                                    // 检查约束是否相同（比较 trait ID）
                                    if bounds_vec.len() == trait_bounds.len()
                                        && bounds_vec
                                            .iter()
                                            .all(|b| trait_bounds.iter().any(|t| t.id == b.id))
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

                // 注册到 type_cache，使用 trait_name:T 作为 key
                self.type_cache
                    .insert_generic(normalized_name.clone(), None, generic_node_index);

                // 创建 Require 边
                for trait_path in &trait_bounds {
                    let trait_id = trait_path.id;
                    let trait_full_name = Self::get_trait_full_name(trait_path);
                    let (has_concrete_args, _) =
                        Self::check_trait_args_for_concrete_type(&trait_path.args);

                    // 获取或创建 Trait 节点
                    let trait_node = if has_concrete_args {
                        // 带具体类型参数的 Trait
                        let type_key = TypeKey::TraitWithArgs {
                            trait_id,
                            args_repr: trait_full_name.clone(),
                        };
                        if let Some(node) = self.type_cache.get_node(&type_key) {
                            node
                        } else {
                            let node = self.graph.add_type_node(&trait_full_name);
                            self.graph.node_types.insert(node, NodeType::Trait);
                            self.type_cache.insert_node(type_key, node);
                            debug!(
                                "创建带参数的外部 Trait 节点: {} (id: {}, 用于归一化泛型约束)",
                                trait_full_name, trait_id.0
                            );
                            node
                        }
                    } else if let Some(node) = self.type_cache.get_by_id(&trait_id) {
                        node
                    } else {
                        // 外部 Trait，创建节点
                        let trait_name = trait_path
                            .path
                            .split("::")
                            .last()
                            .unwrap_or(&trait_path.path);
                        let node = self.graph.add_type_node(trait_name);
                        self.graph.node_types.insert(node, NodeType::Trait);
                        self.type_cache.insert_type_by_id(trait_id, node);

                        debug!(
                            "创建外部 Trait 节点: {} (id: {}, 用于归一化泛型约束)",
                            trait_name, trait_id.0
                        );
                        node
                    };

                    // Trait -> 泛型（Petri 网语义：token 从 Trait 流向泛型）
                    self.graph.add_type_relation(
                        trait_node,
                        generic_node_index,
                        EdgeMode::Require,
                        Some(format!("required by {}", generic_name)),
                    );
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
        let stats = self.type_cache.stats();
        debug!(
            "开始处理 Constants 和 Statics\n\
            - Constants: {} 个\n\
            - Statics: {} 个\n\
            - 当前 type_cache 中有 {} 个类型节点",
            self.parsed.info.constants.len(),
            self.parsed.info.statics.len(),
            stats.types
        );

        let crate_name = self.get_crate_name();

        // 处理 Constants
        for &constant_id in &self.parsed.info.constants {
            if let Some(item) = self.parsed.crate_data.index.get(&constant_id) {
                if let ItemEnum::Constant { type_, const_ } = &item.inner {
                    let constant_name = item.name.as_deref().unwrap_or("unknown");

                    // 创建 Constant 节点
                    let constant_node = self.graph.add_type_node(constant_name);
                    self.graph
                        .node_types
                        .insert(constant_node, NodeType::Constant);
                    self.type_cache
                        .insert_type_by_id(constant_id, constant_node);

                    // 创建 ConstantInfo
                    let constant_info = ConstantInfo {
                        path: PathInfo::new(
                            &format!("{}::{}", crate_name, constant_name),
                            constant_name,
                        ),
                        type_node: None, // 后续填充
                        type_str: format!("{:?}", type_),
                        init_value: const_.value.clone(),
                    };
                    self.graph
                        .node_infos
                        .insert(constant_node, NodeInfo::Constant(constant_info));

                    // 解析 Constant 的类型并创建 Instance 边
                    match type_ {
                        Type::ResolvedPath(path) => {
                            let type_name = self
                                .parsed
                                .crate_data
                                .index
                                .get(&path.id)
                                .and_then(|item| item.name.as_deref())
                                .unwrap_or("unknown");

                            debug!(
                                "处理 Constant: {} -> 类型 {} (id: {})",
                                constant_name, type_name, path.id.0
                            );

                            // 查找类型节点
                            if let Some(type_node) = self.type_cache.get_by_id(&path.id) {
                                debug!(
                                    "✓ 从 type_cache 找到类型节点: {} (id: {}) -> NodeIndex({:?})",
                                    type_name, path.id.0, type_node
                                );

                                // 创建 Instance 边：Constant -> Type
                                self.graph.add_type_relation(
                                    constant_node,
                                    type_node,
                                    EdgeMode::Instance,
                                    Some("instance of".to_string()),
                                );

                                // 更新 ConstantInfo 的 type_node
                                if let Some(NodeInfo::Constant(info)) =
                                    self.graph.node_infos.get_mut(&constant_node)
                                {
                                    info.type_node = Some(type_node);
                                }

                                debug!(
                                    "创建 Instance 边: {} (NodeIndex({:?})) -> {} (NodeIndex({:?}))",
                                    constant_name, constant_node, type_name, type_node
                                );
                            } else {
                                error!(
                                    "✗ 未找到类型节点: {} (id: {}) 在 type_cache 中",
                                    type_name, path.id.0
                                );
                                error!(
                                    "  当前 type_cache 中的类型 ID: {:?}",
                                    self.type_cache.type_ids().collect::<Vec<_>>()
                                );

                                // 尝试通过 TypeCache 的 TypeKey 查找
                                use super::type_cache::TypeContext;
                                let context = TypeContext::new();
                                if let Some(type_key) =
                                    self.type_cache.create_type_key(type_, &context)
                                {
                                    if let Some(cached_node) = self.type_cache.get_node(&type_key) {
                                        error!(
                                            "⚠️  从 TypeCache 通过 TypeKey 找到类型节点: {} (id: {}) -> NodeIndex({:?})\n\
                                            ⚠️  但该节点不在 id_to_node 中！这可能导致重复节点。\n\
                                            ⚠️  正在更新 type_cache 以避免后续问题。",
                                            type_name, path.id.0, cached_node
                                        );

                                        // 检查是否已经有其他节点映射到这个 ID
                                        if let Some(existing_node) =
                                            self.type_cache.get_by_id(&path.id)
                                        {
                                            error!(
                                                "❌ 冲突！类型 {} (id: {}) 已经映射到 NodeIndex({:?})，\n\
                                                但 TypeCache 返回的是 NodeIndex({:?})！\n\
                                                这会导致重复节点。",
                                                type_name, path.id.0, existing_node, cached_node
                                            );
                                        } else {
                                            // 更新 type_cache
                                            self.type_cache.insert_type_by_id(path.id, cached_node);
                                            debug!(
                                                "✓ 已更新 type_cache: id {} -> node {:?}",
                                                path.id.0, cached_node
                                            );
                                        }

                                        self.graph.add_type_relation(
                                            constant_node,
                                            cached_node,
                                            EdgeMode::Instance,
                                            Some("instance of".to_string()),
                                        );
                                    } else {
                                        error!("✗ TypeCache 中也没有找到类型节点: {}", type_name);
                                    }
                                } else {
                                    error!("✗ 无法创建 TypeKey 用于类型: {}", type_name);
                                }
                            }
                        }
                        _ => {
                            error!(
                                "Constant {} 的类型不是 ResolvedPath: {:?}",
                                constant_name, type_
                            );
                            continue;
                        }
                    }
                }
            }
        }

        // 处理 Statics
        for &static_id in &self.parsed.info.statics {
            if let Some(item) = self.parsed.crate_data.index.get(&static_id) {
                if let ItemEnum::Static(static_data) = &item.inner {
                    let static_name = item.name.as_deref().unwrap_or("unknown");

                    // 创建 Static 节点
                    let static_node = self.graph.add_type_node(static_name);
                    self.graph.node_types.insert(static_node, NodeType::Static);
                    self.type_cache.insert_type_by_id(static_id, static_node);

                    // 创建 StaticInfo
                    let static_info = StaticInfo {
                        path: PathInfo::new(
                            &format!("{}::{}", crate_name, static_name),
                            static_name,
                        ),
                        type_node: None, // 后续填充
                        type_str: format!("{:?}", static_data.type_),
                        is_mutable: static_data.is_mutable,
                        init_value: Some(static_data.expr.clone()),
                    };
                    self.graph
                        .node_infos
                        .insert(static_node, NodeInfo::Static(static_info));

                    match &static_data.type_ {
                        Type::ResolvedPath(path) => {
                            if let Some(type_node) = self.type_cache.get_by_id(&path.id) {
                                self.graph.add_type_relation(
                                    static_node,
                                    type_node,
                                    EdgeMode::Instance,
                                    Some("instance of".to_string()),
                                );

                                // 更新 StaticInfo 的 type_node
                                if let Some(NodeInfo::Static(info)) =
                                    self.graph.node_infos.get_mut(&static_node)
                                {
                                    info.type_node = Some(type_node);
                                }
                            } else {
                                error!("Static {} 的类型节点未找到", static_name);
                            }
                        }
                        _ => {
                            error!("Static {} 的类型不是 ResolvedPath", static_name);
                        }
                    }
                }
            }
        }

        // 总结：检查是否有重复的类型节点
        let stats = self.type_cache.stats();
        debug!("=== Constants 和 Statics 处理完成 ===");
        debug!("最终 type_cache 中有 {} 个类型节点", stats.types);

        // 检查是否有重复的节点（同一个 ID 映射到不同节点）
        let mut id_to_nodes: HashMap<Id, Vec<NodeIndex>> = HashMap::new();
        for id in self.type_cache.type_ids() {
            if let Some(node) = self.type_cache.get_by_id(id) {
                id_to_nodes.entry(*id).or_insert_with(Vec::new).push(node);
            }
        }

        for (id, nodes) in id_to_nodes {
            if nodes.len() > 1 {
                let type_name = self.get_name(&id).unwrap_or("unknown");
                error!(
                    "❌ 发现重复节点！类型 {} (id: {}) 映射到 {} 个不同节点: {:?}",
                    type_name,
                    id.0,
                    nodes.len(),
                    nodes
                );
            }
        }
    }

    fn get_name(&self, id: &Id) -> Option<&str> {
        self.parsed.crate_data.index.get(id)?.name.as_deref()
    }

    /// 获取 crate 名称
    fn get_crate_name(&self) -> String {
        self.parsed
            .crate_data
            .index
            .get(&self.parsed.crate_data.root)
            .and_then(|item| item.name.as_deref())
            .unwrap_or("crate")
            .to_string()
    }

    fn is_blacklisted_trait(&self, name: &str) -> bool {
        use crate::support_types::TRAIT_BLACKLIST;
        TRAIT_BLACKLIST.contains(&name)
    }

    /// 后处理：为 Primitive 类型添加到 Trait 的 Implements 边
    fn postprocess_primitive_trait_edges(&mut self) {
        use super::structure::TypeRelation;

        // 收集所有 Primitive 节点及其 default_traits
        let primitives: Vec<(NodeIndex, Vec<String>)> = self
            .graph
            .node_infos
            .iter()
            .filter_map(|(&idx, info)| {
                if let NodeInfo::Primitive(prim_info) = info {
                    Some((idx, prim_info.default_traits.clone()))
                } else {
                    None
                }
            })
            .collect();

        // 收集所有 Trait 节点的名称到索引映射
        let trait_name_to_node: HashMap<String, NodeIndex> = self
            .graph
            .node_infos
            .iter()
            .filter_map(|(&idx, info)| {
                if let NodeInfo::Trait(trait_info) = info {
                    Some((trait_info.path.name.clone(), idx))
                } else {
                    None
                }
            })
            .collect();

        // 为每个 Primitive 添加到对应 Trait 的 Implements 边
        let mut trait_nodes_updates: Vec<(NodeIndex, Vec<NodeIndex>)> = Vec::new();

        for (prim_idx, default_traits) in primitives {
            let mut found_trait_nodes = Vec::new();

            for trait_name in &default_traits {
                if let Some(&trait_idx) = trait_name_to_node.get(trait_name) {
                    // 添加 Implements 边：Primitive -> Trait
                    self.graph.type_graph.add_edge(
                        prim_idx,
                        trait_idx,
                        TypeRelation {
                            mode: EdgeMode::Implements,
                            label: None,
                        },
                    );
                    found_trait_nodes.push(trait_idx);
                    debug!(
                        "✓ 添加 Primitive->Trait Implements 边: {:?} -> {:?} ({})",
                        prim_idx, trait_idx, trait_name
                    );
                }
            }

            if !found_trait_nodes.is_empty() {
                trait_nodes_updates.push((prim_idx, found_trait_nodes));
            }
        }

        // 更新 PrimitiveInfo 中的 trait_nodes
        for (prim_idx, trait_nodes) in trait_nodes_updates {
            if let Some(NodeInfo::Primitive(prim_info)) = self.graph.node_infos.get_mut(&prim_idx) {
                prim_info.trait_nodes = trait_nodes;
            }
        }
    }

    /// 后处理：检查 Generic 约束，为满足约束的 Primitive 添加 Instance 边
    fn postprocess_generic_constraints(&mut self) {
        use super::structure::TypeRelation;

        // 收集所有 Generic 节点及其 bounds（NodeIndex -> 对应的 Trait 名称）
        let generics: Vec<(NodeIndex, Vec<String>)> = self
            .graph
            .node_infos
            .iter()
            .filter_map(|(&idx, info)| {
                if let NodeInfo::Generic(gen_info) = info {
                    // 将 bounds 的 NodeIndex 转换为 Trait 名称
                    let bound_names: Vec<String> = gen_info
                        .bounds
                        .iter()
                        .filter_map(|&bound_idx| {
                            // 从图中获取节点名称
                            self.graph.type_graph.node_weight(bound_idx).cloned()
                        })
                        .collect();
                    Some((idx, bound_names))
                } else {
                    None
                }
            })
            .collect();

        // 收集所有 Primitive 节点及其实现的 Trait
        let primitives: Vec<(NodeIndex, String, Vec<String>)> = self
            .graph
            .node_infos
            .iter()
            .filter_map(|(&idx, info)| {
                if let NodeInfo::Primitive(prim_info) = info {
                    Some((
                        idx,
                        prim_info.name.clone(),
                        prim_info.default_traits.clone(),
                    ))
                } else {
                    None
                }
            })
            .collect();

        // 对于每个 Generic，检查哪些 Primitive 满足其所有 bounds
        for (gen_idx, bounds) in generics {
            if bounds.is_empty() {
                // 无约束的泛型，所有 Primitive 都可以实例化
                for (prim_idx, prim_name, _) in &primitives {
                    self.graph.type_graph.add_edge(
                        *prim_idx,
                        gen_idx,
                        TypeRelation {
                            mode: EdgeMode::Instance,
                            label: Some(format!("{} can instantiate", prim_name)),
                        },
                    );
                    debug!(
                        "✓ 添加 Primitive->Generic Instance 边 (无约束): {:?} -> {:?}",
                        prim_idx, gen_idx
                    );
                }
            } else {
                // 有约束的泛型，检查 Primitive 是否满足所有 bounds
                for (prim_idx, prim_name, prim_traits) in &primitives {
                    let satisfies_all = bounds.iter().all(|bound| {
                        // 简单匹配：检查 Primitive 的 default_traits 是否包含 bound
                        // 注意：bound 可能是完整路径如 "std::fmt::Debug"，需要提取最后一段
                        let bound_name = bound.split("::").last().unwrap_or(bound);
                        prim_traits.iter().any(|t| t == bound_name)
                    });

                    if satisfies_all {
                        self.graph.type_graph.add_edge(
                            *prim_idx,
                            gen_idx,
                            TypeRelation {
                                mode: EdgeMode::Instance,
                                label: Some(format!("{} satisfies {:?}", prim_name, bounds)),
                            },
                        );
                        debug!(
                            "✓ 添加 Primitive->Generic Instance 边: {:?} ({}) -> {:?} (bounds: {:?})",
                            prim_idx, prim_name, gen_idx, bounds
                        );
                    }
                }
            }
        }
    }

    /// 检查 Trait 的泛型参数是否包含具体类型实例
    ///
    /// 返回 (has_concrete_type, concrete_type_repr)
    /// - 如果参数是具体类型（如 `[u8]`），返回 (true, Some("[u8]"))
    /// - 如果参数只是泛型或为空，返回 (false, None)
    pub(crate) fn check_trait_args_for_concrete_type(
        args: &Option<Box<rustdoc_types::GenericArgs>>,
    ) -> (bool, Option<String>) {
        use rustdoc_types::{GenericArg, GenericArgs};

        if let Some(args) = args {
            match args.as_ref() {
                GenericArgs::AngleBracketed { args, .. } => {
                    for arg in args {
                        if let GenericArg::Type(ty) = arg {
                            // 检查是否是具体类型（非泛型）
                            if let Some(repr) = Self::get_concrete_type_repr(ty) {
                                return (true, Some(repr));
                            }
                        }
                    }
                }
                GenericArgs::Parenthesized { inputs, output } => {
                    // Fn(A, B) -> C 形式，检查输入和输出类型
                    let mut parts = Vec::new();
                    for input in inputs {
                        if let Some(repr) = Self::get_concrete_type_repr(input) {
                            parts.push(repr);
                        }
                    }
                    if let Some(out) = output {
                        if let Some(repr) = Self::get_concrete_type_repr(out) {
                            parts.push(format!("-> {}", repr));
                        }
                    }
                    if !parts.is_empty() {
                        return (true, Some(format!("({})", parts.join(", "))));
                    }
                }
                GenericArgs::ReturnTypeNotation => {}
            }
        }
        (false, None)
    }

    /// 获取具体类型的字符串表示
    /// 如果是泛型参数，返回 None
    fn get_concrete_type_repr(ty: &Type) -> Option<String> {
        match ty {
            Type::Primitive(name) => Some(name.clone()),
            Type::Slice(inner) => Self::get_concrete_type_repr(inner).map(|s| format!("[{}]", s)),
            Type::Array { type_, len } => {
                Self::get_concrete_type_repr(type_).map(|s| format!("[{}; {}]", s, len))
            }
            Type::Tuple(types) => {
                let parts: Vec<_> = types
                    .iter()
                    .filter_map(|t| Self::get_concrete_type_repr(t))
                    .collect();
                if parts.len() == types.len() && !parts.is_empty() {
                    Some(format!("({})", parts.join(", ")))
                } else if types.is_empty() {
                    Some("()".to_string())
                } else {
                    None
                }
            }
            Type::RawPointer { is_mutable, type_ } => {
                Self::get_concrete_type_repr(type_).map(|s| {
                    if *is_mutable {
                        format!("*mut {}", s)
                    } else {
                        format!("*const {}", s)
                    }
                })
            }
            Type::BorrowedRef {
                lifetime: _,
                is_mutable,
                type_,
            } => Self::get_concrete_type_repr(type_).map(|s| {
                if *is_mutable {
                    format!("&mut {}", s)
                } else {
                    format!("&{}", s)
                }
            }),
            Type::ResolvedPath(path) => {
                // 对于已解析的路径，使用路径名
                let name = path.path.split("::").last().unwrap_or(&path.path);
                // 检查是否有具体的泛型参数
                if let Some(args) = &path.args {
                    let (has_concrete, repr) =
                        Self::check_trait_args_for_concrete_type(&Some(args.clone()));
                    if has_concrete {
                        return Some(format!("{}<{}>", name, repr.unwrap_or_default()));
                    }
                }
                Some(name.to_string())
            }
            // 泛型参数不是具体类型
            Type::Generic(_) => None,
            // 其他类型暂不处理
            _ => None,
        }
    }

    /// 为带有具体类型参数的 Trait 生成完整名称
    /// 例如：AsRef<[u8]> 而不是只是 AsRef
    pub(crate) fn get_trait_full_name(trait_path: &rustdoc_types::Path) -> String {
        let base_name = trait_path
            .path
            .split("::")
            .last()
            .unwrap_or(&trait_path.path);
        let (has_concrete, repr) = Self::check_trait_args_for_concrete_type(&trait_path.args);
        if has_concrete {
            format!("{}<{}>", base_name, repr.unwrap_or_default())
        } else {
            base_name.to_string()
        }
    }
}
