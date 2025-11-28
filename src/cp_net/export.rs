/// CP-Net 导出功能
///
/// 支持导出为：
/// - JSON 格式（用于序列化和持久化）
/// - DOT 格式（用于 Graphviz 可视化）
use super::structure::{ArcType, CpPetriNet, TransitionKind};
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

impl CpPetriNet {
    /// 导出为 JSON 文件
    pub fn export_json<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let json = self
            .to_json()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let mut file = File::create(path)?;
        file.write_all(json.as_bytes())?;
        Ok(())
    }

    /// 导出为 DOT 格式（Graphviz）
    pub fn export_dot<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let dot = self.to_dot();
        let mut file = File::create(path)?;
        file.write_all(dot.as_bytes())?;
        Ok(())
    }

    /// 生成 DOT 格式字符串
    pub fn to_dot(&self) -> String {
        let mut output = String::new();

        output.push_str("digraph CPPetriNet {\n");
        output.push_str("  rankdir=LR;\n");
        output.push_str("  node [fontname=\"Helvetica\"];\n");
        output.push_str("  edge [fontname=\"Helvetica\"];\n\n");

        // 定义 Place 节点
        output.push_str("  // Places\n");
        for place in &self.places {
            let shape = if place.is_trait_hub {
                "hexagon"
            } else if place.is_source {
                "doublecircle"
            } else {
                "circle"
            };

            let color = if place.is_trait_hub {
                "orange"
            } else if place.is_source {
                "green"
            } else if place.is_copy {
                "lightblue"
            } else {
                "white"
            };

            let label = if place.is_trait_hub {
                format!("dyn {}", place.type_info.replace("dyn ", ""))
            } else {
                place.type_info.clone()
            };

            output.push_str(&format!(
                "  \"{}\" [label=\"{}\", shape={}, style=filled, fillcolor={}];\n",
                place.id, label, shape, color
            ));
        }

        output.push_str("\n  // Transitions\n");
        for transition in &self.transitions {
            let shape = match &transition.kind {
                TransitionKind::ImplCast { .. } => "diamond",
                TransitionKind::Constructor => "house",
                TransitionKind::FieldAccessor => "invhouse",
                _ => "box",
            };

            let color = match &transition.kind {
                TransitionKind::ImplCast { .. } => "yellow",
                TransitionKind::Constructor => "lightgreen",
                TransitionKind::FieldAccessor => "lightpink",
                _ => "lightgray",
            };

            // 转义特殊字符
            let label = transition
                .name
                .replace("\"", "\\\"")
                .replace("<", "\\<")
                .replace(">", "\\>");

            output.push_str(&format!(
                "  \"{}\" [label=\"{}\", shape={}, style=filled, fillcolor={}];\n",
                transition.id, label, shape, color
            ));
        }

        output.push_str("\n  // Arcs\n");
        for arc in &self.arcs {
            let style = match arc.arc_type {
                ArcType::Input => "solid",
                ArcType::Output => "solid",
                ArcType::Read => "dashed",
                ArcType::ReadWrite => "bold",
            };

            let color = match arc.arc_type {
                ArcType::Input => "red",
                ArcType::Output => "blue",
                ArcType::Read => "green",
                ArcType::ReadWrite => "purple",
            };

            let label = if let Some(idx) = arc.param_index {
                format!(
                    "{}[{}]",
                    match arc.arc_type {
                        ArcType::Input => "move",
                        ArcType::Output => "out",
                        ArcType::Read => "ref",
                        ArcType::ReadWrite => "mut",
                    },
                    idx
                )
            } else {
                match arc.arc_type {
                    ArcType::Input => "move".to_string(),
                    ArcType::Output => "out".to_string(),
                    ArcType::Read => "ref".to_string(),
                    ArcType::ReadWrite => "mut".to_string(),
                }
            };

            output.push_str(&format!(
                "  \"{}\" -> \"{}\" [label=\"{}\", style={}, color={}];\n",
                arc.source, arc.target, label, style, color
            ));
        }

        output.push_str("}\n");
        output
    }

    /// 打印统计信息
    pub fn print_stats(&self) {
        println!("=== CP-Petri Net 统计 ===");
        println!("Places: {}", self.places.len());

        let concrete_places = self.places.iter().filter(|p| !p.is_trait_hub).count();
        let trait_hubs = self.places.iter().filter(|p| p.is_trait_hub).count();
        let source_places = self.places.iter().filter(|p| p.is_source).count();

        println!("  - 具体类型 Places: {}", concrete_places);
        println!("  - Trait Hub Places: {}", trait_hubs);
        println!("  - 源类型 Places: {}", source_places);

        println!("Transitions: {}", self.transitions.len());

        let call_trans = self
            .transitions
            .iter()
            .filter(|t| matches!(t.kind, TransitionKind::Call))
            .count();
        let impl_cast_trans = self
            .transitions
            .iter()
            .filter(|t| matches!(t.kind, TransitionKind::ImplCast { .. }))
            .count();
        let ctor_trans = self
            .transitions
            .iter()
            .filter(|t| matches!(t.kind, TransitionKind::Constructor))
            .count();

        println!("  - 函数调用: {}", call_trans);
        println!("  - ImplCast (Trait 上转): {}", impl_cast_trans);
        println!("  - 构造器: {}", ctor_trans);

        println!("Arcs: {}", self.arcs.len());

        let input_arcs = self
            .arcs
            .iter()
            .filter(|a| a.arc_type == ArcType::Input)
            .count();
        let output_arcs = self
            .arcs
            .iter()
            .filter(|a| a.arc_type == ArcType::Output)
            .count();
        let read_arcs = self
            .arcs
            .iter()
            .filter(|a| a.arc_type == ArcType::Read)
            .count();
        let readwrite_arcs = self
            .arcs
            .iter()
            .filter(|a| a.arc_type == ArcType::ReadWrite)
            .count();

        println!("  - Input (Move): {}", input_arcs);
        println!("  - Output: {}", output_arcs);
        println!("  - Read (Ref): {}", read_arcs);
        println!("  - ReadWrite (MutRef): {}", readwrite_arcs);
    }
}
