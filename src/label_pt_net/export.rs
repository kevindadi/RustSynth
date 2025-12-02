use crate::label_pt_net::net::LabeledPetriNet;
use crate::petri_net_traits::{escape_dot, escape_xml, PetriNetExport};

/// 导出 Petri 网为 PNML 格式(Petri Net Markup Language)
impl PetriNetExport for LabeledPetriNet {
    /// 导出为 PNML XML 格式
    fn to_pnml(&self) -> String {
        let mut xml = String::new();
        xml.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
        xml.push('\n');
        xml.push_str(r#"<pnml xmlns="http://www.pnml.org/version-2009/grammar/pnml">"#);
        xml.push('\n');
        xml.push_str(
            r#"  <net id="ir_graph_net" type="http://www.pnml.org/version-2009/grammar/ptnet">"#,
        );
        xml.push('\n');
        xml.push_str(r#"    <page id="page1">"#);
        xml.push('\n');

        // Places
        for (idx, place) in self.places.iter().enumerate() {
            let tokens = self.initial_marking.get(idx).copied().unwrap_or(0);
            xml.push_str(&format!(
                r#"      <place id="p{}">
        <name><text>{}</text></name>
        <initialMarking><text>{}</text></initialMarking>
      </place>
"#,
                idx,
                escape_xml(place),
                tokens
            ));
        }

        // Transitions
        for (idx, trans) in self.transitions.iter().enumerate() {
            xml.push_str(&format!(
                r#"      <transition id="t{}">
        <name><text>{}</text></name>
      </transition>
"#,
                idx,
                escape_xml(trans)
            ));
        }

        // Arcs
        for (idx, arc) in self.arcs.iter().enumerate() {
            let (source_id, target_id) = if arc.is_input_arc {
                (format!("p{}", arc.from_idx), format!("t{}", arc.to_idx))
            } else {
                (format!("t{}", arc.from_idx), format!("p{}", arc.to_idx))
            };

            xml.push_str(&format!(
                r#"      <arc id="a{}" source="{}" target="{}">
        <inscription><text>{}</text></inscription>
      </arc>
"#,
                idx, source_id, target_id, arc.weight
            ));
        }

        xml.push_str("    </page>\n");
        xml.push_str("  </net>\n");
        xml.push_str("</pnml>\n");

        xml
    }

    /// 导出为 DOT 格式(用于 Graphviz 可视化)
    fn to_dot(&self) -> String {
        let mut dot = String::new();
        dot.push_str("digraph PetriNet {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  node [style=filled];\n\n");

        // Places (circles)
        dot.push_str("  // Places\n");
        for (idx, place) in self.places.iter().enumerate() {
            let tokens = self.initial_marking.get(idx).copied().unwrap_or(0);
            let label = if tokens > 0 {
                format!("{}\\n[{}]", place, tokens)
            } else {
                place.clone()
            };
            dot.push_str(&format!(
                "  p{} [label=\"{}\" shape=circle fillcolor=lightblue];\n",
                idx,
                escape_dot(&label)
            ));
        }

        // Transitions (boxes)
        dot.push_str("\n  // Transitions\n");
        for (idx, trans) in self.transitions.iter().enumerate() {
            let attr = self.transition_attrs.get(idx);
            let (color, style) = match attr {
                Some(a) if a.is_const && a.is_unsafe => ("orange", "bold"), // const unsafe
                Some(a) if a.is_const => ("gold", "bold"),                  // const
                Some(a) if a.is_async => ("lightpink", "dashed"),           // async
                Some(a) if a.is_unsafe => ("salmon", "solid"),              // unsafe
                _ => ("palegreen", "solid"),                                // normal
            };
            dot.push_str(&format!(
                "  t{} [label=\"{}\" shape=box fillcolor={} style={}];\n",
                idx,
                escape_dot(trans),
                color,
                style
            ));
        }

        // Arcs
        dot.push_str("\n  // Arcs\n");
        for arc in &self.arcs {
            let (source, target) = if arc.is_input_arc {
                (format!("p{}", arc.from_idx), format!("t{}", arc.to_idx))
            } else {
                (format!("t{}", arc.from_idx), format!("p{}", arc.to_idx))
            };

            let label = format!("{:?}", arc.label);
            let edge_label = if let Some(name) = &arc.name {
                format!("{}\\n{}", label, name)
            } else {
                label
            };

            dot.push_str(&format!(
                "  {} -> {} [label=\"{}\"];\n",
                source, target, edge_label
            ));
        }

        dot.push_str("}\n");
        dot
    }

    /// 导出为 JSON 格式
    fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    fn print_stats(&self) {
        let stats = self.stats();
        println!("Petri Net Stats:");
        println!("  Places: {}", stats.place_count);
        println!("  Transitions: {}", stats.transition_count);
        println!("  Input Arcs: {}", stats.input_arc_count);
        println!("  Output Arcs: {}", stats.output_arc_count);
        println!("  Total Initial Tokens: {}", stats.total_initial_tokens);
    }
}


