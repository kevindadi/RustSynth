/// CP-Net Builder：从 IrGraph 构建 Petri Net
///
/// 核心转换逻辑：
/// 1. 为具体类型创建 Place
/// 2. 为每个 Trait 创建 Trait Hub Place
/// 3. 为 Trait 实现创建 ImplCast Transition
/// 4. 为操作创建 Operation Transition（处理泛型约束）
use super::structure::{Arc, ArcType, CpPetriNet, Place, Transition, TransitionKind};
use crate::ir_graph::structure::{EdgeMode, IrGraph, OpKind, OpNode, TypeNode};
use rustdoc_types::Id;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

/// Petri Net 构建器
pub struct CpNetBuilder<'a> {
    /// 输入的 IR Graph
    ir: &'a IrGraph,

    /// 输出的 Petri Net
    net: CpPetriNet,

    /// TypeNode 到 Place ID 的映射
    type_to_place: HashMap<TypeNode, String>,

    /// Trait ID 到 Place ID 的映射（Trait Hub）
    trait_to_place: HashMap<Id, String>,

    /// Trait ID 到 Trait Name 的映射
    trait_id_to_name: HashMap<Id, String>,

    /// Copy Trait 的 ID（用于判断类型是否实现了 Copy）
    copy_trait_id: Option<Id>,
}

impl<'a> CpNetBuilder<'a> {
    /// 从 IR Graph 构建 Petri Net
    pub fn from_ir(ir: &'a IrGraph) -> CpPetriNet {
        let mut builder = Self {
            ir,
            net: CpPetriNet::new(),
            type_to_place: HashMap::new(),
            trait_to_place: HashMap::new(),
            trait_id_to_name: HashMap::new(),
            copy_trait_id: Self::find_copy_trait(ir),
        };

        // 构建 Trait ID 到名称的映射
        builder.build_trait_map();

        // 步骤 1: 创建具体类型的 Place
        builder.create_concrete_places();

        // 步骤 2: 创建 Trait Hub Place
        builder.create_trait_hub_places();

        // 步骤 3: 创建 ImplCast Transition（Trait 实现上转）
        builder.create_impl_cast_transitions();

        // 步骤 4: 创建 Operation Transition（函数调用）
        builder.create_operation_transitions();

        builder.net
    }

    /// 查找 Copy Trait 的 ID
    fn find_copy_trait(ir: &IrGraph) -> Option<Id> {
        for trait_info in &ir.parsed_crate().traits {
            if trait_info.name == "Copy" || trait_info.name.ends_with("::Copy") {
                return Some(trait_info.id);
            }
        }
        None
    }

    /// 构建 Trait ID 到名称的映射
    fn build_trait_map(&mut self) {
        for trait_info in &self.ir.parsed_crate().traits {
            self.trait_id_to_name
                .insert(trait_info.id.clone(), trait_info.name.clone());
        }
    }

    /// 步骤 1: 为所有具体类型创建 Place
    fn create_concrete_places(&mut self) {
        // 遍历 IR Graph 中的所有类型节点
        for type_node in &self.ir.type_nodes {
            // 跳过 GenericParam（泛型参数不创建独立的 Place）
            if matches!(type_node, TypeNode::GenericParam { .. }) {
                continue;
            }

            // 为具体类型创建 Place
            self.ensure_place_for_type(type_node);
        }
    }

    /// 步骤 2: 为所有 Trait 创建 Trait Hub Place
    fn create_trait_hub_places(&mut self) {
        let mut trait_ids = HashSet::new();

        // 从 trait_impls 中收集所有被实现的 Trait ID
        for trait_id in self.ir.trait_impls.keys() {
            trait_ids.insert(trait_id.clone());
        }

        // 从操作的泛型约束中收集 Trait ID
        for op in &self.ir.operations {
            for trait_bounds in op.generic_constraints.values() {
                for trait_id in trait_bounds {
                    trait_ids.insert(trait_id.clone());
                }
            }
        }

        // 为每个 Trait 创建 Hub Place
        for trait_id in trait_ids {
            self.ensure_trait_hub_place(&trait_id);
        }
    }

    /// 步骤 3: 为每个 Trait 实现创建 ImplCast Transition
    fn create_impl_cast_transitions(&mut self) {
        // 遍历 IR Graph 中的 Trait 实现详细信息
        for trait_impl in &self.ir.trait_impl_details {
            self.create_impl_cast_transition(trait_impl);
        }
    }

    /// 步骤 4: 为每个操作创建 Operation Transition
    fn create_operation_transitions(&mut self) {
        for op in &self.ir.operations {
            self.create_operation_transition(op);
        }
    }

    /// 确保为指定类型创建 Place（如果尚未存在）
    fn ensure_place_for_type(&mut self, type_node: &TypeNode) -> String {
        // 如果已经存在，直接返回 Place ID
        if let Some(place_id) = self.type_to_place.get(type_node) {
            return place_id.clone();
        }

        // 生成唯一的 Place ID
        let place_id = self.hash_type(type_node);

        // 获取类型名称
        let type_name = self
            .ir
            .get_type_name(type_node)
            .unwrap_or("unknown")
            .to_string();

        // 获取完整路径
        let resolved_path = self.ir.get_type_path(type_node).map(|s| s.to_string());

        // 判断是否是源类型（原语）
        let is_source = matches!(type_node, TypeNode::Primitive(_));

        // 判断是否实现了 Copy
        let is_copy = self.check_is_copy(type_node);

        // 创建 Place
        let place = Place {
            id: place_id.clone(),
            type_info: type_name,
            is_trait_hub: false,
            trait_id: None,
            resolved_path,
            is_source,
            is_copy,
        };

        self.net.add_place(place);
        self.type_to_place
            .insert(type_node.clone(), place_id.clone());

        place_id
    }

    /// 确保为指定 Trait 创建 Hub Place（如果尚未存在）
    fn ensure_trait_hub_place(&mut self, trait_id: &Id) -> String {
        // 如果已经存在，直接返回 Place ID
        if let Some(place_id) = self.trait_to_place.get(trait_id) {
            return place_id.clone();
        }

        // 生成唯一的 Place ID（使用 "trait_" 前缀）
        let place_id = format!("trait_{:?}", trait_id);

        // 获取 Trait 名称
        let trait_name = self
            .trait_id_to_name
            .get(trait_id)
            .map(|s| s.as_str())
            .unwrap_or("UnknownTrait");

        // 创建 Trait Hub Place
        let place = Place {
            id: place_id.clone(),
            type_info: format!("dyn {}", trait_name),
            is_trait_hub: true,
            trait_id: Some(format!("{:?}", trait_id)),
            resolved_path: None,
            is_source: false,
            is_copy: false,
        };

        self.net.add_place(place);
        self.trait_to_place
            .insert(trait_id.clone(), place_id.clone());

        place_id
    }

    /// 为 Trait 实现创建 ImplCast Transition
    fn create_impl_cast_transition(&mut self, trait_impl: &crate::ir_graph::structure::TraitImpl) {
        // 找到实现类型对应的 TypeNode
        let impl_type_node = self.find_type_node_by_id(&trait_impl.for_type);

        if impl_type_node.is_none() {
            log::warn!(
                "无法找到类型 {:?} 的 TypeNode，跳过 ImplCast",
                trait_impl.for_type
            );
            return;
        }

        let impl_type_node = impl_type_node.unwrap();

        // 确保源类型的 Place 存在
        let source_place_id = self.ensure_place_for_type(&impl_type_node);

        // 确保目标 Trait Hub Place 存在
        let target_place_id = self.ensure_trait_hub_place(&trait_impl.trait_id);

        // 生成 Transition ID
        let trans_id = format!(
            "impl_{}_{:?}_for_{:?}",
            self.trait_id_to_name
                .get(&trait_impl.trait_id)
                .unwrap_or(&"UnknownTrait".to_string()),
            trait_impl.trait_id,
            trait_impl.for_type
        );

        // 创建 ImplCast Transition
        let transition = Transition {
            id: trans_id.clone(),
            name: format!(
                "impl {} for {}",
                self.trait_id_to_name
                    .get(&trait_impl.trait_id)
                    .unwrap_or(&"UnknownTrait".to_string()),
                self.ir.get_type_name(&impl_type_node).unwrap_or("unknown")
            ),
            kind: TransitionKind::ImplCast {
                from_type: source_place_id.clone(),
                to_trait: target_place_id.clone(),
            },
            generic_map: HashMap::new(),
        };

        self.net.add_transition(transition);

        // 创建输入弧：从源类型 Place 到 Transition（Read 类型，不消耗 Token）
        let input_arc = Arc {
            source: source_place_id.clone(),
            target: trans_id.clone(),
            arc_type: ArcType::Read,
            weight: 1,
            param_index: Some(0),
        };
        self.net.add_arc(input_arc);

        // 创建输出弧：从 Transition 到 Trait Hub Place
        let output_arc = Arc {
            source: trans_id,
            target: target_place_id,
            arc_type: ArcType::Output,
            weight: 1,
            param_index: None,
        };
        self.net.add_arc(output_arc);
    }

    /// 为操作创建 Transition
    fn create_operation_transition(&mut self, op: &OpNode) {
        // 生成 Transition ID
        let trans_id = format!("op_{}_{:?}", op.name, op.id);

        // 确定 Transition 类型
        let kind = match &op.kind {
            OpKind::FnCall => TransitionKind::Call,
            OpKind::FieldAccessor { .. } => TransitionKind::FieldAccessor,
            OpKind::MethodCall { .. } => TransitionKind::MethodCall,
            OpKind::AssocFn { .. } => TransitionKind::AssocFn,
            OpKind::ConstantAlias { .. } => TransitionKind::Call,
            OpKind::StaticAlias { .. } => TransitionKind::Call,
        };

        // 创建 Transition
        let transition = Transition {
            id: trans_id.clone(),
            name: op.name.clone(),
            kind,
            generic_map: HashMap::new(),
        };

        self.net.add_transition(transition);

        // 处理输入边（参数）
        for input_edge in &op.inputs {
            self.create_input_arc(
                op,
                &trans_id,
                &input_edge.type_node,
                input_edge.mode,
                input_edge.param_index.unwrap_or(0),
            );
        }

        // 处理输出边（返回值）
        if let Some(output_edge) = &op.output {
            self.create_output_arc(&trans_id, &output_edge.type_node, output_edge.mode, 0);
        }

        // 处理错误输出边（Result<T, E> 的 E）
        if let Some(error_edge) = &op.error_output {
            // 为错误路径创建一个单独的 Transition（模拟非确定性）
            let error_trans_id = format!("{}_error", trans_id);
            let error_transition = Transition {
                id: error_trans_id.clone(),
                name: format!("{}_error", op.name),
                kind: TransitionKind::Call,
                generic_map: HashMap::new(),
            };
            self.net.add_transition(error_transition);

            // 复制相同的输入边
            for input_edge in &op.inputs {
                self.create_input_arc(
                    op,
                    &error_trans_id,
                    &input_edge.type_node,
                    input_edge.mode,
                    input_edge.param_index.unwrap_or(0),
                );
            }

            // 创建错误输出边
            self.create_output_arc(&error_trans_id, &error_edge.type_node, error_edge.mode, 0);
        }
    }

    /// 创建输入弧（从 Place 到 Transition）
    fn create_input_arc(
        &mut self,
        op: &OpNode,
        trans_id: &str,
        type_node: &TypeNode,
        mode: EdgeMode,
        param_index: usize,
    ) {
        // 关键逻辑：处理泛型参数
        let source_place_id = match type_node {
            TypeNode::GenericParam { name, .. } => {
                // 查找该泛型参数的约束
                if let Some(trait_bounds) = op.generic_constraints.get(name) {
                    if !trait_bounds.is_empty() {
                        // 从第一个 Trait 的 Hub Place 连线
                        let trait_id = &trait_bounds[0];
                        self.ensure_trait_hub_place(trait_id)
                    } else {
                        // 没有约束，跳过
                        log::warn!("泛型参数 {} 没有约束，跳过", name);
                        return;
                    }
                } else {
                    // 没有找到约束信息，跳过
                    log::warn!("泛型参数 {} 没有约束信息，跳过", name);
                    return;
                }
            }
            _ => {
                // 具体类型，从具体类型的 Place 连线
                self.ensure_place_for_type(type_node)
            }
        };

        // 将 EdgeMode 映射到 ArcType
        let arc_type = self.edge_mode_to_arc_type(mode);

        // 创建弧
        let arc = Arc {
            source: source_place_id,
            target: trans_id.to_string(),
            arc_type,
            weight: 1,
            param_index: Some(param_index),
        };

        self.net.add_arc(arc);
    }

    /// 创建输出弧（从 Transition 到 Place）
    fn create_output_arc(
        &mut self,
        trans_id: &str,
        type_node: &TypeNode,
        _mode: EdgeMode,
        _output_index: usize,
    ) {
        // 跳过泛型参数（输出通常不会是未解析的泛型参数）
        if matches!(type_node, TypeNode::GenericParam { .. }) {
            return;
        }

        // 确保目标类型的 Place 存在
        let target_place_id = self.ensure_place_for_type(type_node);

        // 创建输出弧
        let arc = Arc {
            source: trans_id.to_string(),
            target: target_place_id,
            arc_type: ArcType::Output,
            weight: 1,
            param_index: None,
        };

        self.net.add_arc(arc);
    }

    /// 将 EdgeMode 映射到 ArcType
    fn edge_mode_to_arc_type(&self, mode: EdgeMode) -> ArcType {
        match mode {
            EdgeMode::Move => ArcType::Input,
            EdgeMode::Ref => ArcType::Read,
            EdgeMode::MutRef => ArcType::ReadWrite,
            EdgeMode::RawPtr => ArcType::Read,
            EdgeMode::MutRawPtr => ArcType::ReadWrite,
        }
    }

    /// 根据 ID 查找 TypeNode
    fn find_type_node_by_id(&self, id: &Id) -> Option<TypeNode> {
        // 遍历所有类型节点，找到匹配的
        for type_node in &self.ir.type_nodes {
            match type_node {
                TypeNode::Struct(Some(node_id))
                | TypeNode::Enum(Some(node_id))
                | TypeNode::Union(Some(node_id))
                | TypeNode::TraitObject(Some(node_id)) => {
                    if node_id == id {
                        return Some(type_node.clone());
                    }
                }
                TypeNode::GenericInstance { base_id, .. } => {
                    if base_id == id {
                        return Some(type_node.clone());
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// 检查类型是否实现了 Copy trait
    fn check_is_copy(&self, type_node: &TypeNode) -> bool {
        // 原始类型默认实现 Copy
        if let TypeNode::Primitive(name) = type_node {
            match name.as_str() {
                "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64"
                | "i128" | "isize" | "f32" | "f64" | "bool" | "char" => return true,
                _ => {}
            }
        }

        // 检查是否实现了 Copy trait
        if let Some(copy_id) = self.copy_trait_id {
            let type_id = match type_node {
                TypeNode::Struct(Some(id))
                | TypeNode::Enum(Some(id))
                | TypeNode::Union(Some(id)) => Some(id),
                _ => None,
            };
            if let Some(tid) = type_id {
                return self.ir.implements_trait(tid, &copy_id);
            }
        }

        false
    }

    /// 计算 TypeNode 的哈希值作为 Place ID
    fn hash_type(&self, type_node: &TypeNode) -> String {
        let mut hasher = DefaultHasher::new();
        type_node.hash(&mut hasher);
        format!("place_{}", hasher.finish())
    }
}

/// 公开的转换函数
pub fn convert_ir_to_petri(ir: &IrGraph) -> CpPetriNet {
    CpNetBuilder::from_ir(ir)
}
