//! API 依赖图：分析类型依赖、Trait 实例化、字段访问
//!
//! 核心功能：
//! 1. 构建类型依赖图（哪些 API 产生/消费哪些类型）
//! 2. 提取 Trait 实例化（Default, Clone, From, Into 等）
//! 3. 分析 public 字段（struct.field 可以作为某类型的来源）
//! 4. 生成 DOT 格式可视化

use anyhow::Result;
use indexmap::{IndexMap, IndexSet};
use rustdoc_types::{Crate, Id, Item, ItemEnum, Type, Visibility};
use std::collections::HashMap;

use crate::api_extract::ApiSignature;
use crate::model::TypeKey;
use crate::type_norm::TypeContext;

/// API 图节点
#[derive(Debug, Clone)]
pub struct ApiNode {
    /// API 索引
    pub index: usize,
    /// API 签名
    pub api: ApiSignature,
    /// 输入类型（参数）- 保留完整信息
    pub inputs: Vec<(TypeKey, crate::model::Capability)>,
    /// 输出类型（返回值）- 保留完整信息
    pub outputs: Vec<(TypeKey, crate::model::Capability)>,
    /// 是否是入口 API（无参数或只需 primitive）
    pub is_entry: bool,
    /// 来源（Normal API / Trait Impl / Field Access）
    pub source: ApiSource,
}

/// API 来源
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiSource {
    /// 普通函数/方法
    Normal,
    /// Trait 实现（例如 Default::default）
    TraitImpl { trait_name: String },
    /// 字段访问（例如 Counter.value）
    FieldAccess { struct_name: String, field_name: String },
}

/// 类型依赖边
#[derive(Debug, Clone)]
pub struct TypeEdge {
    /// 从哪个 API
    pub from_api: usize,
    /// 到哪个 API
    pub to_api: usize,
    /// 通过什么类型
    pub type_key: TypeKey,
    /// Capability: Own / Shr / Mut
    pub capability: crate::model::Capability,
}

/// API 依赖图
#[derive(Debug)]
pub struct ApiGraph {
    /// 所有节点
    pub nodes: Vec<ApiNode>,
    /// 类型依赖边
    pub edges: Vec<TypeEdge>,
    /// 类型 -> 生产者 APIs
    pub producers: IndexMap<TypeKey, Vec<usize>>,
    /// 类型 -> 消费者 APIs
    pub consumers: IndexMap<TypeKey, Vec<usize>>,
    /// Trait 实现映射
    pub trait_impls: HashMap<String, Vec<TraitImplInfo>>,
    /// Public 字段映射
    pub public_fields: HashMap<TypeKey, Vec<FieldInfo>>,
}

/// Trait 实现信息
#[derive(Debug, Clone)]
pub struct TraitImplInfo {
    pub trait_name: String,
    pub self_type: TypeKey,
    pub method_name: String,
    pub inputs: Vec<TypeKey>,
    pub output: Option<TypeKey>,
}

/// 字段信息
#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub field_name: String,
    pub field_type: TypeKey,
    pub parent_type: TypeKey,
}

impl ApiGraph {
    /// 从 APIs 和 Crate 构建依赖图
    pub fn build(
        apis: &[ApiSignature],
        krate: &Crate,
        type_ctx: &TypeContext,
    ) -> Result<Self> {
        let mut graph = ApiGraph {
            nodes: Vec::new(),
            edges: Vec::new(),
            producers: IndexMap::new(),
            consumers: IndexMap::new(),
            trait_impls: HashMap::new(),
            public_fields: HashMap::new(),
        };

        // 1. 提取 Trait 实现
        graph.extract_trait_impls(krate, type_ctx)?;

        // 2. 提取 public 字段
        graph.extract_public_fields(krate, type_ctx)?;

        // 3. 添加普通 API 节点
        for (idx, api) in apis.iter().enumerate() {
            graph.add_api_node(idx, api.clone(), ApiSource::Normal);
        }

        // 4. 添加 Trait impl 节点
        let mut trait_api_idx = apis.len();
        let trait_impls_clone = graph.trait_impls.clone();
        for impls in trait_impls_clone.values() {
            for impl_info in impls {
                let api = Self::trait_impl_to_api_static(impl_info, trait_api_idx);
                graph.add_api_node(trait_api_idx, api, ApiSource::TraitImpl {
                    trait_name: impl_info.trait_name.clone(),
                });
                trait_api_idx += 1;
            }
        }

        // 5. 添加字段访问节点
        let mut field_api_idx = trait_api_idx;
        let public_fields_clone = graph.public_fields.clone();
        for (parent_ty, fields) in &public_fields_clone {
            for field in fields {
                let api = Self::field_access_to_api_static(field, field_api_idx);
                graph.add_api_node(field_api_idx, api, ApiSource::FieldAccess {
                    struct_name: parent_ty.clone(),
                    field_name: field.field_name.clone(),
                });
                field_api_idx += 1;
            }
        }

        // 6. 构建边
        graph.build_edges();

        Ok(graph)
    }

    /// 添加 API 节点
    fn add_api_node(&mut self, index: usize, api: ApiSignature, source: ApiSource) {
        use crate::api_extract::{ParamMode, ReturnMode};
        use crate::model::Capability;
        
        // 提取输入参数及其 Capability
        let inputs: Vec<(TypeKey, Capability)> = api
            .all_params()
            .iter()
            .map(|p| {
                let cap = match p {
                    ParamMode::ByValue(_, _) => Capability::Own,
                    ParamMode::SharedRef(_) => Capability::Shr,
                    ParamMode::MutRef(_) => Capability::Mut,
                };
                (p.type_key().clone(), cap)
            })
            .collect();

        // 提取输出及其 Capability
        let outputs: Vec<(TypeKey, Capability)> = match &api.return_mode {
            ReturnMode::OwnedValue(ty, _) => vec![(ty.clone(), Capability::Own)],
            ReturnMode::SharedRef(ty) => vec![(ty.clone(), Capability::Shr)],
            ReturnMode::MutRef(ty) => vec![(ty.clone(), Capability::Mut)],
            ReturnMode::Unit => vec![],
        };

        let is_entry = inputs.is_empty()
            || inputs.iter().all(|(t, _)| Self::is_primitive(t) || Self::is_simple_literal(t));

        // 记录生产者和消费者
        for (ty, _) in &outputs {
            self.producers
                .entry(ty.clone())
                .or_insert_with(Vec::new)
                .push(index);
        }
        for (ty, _) in &inputs {
            self.consumers
                .entry(ty.clone())
                .or_insert_with(Vec::new)
                .push(index);
        }

        self.nodes.push(ApiNode {
            index,
            api,
            inputs,
            outputs,
            is_entry,
            source,
        });
    }

    /// 构建类型依赖边（考虑引用兼容性）
    fn build_edges(&mut self) {
        use crate::model::Capability;
        
        for producer_node in &self.nodes {
            for (output_ty, output_cap) in &producer_node.outputs {
                // 遍历所有消费者，检查类型和 Capability 兼容性
                for consumer_node in &self.nodes {
                    for (input_ty, input_cap) in &consumer_node.inputs {
                        // 检查类型是否匹配
                        if output_ty != input_ty {
                            continue;
                        }
                        
                        // 检查 Capability 是否兼容
                        let compatible = match (output_cap, input_cap) {
                            // Own 可以传给任何类型（通过临时借用）
                            (Capability::Own, _) => true,
                            // Shr 只能传给 Shr（共享引用可以复制）
                            (Capability::Shr, Capability::Shr) => true,
                            // Mut 可以传给 Shr 或 Mut
                            (Capability::Mut, Capability::Shr) => true,
                            (Capability::Mut, Capability::Mut) => true,
                            // 其他情况不兼容
                            _ => false,
                        };
                        
                        if compatible {
                            self.edges.push(TypeEdge {
                                from_api: producer_node.index,
                                to_api: consumer_node.index,
                                type_key: output_ty.clone(),
                                capability: *input_cap,  // 边的 capability 是消费者要求的
                            });
                        }
                    }
                }
            }
        }
    }

    /// 提取 Trait 实现
    fn extract_trait_impls(&mut self, krate: &Crate, type_ctx: &TypeContext) -> Result<()> {
        for (id, item) in &krate.index {
            if let ItemEnum::Impl(impl_) = &item.inner {
                let self_type = Self::extract_self_type(&impl_.for_, type_ctx);
                
                // 检查是否是 trait impl
                if let Some(trait_ref) = &impl_.trait_ {
                    // 通过 trait id 查找 trait 名
                    let trait_name = if let Some(trait_item) = krate.index.get(&trait_ref.id) {
                        trait_item.name.clone().unwrap_or_else(|| "Trait".to_string())
                    } else {
                        "Trait".to_string()
                    };

                    // 提取常见 trait 的方法
                    match trait_name.as_str() {
                        "Default" => {
                            self.trait_impls
                                .entry("Default".to_string())
                                .or_insert_with(Vec::new)
                                .push(TraitImplInfo {
                                    trait_name: "Default".to_string(),
                                    self_type: self_type.clone(),
                                    method_name: "default".to_string(),
                                    inputs: vec![],
                                    output: Some(self_type),
                                });
                        }
                        "Clone" => {
                            self.trait_impls
                                .entry("Clone".to_string())
                                .or_insert_with(Vec::new)
                                .push(TraitImplInfo {
                                    trait_name: "Clone".to_string(),
                                    self_type: self_type.clone(),
                                    method_name: "clone".to_string(),
                                    inputs: vec![self_type.clone()], // &self
                                    output: Some(self_type),
                                });
                        }
                        _ => {
                            // 其他 trait 暂时忽略
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// 提取 public 字段
    fn extract_public_fields(&mut self, krate: &Crate, type_ctx: &TypeContext) -> Result<()> {
        for (id, item) in &krate.index {
            match &item.inner {
                ItemEnum::Struct(struct_) => {
                    let struct_type = type_ctx
                        .id_to_path
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());

                    // 提取 public 字段
                    if let rustdoc_types::StructKind::Plain { fields, .. } = &struct_.kind {
                        for field_id in fields {
                            if let Some(field_item) = krate.index.get(field_id) {
                                // 检查可见性
                                if matches!(field_item.visibility, Visibility::Public) {
                                    if let ItemEnum::StructField(ty) = &field_item.inner {
                                        if let Ok((field_ty, _, _)) =
                                            type_ctx.normalize_type(ty)
                                        {
                                            let field_name = field_item
                                                .name
                                                .clone()
                                                .unwrap_or_else(|| "unnamed".to_string());

                                            self.public_fields
                                                .entry(struct_type.clone())
                                                .or_insert_with(Vec::new)
                                                .push(FieldInfo {
                                                    field_name,
                                                    field_type: field_ty,
                                                    parent_type: struct_type.clone(),
                                                });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Trait impl 转换为 API 签名
    fn trait_impl_to_api_static(impl_info: &TraitImplInfo, index: usize) -> ApiSignature {
        use crate::api_extract::{ParamMode, ReturnMode};

        let full_path = format!(
            "{}::{}",
            impl_info.self_type,
            impl_info.method_name
        );

        let params: Vec<ParamMode> = impl_info
            .inputs
            .iter()
            .map(|ty| ParamMode::SharedRef(ty.clone()))
            .collect();

        let return_mode = if let Some(ref out_ty) = impl_info.output {
            ReturnMode::OwnedValue(out_ty.clone(), false)
        } else {
            ReturnMode::Unit
        };

        ApiSignature {
            full_path,
            is_method: !impl_info.inputs.is_empty(),
            self_mode: impl_info.inputs.first().map(|ty| ParamMode::SharedRef(ty.clone())),
            params: vec![],
            return_mode,
            is_unsafe: false,
        }
    }

    /// 字段访问转换为 API 签名
    fn field_access_to_api_static(field: &FieldInfo, index: usize) -> ApiSignature {
        use crate::api_extract::{ParamMode, ReturnMode};

        let full_path = format!("{}.{}", field.parent_type, field.field_name);

        ApiSignature {
            full_path,
            is_method: false,
            self_mode: Some(ParamMode::SharedRef(field.parent_type.clone())),
            params: vec![],
            return_mode: ReturnMode::OwnedValue(field.field_type.clone(), true), // 字段通常是 Copy
            is_unsafe: false,
        }
    }

    /// 生成 DOT 格式
    pub fn to_dot(&self) -> String {
        let mut dot = String::new();
        dot.push_str("digraph ApiGraph {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  node [shape=box, style=rounded];\n\n");

        // 节点
        for node in &self.nodes {
            let color = match &node.source {
                ApiSource::Normal => "lightblue",
                ApiSource::TraitImpl { .. } => "lightgreen",
                ApiSource::FieldAccess { .. } => "lightyellow",
            };

            let shape = if node.is_entry { "doubleoctagon" } else { "box" };

            let label = self.format_node_label(node);
            
            dot.push_str(&format!(
                "  n{} [label=\"{}\", fillcolor={}, style=\"rounded,filled\", shape={}];\n",
                node.index, label, color, shape
            ));
        }

        dot.push_str("\n");

        // 边（显示类型和 Capability）
        for edge in &self.edges {
            let color = Self::type_to_color(&edge.type_key);
            let cap_label = Self::capability_to_label(&edge.capability);
            let type_label = Self::simplify_type(&edge.type_key);
            
            // 标签格式：Type (capability)
            let label = if edge.capability == crate::model::Capability::Own {
                type_label  // Own 不需要特殊标记
            } else {
                format!("{} ({})", type_label, cap_label)
            };
            
            dot.push_str(&format!(
                "  n{} -> n{} [label=\"{}\", color=\"{}\"];\n",
                edge.from_api,
                edge.to_api,
                label,
                color
            ));
        }

        dot.push_str("}\n");
        dot
    }
    
    /// Capability 转标签
    fn capability_to_label(cap: &crate::model::Capability) -> &'static str {
        match cap {
            crate::model::Capability::Own => "own",
            crate::model::Capability::Shr => "&",
            crate::model::Capability::Mut => "&mut",
        }
    }

    /// 格式化节点标签
    fn format_node_label(&self, node: &ApiNode) -> String {
        let name = node.api.full_path.split("::").last().unwrap_or(&node.api.full_path);
        
        let source_marker = match &node.source {
            ApiSource::Normal => "",
            ApiSource::TraitImpl { trait_name } => &format!(" [{}]", trait_name),
            ApiSource::FieldAccess { .. } => " [field]",
        };

        // 格式化输入参数，带 Capability 标注
        let inputs = node
            .inputs
            .iter()
            .map(|(ty, cap)| {
                let ty_str = Self::simplify_type(ty);
                match cap {
                    crate::model::Capability::Own => ty_str,
                    crate::model::Capability::Shr => format!("&{}", ty_str),
                    crate::model::Capability::Mut => format!("&mut {}", ty_str),
                }
            })
            .collect::<Vec<_>>()
            .join(", ");

        // 格式化输出，带 Capability 标注
        let outputs = node
            .outputs
            .iter()
            .map(|(ty, cap)| {
                let ty_str = Self::simplify_type(ty);
                match cap {
                    crate::model::Capability::Own => ty_str,
                    crate::model::Capability::Shr => format!("&{}", ty_str),
                    crate::model::Capability::Mut => format!("&mut {}", ty_str),
                }
            })
            .collect::<Vec<_>>()
            .join(", ");

        if outputs.is_empty() {
            format!("{}{}\\n({})", name, source_marker, inputs)
        } else {
            format!("{}{}\\n({}) → {}", name, source_marker, inputs, outputs)
        }
    }

    /// 简化类型名
    fn simplify_type(ty: &str) -> String {
        ty.split("::").last().unwrap_or(ty).to_string()
    }

    /// 类型到颜色映射
    fn type_to_color(ty: &str) -> &'static str {
        if Self::is_primitive(ty) {
            "gray"
        } else {
            "black"
        }
    }

    /// 判断是否是 primitive 类型
    fn is_primitive(ty: &str) -> bool {
        matches!(
            ty,
            "bool" | "i8" | "i16" | "i32" | "i64" | "i128" | "isize"
                | "u8" | "u16" | "u32" | "u64" | "u128" | "usize"
                | "f32" | "f64" | "()" | "char" | "str"
        )
    }

    /// 判断是否是简单字面量类型
    fn is_simple_literal(ty: &str) -> bool {
        Self::is_primitive(ty)
    }

    /// 提取 trait 名称
    fn extract_trait_name(trait_ref: &rustdoc_types::Path) -> String {
        // Path 结构可能没有直接的 name 字段，尝试从 id 获取
        "Trait".to_string() // 简化处理，后续可以通过 id 查询
    }

    /// 提取 self 类型
    fn extract_self_type(ty: &Type, type_ctx: &TypeContext) -> TypeKey {
        type_ctx
            .normalize_type(ty)
            .map(|(key, _, _)| key)
            .unwrap_or_else(|_| "unknown".to_string())
    }
}
