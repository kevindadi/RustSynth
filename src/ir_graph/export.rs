use super::structure::{IrGraph, TypeNode};
/// IR Graph 导出功能
use serde_json;

impl IrGraph {
    /// 导出为 JSON 格式
    pub fn export_to_json(&self) -> serde_json::Value {
        let types: Vec<_> = self
            .type_nodes
            .iter()
            .map(|node| {
                let name = self.get_type_name(node).unwrap_or("unknown");
                let kind = match node {
                    TypeNode::Primitive(_) => "primitive",
                    TypeNode::Struct(_) => "struct",
                    TypeNode::Enum(_) => "enum",
                    TypeNode::Union(_) => "union",
                    TypeNode::TraitObject(_) => "trait_object",
                    TypeNode::GenericParam { .. } => "generic_param",
                    TypeNode::Tuple(_) => "tuple",
                    TypeNode::Array(_) => "array",
                    TypeNode::FnPointer { .. } => "fn_pointer",
                    TypeNode::Unit => "unit",
                    TypeNode::Never => "never",
                    TypeNode::QualifiedPath { .. } => "qualified_path",
                    TypeNode::Opaque(_) => "opaque",
                    TypeNode::Unknown => "unknown",
                };

                serde_json::json!({
                    "name": name,
                    "kind": kind,
                })
            })
            .collect();

        let operations: Vec<_> = self
            .operations
            .iter()
            .map(|op| {
                let inputs: Vec<_> = op
                    .inputs
                    .iter()
                    .map(|edge| {
                        serde_json::json!({
                            "type": format!("{:?}", edge.type_node),
                            "mode": format!("{:?}", edge.mode),
                            "name": edge.name,
                        })
                    })
                    .collect();

                let output = op.output.as_ref().map(|edge| {
                    serde_json::json!({
                        "type": format!("{:?}", edge.type_node),
                        "mode": format!("{:?}", edge.mode),
                    })
                });

                serde_json::json!({
                    "id": op.id.0,
                    "name": op.name,
                    "kind": format!("{:?}", op.kind),
                    "inputs": inputs,
                    "output": output,
                    "is_generic": op.is_generic(),
                    "is_unsafe": op.is_unsafe,
                })
            })
            .collect();

        serde_json::json!({
            "types": types,
            "operations": operations,
        })
    }

    /// 导出为 DOT 格式（Petri Net 风格）
    pub fn export_to_dot(&self) -> String {
        let mut dot = String::from("digraph IrGraph {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  // Types are places (circles)\n");
        dot.push_str("  node [shape=circle, style=filled, fillcolor=lightblue];\n\n");

        // 类型节点（Places）
        for (idx, node) in self.type_nodes.iter().enumerate() {
            let label = match node {
                TypeNode::GenericParam { name, owner_id, .. } => {
                    let owner_name = self
                        .parsed_crate()
                        .type_index
                        .get(owner_id)
                        .and_then(|item| item.name.as_deref())
                        .unwrap_or("unknown_owner");
                    format!("{}::{}", owner_name, name)
                }
                _ => {
                    let name = self.get_type_name(node);
                    if name.is_none() {
                        log::warn!("DOT导出: 发现未命名类型节点 (显示为unknown): {:?}", node);
                    }
                    name.unwrap_or("unknown").to_string()
                }
            };
            dot.push_str(&format!("  type_{} [label=\"{}\"];\n", idx, label));
        }

        dot.push_str("\n  // Operations are transitions (boxes)\n");
        dot.push_str("  node [shape=box, style=filled, fillcolor=lightgreen];\n\n");

        // 操作节点（Transitions）
        for (idx, op) in self.operations.iter().enumerate() {
            dot.push_str(&format!("  op_{} [label=\"{}\"];\n", idx, op.name));
        }

        dot.push_str("\n  // Edges with modes\n");

        // 连接
        for (op_idx, op) in self.operations.iter().enumerate() {
            for input in &op.inputs {
                if let Some(type_idx) = self.type_nodes.iter().position(|n| n == &input.type_node) {
                    let edge_label = format!("{:?}", input.mode);
                    dot.push_str(&format!(
                        "  type_{} -> op_{} [label=\"{}\"];\n",
                        type_idx, op_idx, edge_label
                    ));
                }
            }

            if let Some(output) = &op.output {
                if let Some(type_idx) = self.type_nodes.iter().position(|n| n == &output.type_node)
                {
                    let edge_label = format!("{:?}", output.mode);
                    dot.push_str(&format!(
                        "  op_{} -> type_{} [label=\"{}\"];\n",
                        op_idx, type_idx, edge_label
                    ));
                }
            }
        }

        dot.push_str("}\n");
        dot
    }
}
