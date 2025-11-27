use super::structure::{EdgeData, EdgeKind, PetriNet, PlaceData, TransitionData, TransitionKind};
use crate::ir_graph::structure::{EdgeMode, IrGraph, OpKind, OpNode, TypeNode};
use rustdoc_types::Id;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Petri Net 构建器
pub struct PetriNetBuilder;

impl PetriNetBuilder {
    /// 从 IR Graph 构建 Petri Net
    pub fn from_ir(ir: &IrGraph) -> PetriNet {
        let mut net = PetriNet::new();
        let mut builder = BuilderContext {
            net: &mut net,
            ir,
            copy_trait_id: Self::find_copy_trait(ir),
        };

        builder.build();
        net
    }

    /// 尝试查找 Copy trait 的 ID
    fn find_copy_trait(ir: &IrGraph) -> Option<Id> {
        // 简单策略: 遍历所有 trait，查找名为 "Copy" 或以 "::Copy" 结尾的
        // 注意: 这里假设 ir.parsed_crate().traits 包含了相关 trait 信息
        // 或者我们可以检查 trait_blacklist 中的定义?
        // 更准确的方法是在解析阶段就标记 Copy trait。
        // 这里暂时尝试通过名称匹配。

        // 由于 IR Graph 可能没有直接存储 Trait 列表（只有 parsed_crate 有），
        // 我们从 parsed_crate.traits 中查找
        for trait_info in &ir.parsed_crate().traits {
            if trait_info.name == "Copy" || trait_info.name.ends_with("::Copy") {
                return Some(trait_info.id);
            }
        }
        None
    }
}

struct BuilderContext<'a> {
    net: &'a mut PetriNet,
    ir: &'a IrGraph,
    copy_trait_id: Option<Id>,
}

impl<'a> BuilderContext<'a> {
    fn build(&mut self) {
        // Copy the reference to avoid borrowing self
        let ir = self.ir;
        for op in &ir.operations {
            self.convert_operation(op);
        }
    }

    fn convert_operation(&mut self, op: &OpNode) {
        // 1. 创建 Transition
        // 使用 hash_id 将 rustdoc Id 转换为 u64
        let trans_id = self.hash_id(&op.id);

        let kind = match op.kind {
            OpKind::FnCall => TransitionKind::FnCall,
            OpKind::StructCtor => TransitionKind::StructCtor,
            OpKind::VariantCtor { .. } => TransitionKind::VariantCtor, // 丢失了变体信息，但对于 P/T net 可能足够
            OpKind::UnionCtor => TransitionKind::UnionCtor,
            OpKind::FieldAccessor { .. } => TransitionKind::FieldAccessor,
            OpKind::MethodCall { .. } => TransitionKind::MethodCall,
            OpKind::AssocFn { .. } => TransitionKind::AssocFn,
        };

        let trans_data = TransitionData {
            id: trans_id,
            func_name: op.name.clone(),
            kind,
            generic_map: HashMap::new(), // 暂不处理泛型
        };

        let trans_idx = self.net.add_transition(trans_data);

        // 2. 处理输入 (Arguments)
        for (idx, input_edge) in op.inputs.iter().enumerate() {
            // 跳过 GenericParam (根据需求: "Ignore GenericParam nodes for now")
            if let TypeNode::GenericParam { .. } = input_edge.type_node {
                continue;
            }

            let place_data = self.create_place_data(&input_edge.type_node);
            let place_idx = self.net.add_place(place_data);

            let edge_data = self.convert_edge_mode(input_edge.mode, idx);

            // Input edge: Place -> Transition
            self.net.connect(place_idx, trans_idx, edge_data);
        }

        // 3. 处理输出 (Return Value)
        if let Some(output_edge) = &op.output {
            if let TypeNode::GenericParam { .. } = output_edge.type_node {
                // skip
            } else {
                let place_data = self.create_place_data(&output_edge.type_node);
                let place_idx = self.net.add_place(place_data);

                // Output edge: Transition -> Place
                // 通常返回值是 Move，但如果是引用返回，保持 Ref
                // index 0 for return value
                let edge_data = self.convert_edge_mode(output_edge.mode, 0);
                self.net.connect(trans_idx, place_idx, edge_data);
            }
        }

        // 4. 处理错误输出 (Result Error)
        if let Some(error_edge) = &op.error_output {
            if let TypeNode::GenericParam { .. } = error_edge.type_node {
                // skip
            } else {
                let place_data = self.create_place_data(&error_edge.type_node);
                let place_idx = self.net.add_place(place_data);

                // Error output edge: Transition -> Place
                // 使用索引 1 区分正常返回 (0) 和错误返回 (1)
                let edge_data = self.convert_edge_mode(error_edge.mode, 1);
                self.net.connect(trans_idx, place_idx, edge_data);
            }
        }
    }

    fn create_place_data(&self, type_node: &TypeNode) -> PlaceData {
        let id = self.hash_type(type_node);
        let type_name = self
            .ir
            .get_type_name(type_node)
            .unwrap_or("unknown")
            .to_string();

        let is_source = matches!(type_node, TypeNode::Primitive(_));

        // 检查是否 Copy
        let is_copy = self.check_is_copy(type_node);

        PlaceData {
            id,
            type_name,
            is_source,
            is_copy,
        }
    }

    fn check_is_copy(&self, type_node: &TypeNode) -> bool {
        // 如果是 Primitive，大多是 Copy (除了 str, 但 str 通常是 &str 引用)
        // 严格来说应该查 trait impls
        if let TypeNode::Primitive(name) = type_node {
            match name.as_str() {
                "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64"
                | "i128" | "isize" | "f32" | "f64" | "bool" | "char" => return true,
                _ => {}
            }
        }

        if let Some(copy_id) = self.copy_trait_id {
            // 获取该类型的 ID
            let type_id = match type_node {
                TypeNode::Struct(Some(id))
                | TypeNode::Enum(Some(id))
                | TypeNode::Union(Some(id)) => Some(id),
                _ => None, // 复杂类型暂不深入检查
            };

            if let Some(tid) = type_id {
                return self.ir.implements_trait(tid, &copy_id);
            }
        }

        false
    }

    fn convert_edge_mode(&self, mode: EdgeMode, index: usize) -> EdgeData {
        let (kind, is_raw_ptr) = match mode {
            EdgeMode::Move => (EdgeKind::Move, false),
            EdgeMode::Ref => (EdgeKind::Ref, false),
            EdgeMode::MutRef => (EdgeKind::MutRef, false),
            EdgeMode::RawPtr => (EdgeKind::Ref, true),
            EdgeMode::MutRawPtr => (EdgeKind::MutRef, true),
        };

        EdgeData {
            kind,
            index,
            is_raw_ptr,
        }
    }

    fn hash_type(&self, type_node: &TypeNode) -> u64 {
        let mut hasher = DefaultHasher::new();
        type_node.hash(&mut hasher);
        hasher.finish()
    }

    fn hash_id(&self, id: &Id) -> u64 {
        let mut hasher = DefaultHasher::new();
        id.hash(&mut hasher);
        hasher.finish()
    }
}
