use crate::petri_net_traits::PetriNetExport;
use crate::pushdown_colored_pt_net::net::PushdownColoredPetriNet;
use crate::petri_net_traits::escape_dot;
use serde_json;

impl PetriNetExport for PushdownColoredPetriNet {
    fn to_pnml(&self) -> String {
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str("<pnml>\n");
        xml.push_str("  <net id=\"pcpn\" type=\"http://www.pnml.org/version-2009/grammar/pnmlcoremodel\">\n");

        for (idx, place) in self.places.iter().enumerate() {
            xml.push_str(&format!(
                "    <place id=\"p{}\">\n",
                idx
            ));
            xml.push_str(&format!("      <name><text>{}</text></name>\n", escape_xml(place)));
                        
            if let Some(colors) = self.initial_marking.get(&idx) {
                for (color, count) in colors {
                    if *count > 0 {
                        xml.push_str(&format!(
                            "      <initialMarking><text>{}:{}</text></initialMarking>\n",
                            escape_xml(&color.to_string()),
                            count
                        ));
                    }
                }
            }
            
            xml.push_str("    </place>\n");
        }

        // Transitions
        for (idx, transition) in self.transitions.iter().enumerate() {
            xml.push_str(&format!(
                "    <transition id=\"t{}\">\n",
                idx
            ));
            xml.push_str(&format!("      <name><text>{}</text></name>\n", escape_xml(transition)));
            
            // 栈操作
            if let Some(stack_op) = self.stack_operations.get(&idx) {
                xml.push_str(&format!(
                    "      <toolspecific tool=\"pcpn\" version=\"1.0\">\n"
                ));
                xml.push_str(&format!(
                    "        <stackOperation>{:?}</stackOperation>\n",
                    stack_op
                ));
                xml.push_str("      </toolspecific>\n");
            }
            
            xml.push_str("    </transition>\n");
        }

        // Arcs
        for (idx, arc) in self.arcs.iter().enumerate() {
            let (from_id, to_id) = if arc.is_input_arc {
                (format!("p{}", arc.from_idx), format!("t{}", arc.to_idx))
            } else {
                (format!("t{}", arc.from_idx), format!("p{}", arc.to_idx))
            };
            
            xml.push_str(&format!(
                "    <arc id=\"a{}\" source=\"{}\" target=\"{}\">\n",
                idx, from_id, to_id
            ));
            
            // 颜色约束
            if let Some(ref color) = arc.color_constraint {
                xml.push_str(&format!(
                    "      <inscription><text>color:{}</text></inscription>\n",
                    escape_xml(&color.to_string())
                ));
            }
            
            if arc.weight > 1 {
                xml.push_str(&format!(
                    "      <inscription><text>weight:{}</text></inscription>\n",
                    arc.weight
                ));
            }
            
            xml.push_str("    </arc>\n");
        }

        xml.push_str("  </net>\n");
        xml.push_str("</pnml>\n");
        xml
    }

    fn to_dot(&self) -> String {
        let mut dot = String::from("digraph PushdownColoredPetriNet {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  node [shape=circle, style=filled];\n");

        // Places
        for (idx, place) in self.places.iter().enumerate() {
            let mut label = escape_dot(place);
            
            // 添加初始标记信息
            if let Some(colors) = self.initial_marking.get(&idx) {
                let tokens: Vec<String> = colors
                    .iter()
                    .filter(|(_, count)| **count > 0)
                    .map(|(color, count)| format!("{}:{}", escape_dot(&color.to_string()), count))
                    .collect();
                if !tokens.is_empty() {
                    label.push_str(&format!("\\n[{}]", tokens.join(", ")));
                }
            }
            
            dot.push_str(&format!(
                "  p{} [label=\"{}\", fillcolor=lightblue];\n",
                idx, label
            ));
        }

        // Transitions
        for (idx, transition) in self.transitions.iter().enumerate() {
            let mut label = escape_dot(transition);
            
            // 添加栈操作信息
            if let Some(stack_op) = self.stack_operations.get(&idx) {
                match stack_op {
                    crate::pushdown_colored_pt_net::net::StackOperation::Push => {
                        label.push_str("\\n[Push]");
                    }
                    crate::pushdown_colored_pt_net::net::StackOperation::Pop => {
                        label.push_str("\\n[Pop]");
                    }
                    crate::pushdown_colored_pt_net::net::StackOperation::PushPop => {
                        label.push_str("\\n[PushPop]");
                    }
                    _ => {}
                }
            }
            
            dot.push_str(&format!(
                "  t{} [label=\"{}\", shape=box, fillcolor=lightgreen];\n",
                idx, label
            ));
        }

        // Arcs
        for arc in &self.arcs {
            let (from_id, to_id) = if arc.is_input_arc {
                (format!("p{}", arc.from_idx), format!("t{}", arc.to_idx))
            } else {
                (format!("t{}", arc.from_idx), format!("p{}", arc.to_idx))
            };
            
            let mut arc_label = String::new();
            if let Some(ref color) = arc.color_constraint {
                arc_label.push_str(&escape_dot(&color.to_string()));
            }
            if let Some(ref name) = arc.name {
                if !arc_label.is_empty() {
                    arc_label.push_str(", ");
                }
                arc_label.push_str(&escape_dot(name));
            }
            
            let label_attr = if !arc_label.is_empty() {
                format!(" [label=\"{}\"]", arc_label)
            } else {
                String::new()
            };
            
            dot.push_str(&format!("  {} -> {}{};\n", from_id, to_id, label_attr));
        }

        dot.push_str("}\n");
        dot
    }

    fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    fn get_stats_string(&self) -> String {
        let stats = self.stats();
        format!(
            "Places: {}, Transitions: {}, Input Arcs: {}, Output Arcs: {}, \
             Initial Tokens: {}, Colors: {}, Transitions with Stack: {}",
            stats.place_count,
            stats.transition_count,
            stats.input_arc_count,
            stats.output_arc_count,
            stats.total_initial_tokens,
            stats.color_count,
            stats.transitions_with_stack
        )
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
