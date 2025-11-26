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

                let error_output = op.error_output.as_ref().map(|edge| {
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
                    "error_output": error_output,
                    "is_generic": op.is_generic(),
                    "is_unsafe": op.is_unsafe,
                    "is_fallible": op.is_fallible,
                })
            })
            .collect();

        serde_json::json!({
            "types": types,
            "operations": operations,
        })
    }

    /// 导出为 DOT 格式(Petri Net 风格)
    ///
    /// 配色方案:
    /// - 类型节点(圆形):
    ///   - Primitive(基本类型): lightblue
    ///   - Struct(结构体): lightcyan
    ///   - Enum(枚举): lightyellow
    ///   - Union(联合体): lightpink
    ///   - Trait(特质): lavender
    ///   - Generic(泛型): lightgray
    ///   - Array/Tuple(数组/元组): wheat
    /// - 操作节点(方框):
    ///   - unsafe(不安全): red
    ///   - fallible(可失败): orange
    ///   - Constructor(构造器): lightgreen
    ///   - FieldAccessor(字段访问): palegreen
    ///   - 普通函数/方法: lightgreen
    pub fn export_to_dot(&self) -> String {
        let mut dot = String::from("digraph IrGraph {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  // Types are places (circles)\n\n");

        // 类型节点(Places) - 根据类型种类设置不同颜色
        for (idx, node) in self.type_nodes.iter().enumerate() {
            let (label, color) = match node {
                TypeNode::Primitive(_) => {
                    let name = self.get_type_name(node).unwrap_or("unknown");
                    (name.to_string(), "lightblue")
                }
                TypeNode::Struct(_) => {
                    let name = self.get_type_name(node).unwrap_or("unknown");
                    (name.to_string(), "lightcyan")
                }
                TypeNode::Enum(_) => {
                    let name = self.get_type_name(node).unwrap_or("unknown");
                    (name.to_string(), "lightyellow")
                }
                TypeNode::Union(_) => {
                    let name = self.get_type_name(node).unwrap_or("unknown");
                    (name.to_string(), "lightpink")
                }
                TypeNode::TraitObject(_) => {
                    let name = self.get_type_name(node).unwrap_or("unknown");
                    (name.to_string(), "lavender")
                }
                TypeNode::GenericParam { name, owner_id, .. } => {
                    let owner_name = self
                        .parsed_crate()
                        .type_index
                        .get(owner_id)
                        .and_then(|item| item.name.as_deref())
                        .unwrap_or("unknown_owner");
                    (format!("{}::{}", owner_name, name), "lightgray")
                }
                TypeNode::Array(_) | TypeNode::Tuple(_) => {
                    let name = self.get_type_name(node).unwrap_or("unknown");
                    (name.to_string(), "wheat")
                }
                TypeNode::Unit => ("()".to_string(), "white"),
                TypeNode::Never => ("!".to_string(), "white"),
                _ => {
                    let name = self.get_type_name(node);
                    if name.is_none() {
                        log::warn!("DOT导出: 发现未命名类型节点 (显示为unknown): {:?}", node);
                    }
                    (name.unwrap_or("unknown").to_string(), "white")
                }
            };

            dot.push_str(&format!(
                "  type_{} [shape=circle, style=filled, fillcolor={}, label=\"{}\"];\n",
                idx, color, label
            ));
        }

        dot.push_str("\n  // Operations are transitions (boxes)\n\n");

        // 操作节点(Transitions) - 根据操作特性设置不同颜色
        for (idx, op) in self.operations.iter().enumerate() {
            // 确定颜色
            let color = if op.is_unsafe {
                // unsafe 函数用红色
                "red"
            } else if op.is_fallible {
                // 可失败的函数用橙色
                "orange"
            } else if op.is_constructor() {
                // 构造器用浅绿色
                "lightgreen"
            } else if op.is_field_accessor() {
                // 字段访问器用淡绿色
                "palegreen"
            } else {
                // 普通函数/方法用浅绿色
                "lightgreen"
            };
            
            // 构建标签
            let mut label = op.name.clone();
            if op.is_unsafe {
                label = format!("⚠️ {}", label);
            }
            if op.is_fallible {
                label = format!("{} (?)", label);
            }
            
            // 添加文档注释（如果有）
            if let Some(docs) = &op.docs {
                // 清理文档注释：去除换行符，限制长度
                let cleaned_docs = docs
                    .lines()
                    .map(|line| line.trim())
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                
                // 限制文档长度，避免标签过长
                let docs_preview = if cleaned_docs.len() > 100 {
                    format!("{}...", &cleaned_docs[..100])
                } else {
                    cleaned_docs
                };
                
                // 转义引号
                let escaped_docs = docs_preview.replace("\"", "\\\"");
                label = format!("{}\\n📝 {}", label, escaped_docs);
            }
            
            dot.push_str(&format!(
                "  op_{} [shape=box, style=filled, fillcolor={}, label=\"{}\"];\n",
                idx, color, label
            ));
        }

        dot.push_str("\n  // Edges with modes\n");

        // 连接
        for (op_idx, op) in self.operations.iter().enumerate() {
            // 输入边
            for input in &op.inputs {
                if let Some(type_idx) = self.type_nodes.iter().position(|n| n == &input.type_node) {
                    let edge_label = format!("{:?}", input.mode);
                    dot.push_str(&format!(
                        "  type_{} -> op_{} [label=\"{}\"];\n",
                        type_idx, op_idx, edge_label
                    ));
                }
            }

            // 成功输出边
            if let Some(output) = &op.output {
                if let Some(type_idx) = self.type_nodes.iter().position(|n| n == &output.type_node)
                {
                    let edge_label = format!("{:?}", output.mode);
                    let edge_style = if op.error_output.is_some() {
                        // 如果有错误输出，用绿色标识成功分支
                        "[label=\"{}\", color=green, penwidth=2.0]"
                    } else {
                        "[label=\"{}\"]"
                    };
                    dot.push_str(&format!(
                        "  op_{} -> type_{} {};\n",
                        op_idx,
                        type_idx,
                        edge_style.replace("{}", &edge_label)
                    ));
                }
            }

            // 错误输出边(用红色标识)
            if let Some(error_output) = &op.error_output {
                if let Some(type_idx) = self
                    .type_nodes
                    .iter()
                    .position(|n| n == &error_output.type_node)
                {
                    let edge_label = format!("{:?}", error_output.mode);
                    dot.push_str(&format!(
                        "  op_{} -> type_{} [label=\"{}\", color=red, penwidth=2.0, style=dashed];\n",
                        op_idx, type_idx, edge_label
                    ));
                }
            }
        }

        dot.push_str("}\n");
        dot
    }
}
