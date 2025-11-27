/// Petri Net 导出功能
use super::structure::{EdgeKind, PetriNet, TransitionKind};
use petgraph::visit::EdgeRef;

impl PetriNet {
    /// 导出为 DOT 格式
    ///
    /// Petri Net 的 DOT 表示:
    /// - Place (库所): 圆形节点，代表类型
    /// - Transition (变迁): 方框节点，代表函数/方法
    /// - 边: 带标签的有向边，显示数据流动模式 (Move/Ref/MutRef)
    ///
    /// 配色方案:
    /// - Place: 根据类型特性着色
    ///   - Source types (fuzzing 原语): lightgreen
    ///   - Copy types: lightblue
    ///   - 其他类型: lightyellow
    /// - Transition: 根据操作类型着色
    ///   - FnCall: lightcyan
    ///   - Constructor: palegreen
    ///   - MethodCall: lightblue
    ///   - FieldAccessor: wheat
    pub fn export_to_dot(&self) -> String {
        let mut dot = String::from("digraph PetriNet {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  node [fontname=\"Arial\"];\n");
        dot.push_str("  edge [fontname=\"Arial\"];\n\n");

        // 1. 导出所有节点
        dot.push_str("  // Places (库所 - 类型节点)\n");
        for node_idx in self.graph.node_indices() {
            if let Some(place) = self.graph[node_idx].as_place() {
                let color = if place.is_source {
                    "lightgreen"
                } else if place.is_copy {
                    "lightblue"
                } else {
                    "lightyellow"
                };

                // 转义类型名中的特殊字符
                let escaped_name = escape_dot_label(&place.type_name);

                dot.push_str(&format!(
                    "  n{} [shape=circle, style=filled, fillcolor={}, label=\"{}\"];\n",
                    node_idx.index(),
                    color,
                    escaped_name
                ));
            }
        }

        dot.push_str("\n  // Transitions (变迁 - 函数节点)\n");
        for node_idx in self.graph.node_indices() {
            if let Some(transition) = self.graph[node_idx].as_transition() {
                let color = match transition.kind {
                    TransitionKind::FnCall => "lightcyan",
                    TransitionKind::StructCtor
                    | TransitionKind::VariantCtor
                    | TransitionKind::UnionCtor => "palegreen",
                    TransitionKind::MethodCall => "lightblue",
                    TransitionKind::FieldAccessor => "wheat",
                    TransitionKind::AssocFn => "lavender",
                };

                // 构建标签: 函数名 + 泛型信息
                let mut label = escape_dot_label(&transition.func_name);
                if !transition.generic_map.is_empty() {
                    let generics: Vec<String> = transition
                        .generic_map
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect();
                    label = format!("{}\\n<{}>", label, generics.join(", "));
                }

                dot.push_str(&format!(
                    "  n{} [shape=box, style=filled, fillcolor={}, label=\"{}\"];\n",
                    node_idx.index(),
                    color,
                    label
                ));
            }
        }

        // 2. 导出所有边
        dot.push_str("\n  // Edges (数据流)\n");
        for edge_ref in self.graph.edge_references() {
            let source_idx = edge_ref.source().index();
            let target_idx = edge_ref.target().index();
            let edge_data = edge_ref.weight();

            // 构建边标签
            let mode_str = match edge_data.kind {
                EdgeKind::Move => "Move",
                EdgeKind::Ref => "Ref",
                EdgeKind::MutRef => "MutRef",
            };

            let ptr_suffix = if edge_data.is_raw_ptr { "*" } else { "" };
            let label = format!("{}{}[{}]", mode_str, ptr_suffix, edge_data.index);

            // 根据边类型设置颜色
            let (color, style) = match edge_data.kind {
                EdgeKind::Move => ("black", "solid"),
                EdgeKind::Ref => ("blue", "dashed"),
                EdgeKind::MutRef => ("red", "dashed"),
            };

            dot.push_str(&format!(
                "  n{} -> n{} [label=\"{}\", color={}, style={}];\n",
                source_idx, target_idx, label, color, style
            ));
        }

        dot.push_str("}\n");
        dot
    }

    /// 导出为 JSON 格式 (用于程序化分析)
    pub fn export_to_json(&self) -> serde_json::Value {
        let places: Vec<_> = self
            .graph
            .node_weights()
            .filter_map(|node| node.as_place())
            .map(|place| {
                serde_json::json!({
                    "id": place.id,
                    "type_name": place.type_name,
                    "is_source": place.is_source,
                    "is_copy": place.is_copy,
                })
            })
            .collect();

        let transitions: Vec<_> = self
            .graph
            .node_weights()
            .filter_map(|node| node.as_transition())
            .map(|trans| {
                serde_json::json!({
                    "id": trans.id,
                    "func_name": trans.func_name,
                    "kind": format!("{:?}", trans.kind),
                    "generic_map": trans.generic_map,
                })
            })
            .collect();

        let edges: Vec<_> = self
            .graph
            .edge_references()
            .map(|edge_ref| {
                let source = edge_ref.source();
                let target = edge_ref.target();
                let edge_data = edge_ref.weight();

                // 获取源和目标的 ID
                let source_id = if let Some(place) = self.graph[source].as_place() {
                    place.id
                } else if let Some(trans) = self.graph[source].as_transition() {
                    trans.id
                } else {
                    0
                };

                let target_id = if let Some(place) = self.graph[target].as_place() {
                    place.id
                } else if let Some(trans) = self.graph[target].as_transition() {
                    trans.id
                } else {
                    0
                };

                serde_json::json!({
                    "source": source_id,
                    "target": target_id,
                    "kind": format!("{:?}", edge_data.kind),
                    "index": edge_data.index,
                    "is_raw_ptr": edge_data.is_raw_ptr,
                })
            })
            .collect();

        serde_json::json!({
            "places": places,
            "transitions": transitions,
            "edges": edges,
        })
    }

    /// 导出统计信息
    pub fn export_stats(&self) -> String {
        let place_count = self
            .graph
            .node_weights()
            .filter(|n| n.as_place().is_some())
            .count();

        let transition_count = self
            .graph
            .node_weights()
            .filter(|n| n.as_transition().is_some())
            .count();

        let source_count = self
            .graph
            .node_weights()
            .filter_map(|n| n.as_place())
            .filter(|p| p.is_source)
            .count();

        let copy_count = self
            .graph
            .node_weights()
            .filter_map(|n| n.as_place())
            .filter(|p| p.is_copy)
            .count();

        let edge_count = self.graph.edge_count();

        format!(
            "=== Petri Net 统计 ===\n\
             库所 (Places): {}\n\
               - Source types: {}\n\
               - Copy types: {}\n\
             变迁 (Transitions): {}\n\
             边 (Edges): {}\n",
            place_count, source_count, copy_count, transition_count, edge_count
        )
    }
}

/// 转义 DOT 标签中的特殊字符
fn escape_dot_label(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
