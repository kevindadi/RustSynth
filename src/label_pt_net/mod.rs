//! Labeled Petri Net (LPN) 表示及从 IrGraph 的转换
//!
//! # 设计说明
//!
//! ## Petri 网基本概念
//! - **Place（库所）**：表示系统状态或资源，对应数据类型节点
//! - **Transition（变迁）**：表示状态转换或操作，对应方法/函数节点
//! - **Arc（弧）**：连接 Place 和 Transition，表示数据流
//! - **Token（令牌）**：Place 中的标记，表示资源数量
//!
//! ## 从 IrGraph 的映射
//! - 数据节点 (Struct, Enum, Constant, Primitive 等) → Place
//! - 操作节点 (Method, Function, UnwrapOp) → Transition
//! - 边 (TypeRelation) → Arc，EdgeMode 作为标签
//!
//! ## 守卫逻辑（Guard）
//! 在完整的 Petri 网模拟中，某些变迁需要守卫条件：
//! - `Ref`/`MutRef` 边：借用检查（同时只能有一个 MutRef 或多个 Ref）
//! - `Implements` 边：Trait 约束检查
//! - `UnwrapOp`：Result/Option 分支选择
//!
//! 当前实现不包含模拟逻辑，仅生成静态的 Petri 网结构。

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::ir_graph::{EdgeMode, IrGraph, NodeInfo, NodeType};

/// Labeled Petri Net 结构
///
/// 使用索引而非直接引用，便于序列化和跨系统传输
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabeledPetriNet {
    /// 库所列表（数据类型节点）
    pub places: Vec<String>,
    /// 变迁列表（操作节点）
    pub transitions: Vec<String>,
    /// 变迁属性（是否是 const 函数等）
    pub transition_attrs: Vec<TransitionAttr>,
    /// 弧列表：(from_idx, to_idx, is_input_arc, label, weight)
    /// - is_input_arc: true 表示从 place 到 transition，false 表示从 transition 到 place
    /// - from_idx/to_idx: 根据 is_input_arc 分别是 places/transitions 的索引
    pub arcs: Vec<Arc>,
    /// 初始标记：每个 place 的初始 token 数量
    pub initial_marking: Vec<usize>,
}

/// 变迁属性
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransitionAttr {
    /// 是否是 const 函数
    pub is_const: bool,
    /// 是否是 async 函数
    pub is_async: bool,
    /// 是否是 unsafe 函数
    pub is_unsafe: bool,
}

/// 弧的详细信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arc {
    /// 源索引（place 或 transition 的索引）
    pub from_idx: usize,
    /// 目标索引（transition 或 place 的索引）
    pub to_idx: usize,
    /// 是否是输入弧（place → transition）
    pub is_input_arc: bool,
    /// 弧的标签（EdgeMode）
    pub label: EdgeMode,
    /// 弧的权重（默认为 1）
    pub weight: usize,
    /// 可选的字段/参数名称
    pub name: Option<String>,
}

impl LabeledPetriNet {
    /// 创建空的 Petri 网
    pub fn new() -> Self {
        Self {
            places: Vec::new(),
            transitions: Vec::new(),
            transition_attrs: Vec::new(),
            arcs: Vec::new(),
            initial_marking: Vec::new(),
        }
    }

    /// 添加一个 place，返回其索引
    pub fn add_place(&mut self, name: String) -> usize {
        let idx = self.places.len();
        self.places.push(name);
        self.initial_marking.push(0); // 默认初始标记为 0
        idx
    }

    /// 添加一个 transition，返回其索引
    pub fn add_transition(&mut self, name: String) -> usize {
        let idx = self.transitions.len();
        self.transitions.push(name);
        self.transition_attrs.push(TransitionAttr::default());
        idx
    }

    /// 添加一个带属性的 transition，返回其索引
    pub fn add_transition_with_attr(&mut self, name: String, attr: TransitionAttr) -> usize {
        let idx = self.transitions.len();
        self.transitions.push(name);
        self.transition_attrs.push(attr);
        idx
    }

    /// 添加输入弧（place → transition）
    pub fn add_input_arc(
        &mut self,
        place_idx: usize,
        transition_idx: usize,
        label: EdgeMode,
        weight: usize,
        name: Option<String>,
    ) {
        self.arcs.push(Arc {
            from_idx: place_idx,
            to_idx: transition_idx,
            is_input_arc: true,
            label,
            weight,
            name,
        });
    }

    /// 添加输出弧（transition → place）
    pub fn add_output_arc(
        &mut self,
        transition_idx: usize,
        place_idx: usize,
        label: EdgeMode,
        weight: usize,
        name: Option<String>,
    ) {
        self.arcs.push(Arc {
            from_idx: transition_idx,
            to_idx: place_idx,
            is_input_arc: false,
            label,
            weight,
            name,
        });
    }

    /// 设置 place 的初始标记
    pub fn set_initial_marking(&mut self, place_idx: usize, tokens: usize) {
        if place_idx < self.initial_marking.len() {
            self.initial_marking[place_idx] = tokens;
        }
    }

    /// 获取统计信息
    pub fn stats(&self) -> PetriNetStats {
        let input_arcs = self.arcs.iter().filter(|a| a.is_input_arc).count();
        let output_arcs = self.arcs.iter().filter(|a| !a.is_input_arc).count();
        let total_tokens: usize = self.initial_marking.iter().sum();

        PetriNetStats {
            place_count: self.places.len(),
            transition_count: self.transitions.len(),
            input_arc_count: input_arcs,
            output_arc_count: output_arcs,
            total_initial_tokens: total_tokens,
        }
    }
}

impl Default for LabeledPetriNet {
    fn default() -> Self {
        Self::new()
    }
}

/// Petri 网统计信息
#[derive(Debug, Clone)]
pub struct PetriNetStats {
    pub place_count: usize,
    pub transition_count: usize,
    pub input_arc_count: usize,
    pub output_arc_count: usize,
    pub total_initial_tokens: usize,
}

/// 从 IrGraph 转换到 LabeledPetriNet
///
/// # 转换规则
///
/// 1. **节点分类**：
///    - 数据节点 → Place：Struct, Enum, Union, Constant, Static, Primitive, Tuple, Slice, Array, Variant, Generic, TypeAlias
///    - 操作节点 → Transition：Method, Function, UnwrapOp
///    - Trait 节点：根据使用方式可能是 Place（作为约束）或忽略
///
/// 2. **边转换**：
///    - 数据节点 → 操作节点：输入弧（消耗资源）
///    - 操作节点 → 数据节点：输出弧（产生资源）
///    - 数据节点 → 数据节点：通过虚拟 transition 连接（如字段访问）
///    - 操作节点 → 操作节点：忽略（不符合 Petri 网语义）
///
/// 3. **初始标记**：
///    - Constant/Static 节点：解析 init_value 为 token 数量
///    - 其他节点：默认 0
///
/// 4. **EdgeMode 映射**：
///    - Move：消耗性弧（权重 1）
///    - Ref/MutRef：非消耗性弧（需要守卫逻辑）
///    - Implements/Require：约束弧（用于守卫条件）
///    - Include/Alias/Instance：结构性弧
pub fn convert_ir_to_lpn(ir: &IrGraph) -> LabeledPetriNet {
    let mut lpn = LabeledPetriNet::new();

    // NodeIndex → (is_place, lpn_index) 的映射
    let mut node_mapping: HashMap<NodeIndex, (bool, usize)> = HashMap::new();

    // 第一步：遍历所有节点，分类为 place 或 transition
    for node_idx in ir.type_graph.node_indices() {
        let node_label = &ir.type_graph[node_idx];
        let node_type = ir.node_types.get(&node_idx);

        let is_place = match node_type {
            // 数据类型 → Place
            Some(NodeType::Struct)
            | Some(NodeType::Enum)
            | Some(NodeType::Union)
            | Some(NodeType::Constant)
            | Some(NodeType::Static)
            | Some(NodeType::Primitive)
            | Some(NodeType::Tuple)
            | Some(NodeType::Variant)
            | Some(NodeType::Generic)
            | Some(NodeType::TypeAlias)
            | Some(NodeType::Unit) => true,

            // Result/Option 包装类型 → Place
            Some(NodeType::ResultWrapper) | Some(NodeType::OptionWrapper) => true,

            // 操作类型 → Transition
            Some(NodeType::ImplMethod)
            | Some(NodeType::TraitMethod)
            | Some(NodeType::Function)
            | Some(NodeType::UnwrapOp) => false,

            // Trait 作为 Place（用于约束检查）
            Some(NodeType::Trait) => true,

            // 未知类型默认为 Place
            None => true,
        };

        let lpn_idx = if is_place {
            lpn.add_place(node_label.clone())
        } else {
            // 获取方法属性（is_const, is_async, is_unsafe）
            let attr = if let Some(node_info) = ir.node_infos.get(&node_idx) {
                match node_info {
                    NodeInfo::Method(method_info) => TransitionAttr {
                        is_const: method_info.is_const,
                        is_async: method_info.is_async,
                        is_unsafe: method_info.is_unsafe,
                    },
                    _ => TransitionAttr::default(),
                }
            } else {
                TransitionAttr::default()
            };
            lpn.add_transition_with_attr(node_label.clone(), attr)
        };

        node_mapping.insert(node_idx, (is_place, lpn_idx));
    }

    // 第二步：设置初始标记（从 ConstantInfo/StaticInfo）
    for (node_idx, node_info) in &ir.node_infos {
        if let Some(&(true, place_idx)) = node_mapping.get(node_idx) {
            let tokens = match node_info {
                NodeInfo::Constant(info) => parse_initial_token(&info.init_value),
                NodeInfo::Static(info) => parse_initial_token(&info.init_value),
                _ => 0,
            };
            if tokens > 0 {
                lpn.set_initial_marking(place_idx, tokens);
            }
        }
    }

    // 第三步：遍历边，创建弧
    for edge_ref in ir.type_graph.edge_references() {
        let source_idx = edge_ref.source();
        let target_idx = edge_ref.target();
        let relation = edge_ref.weight();

        let source_mapping = node_mapping.get(&source_idx);
        let target_mapping = node_mapping.get(&target_idx);

        if let (
            Some(&(source_is_place, source_lpn_idx)),
            Some(&(target_is_place, target_lpn_idx)),
        ) = (source_mapping, target_mapping)
        {
            match (source_is_place, target_is_place) {
                // Place → Transition：输入弧
                (true, false) => {
                    lpn.add_input_arc(
                        source_lpn_idx,
                        target_lpn_idx,
                        relation.mode,
                        1,
                        relation.label.clone(),
                    );
                }
                // Transition → Place：输出弧
                (false, true) => {
                    lpn.add_output_arc(
                        source_lpn_idx,
                        target_lpn_idx,
                        relation.mode,
                        1,
                        relation.label.clone(),
                    );
                }
                // Place → Place：创建虚拟 transition（如字段访问、类型包含）
                (true, true) => {
                    // 对于结构性关系（Include, Alias, Instance），创建虚拟 transition
                    if relation.mode.is_relationship() {
                        let virtual_trans_name = format!(
                            "{}_{}_{}",
                            &ir.type_graph[source_idx],
                            format!("{:?}", relation.mode).to_lowercase(),
                            &ir.type_graph[target_idx]
                        );
                        let trans_idx = lpn.add_transition(virtual_trans_name);
                        lpn.add_input_arc(
                            source_lpn_idx,
                            trans_idx,
                            relation.mode,
                            1,
                            relation.label.clone(),
                        );
                        lpn.add_output_arc(trans_idx, target_lpn_idx, relation.mode, 1, None);
                    }
                    // 对于数据流关系（Move, Ref 等），也创建虚拟 transition
                    else {
                        let virtual_trans_name = format!(
                            "access_{}_{}",
                            &ir.type_graph[source_idx],
                            relation.label.as_deref().unwrap_or("field")
                        );
                        let trans_idx = lpn.add_transition(virtual_trans_name);
                        lpn.add_input_arc(
                            source_lpn_idx,
                            trans_idx,
                            relation.mode,
                            1,
                            relation.label.clone(),
                        );
                        lpn.add_output_arc(trans_idx, target_lpn_idx, EdgeMode::Move, 1, None);
                    }
                }
                // Transition → Transition：忽略（不符合 Petri 网语义）
                (false, false) => {
                    // 可以记录日志或创建中间 place
                    log::debug!(
                        "Ignoring transition-to-transition edge: {} -> {}",
                        &ir.type_graph[source_idx],
                        &ir.type_graph[target_idx]
                    );
                }
            }
        }
    }

    lpn
}

/// 解析初始 token 值
///
/// 尝试将字符串解析为数字，失败则返回 1（表示存在一个实例）
fn parse_initial_token(value: &Option<String>) -> usize {
    match value {
        Some(s) => {
            // 尝试解析为数字
            s.trim().parse::<usize>().unwrap_or_else(|_| {
                // 如果不是数字但有值，返回 1
                if s.is_empty() { 0 } else { 1 }
            })
        }
        None => 0,
    }
}

/// 导出 Petri 网为 PNML 格式（Petri Net Markup Language）
impl LabeledPetriNet {
    /// 导出为 PNML XML 格式
    pub fn to_pnml(&self) -> String {
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

    /// 导出为 DOT 格式（用于 Graphviz 可视化）
    pub fn to_dot(&self) -> String {
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
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// 保存到文件
    pub fn save_to_file<P: AsRef<std::path::Path>>(
        &self,
        path: P,
        format: ExportFormat,
    ) -> std::io::Result<()> {
        let content = match format {
            ExportFormat::Pnml => self.to_pnml(),
            ExportFormat::Dot => self.to_dot(),
            ExportFormat::Json => self
                .to_json()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?,
        };
        std::fs::write(path, content)
    }
}

/// 导出格式
#[derive(Debug, Clone, Copy)]
pub enum ExportFormat {
    /// PNML (Petri Net Markup Language)
    Pnml,
    /// DOT (Graphviz)
    Dot,
    /// JSON
    Json,
}

/// XML 转义
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// DOT 转义
fn escape_dot(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_petri_net() {
        let lpn = LabeledPetriNet::new();
        assert_eq!(lpn.places.len(), 0);
        assert_eq!(lpn.transitions.len(), 0);
        assert_eq!(lpn.arcs.len(), 0);
    }

    #[test]
    fn test_add_place_and_transition() {
        let mut lpn = LabeledPetriNet::new();
        let p0 = lpn.add_place("Place0".to_string());
        let p1 = lpn.add_place("Place1".to_string());
        let t0 = lpn.add_transition("Trans0".to_string());

        assert_eq!(p0, 0);
        assert_eq!(p1, 1);
        assert_eq!(t0, 0);
        assert_eq!(lpn.places.len(), 2);
        assert_eq!(lpn.transitions.len(), 1);
        assert_eq!(lpn.initial_marking.len(), 2);
    }

    #[test]
    fn test_add_arcs() {
        let mut lpn = LabeledPetriNet::new();
        let p0 = lpn.add_place("Input".to_string());
        let t0 = lpn.add_transition("Process".to_string());
        let p1 = lpn.add_place("Output".to_string());

        lpn.add_input_arc(p0, t0, EdgeMode::Move, 1, Some("data".to_string()));
        lpn.add_output_arc(t0, p1, EdgeMode::Move, 1, None);

        assert_eq!(lpn.arcs.len(), 2);
        assert!(lpn.arcs[0].is_input_arc);
        assert!(!lpn.arcs[1].is_input_arc);
    }

    #[test]
    fn test_initial_marking() {
        let mut lpn = LabeledPetriNet::new();
        let p0 = lpn.add_place("Resource".to_string());
        lpn.set_initial_marking(p0, 5);

        assert_eq!(lpn.initial_marking[p0], 5);
    }

    #[test]
    fn test_parse_initial_token() {
        assert_eq!(parse_initial_token(&Some("42".to_string())), 42);
        assert_eq!(parse_initial_token(&Some("hello".to_string())), 1);
        assert_eq!(parse_initial_token(&Some("".to_string())), 0);
        assert_eq!(parse_initial_token(&None), 0);
    }

    #[test]
    fn test_stats() {
        let mut lpn = LabeledPetriNet::new();
        let p0 = lpn.add_place("P0".to_string());
        let p1 = lpn.add_place("P1".to_string());
        let t0 = lpn.add_transition("T0".to_string());

        lpn.set_initial_marking(p0, 3);
        lpn.add_input_arc(p0, t0, EdgeMode::Move, 1, None);
        lpn.add_output_arc(t0, p1, EdgeMode::Move, 1, None);

        let stats = lpn.stats();
        assert_eq!(stats.place_count, 2);
        assert_eq!(stats.transition_count, 1);
        assert_eq!(stats.input_arc_count, 1);
        assert_eq!(stats.output_arc_count, 1);
        assert_eq!(stats.total_initial_tokens, 3);
    }

    #[test]
    fn test_to_dot() {
        let mut lpn = LabeledPetriNet::new();
        let p0 = lpn.add_place("Input".to_string());
        let t0 = lpn.add_transition("Process".to_string());
        let p1 = lpn.add_place("Output".to_string());

        lpn.set_initial_marking(p0, 1);
        lpn.add_input_arc(p0, t0, EdgeMode::Move, 1, None);
        lpn.add_output_arc(t0, p1, EdgeMode::Move, 1, None);

        let dot = lpn.to_dot();
        assert!(dot.contains("digraph PetriNet"));
        assert!(dot.contains("p0"));
        assert!(dot.contains("t0"));
        assert!(dot.contains("p1"));
    }
}
