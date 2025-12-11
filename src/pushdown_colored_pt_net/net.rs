use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::ir_graph::{EdgeMode, IrGraph, NodeInfo, NodeType};
use crate::petri_net_traits::{FromIrGraph, PetriNetKind};

/// Token 颜色（类型）
///
/// 在着色 Petri 网中，token 有不同的颜色，表示不同的类型
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum TokenColor {
    /// 基本类型（如 u8, i32, bool, str）
    Primitive(String),
    /// 复合类型（如 Vec<T>, Option<T>）
    Composite {
        /// 类型名称
        name: String,
        /// 类型参数（泛型实例化）
        type_args: Vec<TokenColor>,
    },
    /// 泛型参数（未实例化）
    Generic {
        /// 泛型参数名
        name: String,
        /// 所属作用域
        scope: String,
    },
    /// 关联类型
    AssociatedType {
        /// 所属类型或 Trait
        owner: String,
        /// 关联类型名
        assoc_name: String,
    },
    /// 元组类型
    Tuple(Vec<TokenColor>),
    /// 引用类型
    Reference {
        /// 是否可变
        mutable: bool,
        /// 引用的类型
        inner: Box<TokenColor>,
    },
}

impl TokenColor {
    /// 转换为字符串表示
    pub fn to_string(&self) -> String {
        match self {
            TokenColor::Primitive(name) => name.clone(),
            TokenColor::Composite { name, type_args } => {
                if type_args.is_empty() {
                    name.clone()
                } else {
                    let args_str = type_args
                        .iter()
                        .map(|c| c.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{}<{}>", name, args_str)
                }
            }
            TokenColor::Generic { name, scope } => format!("{}@{}", name, scope),
            TokenColor::AssociatedType { owner, assoc_name } => {
                format!("{}::{}", owner, assoc_name)
            }
            TokenColor::Tuple(types) => {
                let types_str = types
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({})", types_str)
            }
            TokenColor::Reference { mutable, inner } => {
                let mut_str = if *mutable { "mut " } else { "" };
                format!("&{} {}", mut_str, inner.to_string())
            }
        }
    }
}

/// 栈操作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StackOperation {
    /// 无栈操作
    None,
    /// Push：将元素压入栈
    Push,
    /// Pop：从栈中弹出元素
    Pop,
    /// PushPop：先 push 再 pop（用于作用域进入和退出）
    PushPop,
}

/// 下推着色 Petri 网结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushdownColoredPetriNet {
    /// 库所列表（数据类型节点）
    pub places: Vec<String>,
    /// 变迁列表（操作节点）
    pub transitions: Vec<String>,
    /// 变迁属性
    pub transition_attrs: Vec<TransitionAttr>,
    /// 弧列表，带有颜色约束
    pub arcs: Vec<ColoredArc>,
    /// 初始标记：每个 place 的初始 token（按颜色分组）
    /// HashMap<place_idx, HashMap<color, count>>
    pub initial_marking: HashMap<usize, HashMap<TokenColor, usize>>,
    /// 变迁的栈操作
    /// HashMap<transition_idx, StackOperation>
    pub stack_operations: HashMap<usize, StackOperation>,
    /// Transition 到原 IrGraph NodeIndex 的映射
    pub trans_to_node: HashMap<usize, NodeIndex>,
    /// Place 到原 IrGraph NodeIndex 的映射
    pub place_to_node: HashMap<usize, NodeIndex>,
    /// 颜色定义：所有可能出现的 token 颜色
    pub color_set: HashSet<TokenColor>,
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

/// 带颜色约束的弧
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColoredArc {
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
    /// Token 颜色约束：None 表示接受任何颜色，Some 表示只接受特定颜色
    pub color_constraint: Option<TokenColor>,
}

impl PushdownColoredPetriNet {
    /// 创建空的下推着色 Petri 网
    pub fn new() -> Self {
        Self {
            places: Vec::new(),
            transitions: Vec::new(),
            transition_attrs: Vec::new(),
            arcs: Vec::new(),
            initial_marking: HashMap::new(),
            stack_operations: HashMap::new(),
            trans_to_node: HashMap::new(),
            place_to_node: HashMap::new(),
            color_set: HashSet::new(),
        }
    }

    /// 添加一个 place，返回其索引
    pub fn add_place(&mut self, name: String) -> usize {
        let idx = self.places.len();
        self.places.push(name);
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

    /// 设置变迁的栈操作
    pub fn set_stack_operation(&mut self, transition_idx: usize, op: StackOperation) {
        self.stack_operations.insert(transition_idx, op);
    }

    /// 添加输入弧（place → transition），带颜色约束
    pub fn add_input_arc(
        &mut self,
        place_idx: usize,
        transition_idx: usize,
        label: EdgeMode,
        weight: usize,
        name: Option<String>,
        color_constraint: Option<TokenColor>,
    ) {
        if let Some(ref color) = color_constraint {
            self.color_set.insert(color.clone());
        }
        self.arcs.push(ColoredArc {
            from_idx: place_idx,
            to_idx: transition_idx,
            is_input_arc: true,
            label,
            weight,
            name,
            color_constraint,
        });
    }

    /// 添加输出弧（transition → place），带颜色约束
    pub fn add_output_arc(
        &mut self,
        transition_idx: usize,
        place_idx: usize,
        label: EdgeMode,
        weight: usize,
        name: Option<String>,
        color_constraint: Option<TokenColor>,
    ) {
        if let Some(ref color) = color_constraint {
            self.color_set.insert(color.clone());
        }
        self.arcs.push(ColoredArc {
            from_idx: transition_idx,
            to_idx: place_idx,
            is_input_arc: false,
            label,
            weight,
            name,
            color_constraint,
        });
    }

    /// 设置 place 的初始标记（特定颜色的 token）
    pub fn set_initial_marking(
        &mut self,
        place_idx: usize,
        color: TokenColor,
        count: usize,
    ) {
        self.color_set.insert(color.clone());
        self.initial_marking
            .entry(place_idx)
            .or_insert_with(HashMap::new)
            .insert(color, count);
    }

    /// 获取统计信息
    pub fn stats(&self) -> PcpnStats {
        let input_arcs = self.arcs.iter().filter(|a| a.is_input_arc).count();
        let output_arcs = self.arcs.iter().filter(|a| !a.is_input_arc).count();
        let total_tokens: usize = self
            .initial_marking
            .values()
            .flat_map(|colors| colors.values())
            .sum();
        let transitions_with_stack = self.stack_operations.len();

        PcpnStats {
            place_count: self.places.len(),
            transition_count: self.transitions.len(),
            input_arc_count: input_arcs,
            output_arc_count: output_arcs,
            total_initial_tokens: total_tokens,
            color_count: self.color_set.len(),
            transitions_with_stack,
        }
    }
}

impl Default for PushdownColoredPetriNet {
    fn default() -> Self {
        Self::new()
    }
}

/// 下推着色 Petri 网统计信息
#[derive(Debug, Clone)]
pub struct PcpnStats {
    pub place_count: usize,
    pub transition_count: usize,
    pub input_arc_count: usize,
    pub output_arc_count: usize,
    pub total_initial_tokens: usize,
    pub color_count: usize,
    pub transitions_with_stack: usize,
}

/// 从 IrGraph 转换到 PushdownColoredPetriNet
///
/// # 转换规则
///
/// 1. **节点分类**:
///    - 数据节点 → Place
///    - 操作节点 → Transition
///
/// 2. **颜色系统**:
///    - 从节点类型和泛型参数推断 token 颜色
///    - 基本类型 → Primitive color
///    - 复合类型 → Composite color with type args
///    - 泛型参数 → Generic color
///
/// 3. **栈操作**:
///    - 函数调用 → Push（进入作用域）
///    - 函数返回 → Pop（退出作用域）
///    - 泛型实例化 → Push（压入类型参数）
///
/// 4. **弧的颜色约束**:
///    - 根据边的类型信息设置颜色约束
///    - Move/Ref 边：根据源节点的类型设置颜色
impl FromIrGraph for PushdownColoredPetriNet {
    fn from_ir_graph(ir: &IrGraph) -> Self {
        let mut pcpn = PushdownColoredPetriNet::new();

        // NodeIndex → (is_place, pcpn_index) 的映射
        let mut node_mapping: HashMap<NodeIndex, (bool, usize)> = HashMap::new();
        // NodeIndex → TokenColor 的映射（用于推断颜色）
        let mut node_colors: HashMap<NodeIndex, TokenColor> = HashMap::new();

        // 第一步：遍历所有节点，分类为 place 或 transition，并推断颜色
        for node_idx in ir.type_graph.node_indices() {
            let node_label = &ir.type_graph[node_idx];
            let node_type = ir.node_types.get(&node_idx);

            let is_place = match node_type {
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
                Some(NodeType::ResultWrapper) | Some(NodeType::OptionWrapper) => true,
                Some(NodeType::ImplMethod)
                | Some(NodeType::TraitMethod)
                | Some(NodeType::Function)
                | Some(NodeType::UnwrapOp) => false,
                Some(NodeType::Trait) => true,
                None => true,
            };

            // 推断节点的颜色
            let color = infer_token_color(ir, node_idx, node_type, node_label);
            if is_place {
                node_colors.insert(node_idx, color.clone());
            }

            let pcpn_idx = if is_place {
                let idx = pcpn.add_place(node_label.clone());
                pcpn.place_to_node.insert(idx, node_idx);
                idx
            } else {
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
                let idx = pcpn.add_transition_with_attr(node_label.clone(), attr);
                pcpn.trans_to_node.insert(idx, node_idx);
                idx
            };

            node_mapping.insert(node_idx, (is_place, pcpn_idx));
        }

        // 第二步：设置初始标记（从 ConstantInfo/StaticInfo）
        for (node_idx, node_info) in &ir.node_infos {
            if let Some(&(true, place_idx)) = node_mapping.get(node_idx) {
                if let Some(color) = node_colors.get(node_idx) {
                    let tokens = match node_info {
                        NodeInfo::Constant(info) => parse_initial_token(&info.init_value),
                        NodeInfo::Static(info) => parse_initial_token(&info.init_value),
                        _ => 0,
                    };
                    if tokens > 0 {
                        pcpn.set_initial_marking(place_idx, color.clone(), tokens);
                    }
                }
            }
        }

        // 第三步：遍历边，创建带颜色约束的弧
        for edge_ref in ir.type_graph.edge_references() {
            let source_idx = edge_ref.source();
            let target_idx = edge_ref.target();
            let relation = edge_ref.weight();

            let source_mapping = node_mapping.get(&source_idx);
            let target_mapping = node_mapping.get(&target_idx);

            if let (
                Some(&(source_is_place, source_pcpn_idx)),
                Some(&(target_is_place, target_pcpn_idx)),
            ) = (source_mapping, target_mapping)
            {
                // 获取源节点的颜色（用于设置弧的颜色约束）
                let source_color = node_colors.get(&source_idx).cloned();

                match (source_is_place, target_is_place) {
                    // Place → Transition:输入弧
                    (true, false) => {
                        pcpn.add_input_arc(
                            source_pcpn_idx,
                            target_pcpn_idx,
                            relation.mode,
                            1,
                            relation.label.clone(),
                            source_color,
                        );
                    }
                    // Transition → Place:输出弧
                    (false, true) => {
                        // 对于输出弧，颜色约束来自目标节点
                        let target_color = node_colors.get(&target_idx).cloned();
                        pcpn.add_output_arc(
                            source_pcpn_idx,
                            target_pcpn_idx,
                            relation.mode,
                            1,
                            relation.label.clone(),
                            target_color,
                        );
                    }
                    // Place → Place:创建虚拟 transition
                    (true, true) => {
                        if relation.mode.is_relationship() {
                            let virtual_trans_name = format!(
                                "{}_{}_{}",
                                &ir.type_graph[source_idx],
                                format!("{:?}", relation.mode).to_lowercase(),
                                &ir.type_graph[target_idx]
                            );
                            let trans_idx = pcpn.add_transition(virtual_trans_name);
                            pcpn.add_input_arc(
                                source_pcpn_idx,
                                trans_idx,
                                relation.mode,
                                1,
                                relation.label.clone(),
                                source_color,
                            );
                            let target_color = node_colors.get(&target_idx).cloned();
                            pcpn.add_output_arc(trans_idx, target_pcpn_idx, relation.mode, 1, None, target_color);
                        }
                    }
                    // Transition → Transition:忽略
                    (false, false) => {
                        log::debug!(
                            "Ignoring transition-to-transition edge: {} -> {}",
                            &ir.type_graph[source_idx],
                            &ir.type_graph[target_idx]
                        );
                    }
                }
            }
        }

        // 第四步：为函数调用设置栈操作
        let mut stack_ops: Vec<(usize, StackOperation)> = Vec::new();
        for (trans_idx, &node_idx) in &pcpn.trans_to_node {
            if let Some(node_info) = ir.node_infos.get(&node_idx) {
                match node_info {
                    NodeInfo::Method(_) | NodeInfo::Function(_) => {
                        // 函数调用需要 Push（进入作用域）
                        stack_ops.push((*trans_idx, StackOperation::Push));
                    }
                    _ => {}
                }
            }
        }
        for (trans_idx, op) in stack_ops {
            pcpn.set_stack_operation(trans_idx, op);
        }

        pcpn
    }
}

/// 推断节点的 token 颜色
fn infer_token_color(
    ir: &IrGraph,
    node_idx: NodeIndex,
    node_type: Option<&NodeType>,
    node_label: &str,
) -> TokenColor {
    // 尝试从 NodeInfo 获取类型信息
    if let Some(node_info) = ir.node_infos.get(&node_idx) {
        match node_info {
            NodeInfo::Primitive(prim_info) => {
                return TokenColor::Primitive(prim_info.name.clone());
            }
            NodeInfo::Struct(struct_info) => {
                // 简化：使用结构体名称，实际应该解析泛型参数
                return TokenColor::Composite {
                    name: struct_info.path.name.clone(),
                    type_args: Vec::new(), // TODO: 解析泛型参数
                };
            }
            NodeInfo::Enum(enum_info) => {
                return TokenColor::Composite {
                    name: enum_info.path.name.clone(),
                    type_args: Vec::new(),
                };
            }
            _ => {}
        }
    }

    // 根据节点类型推断
    match node_type {
        Some(NodeType::Primitive) => TokenColor::Primitive(node_label.to_string()),
        Some(NodeType::Generic) => TokenColor::Generic {
            name: node_label.to_string(),
            scope: "unknown".to_string(), // TODO: 从上下文获取作用域
        },
        _ => TokenColor::Composite {
            name: node_label.to_string(),
            type_args: Vec::new(),
        },
    }
}

/// 解析初始 token 值
fn parse_initial_token(value: &Option<String>) -> usize {
    match value {
        Some(s) => {
            s.trim().parse::<usize>().unwrap_or_else(|_| {
                if s.is_empty() {
                    0
                } else {
                    1
                }
            })
        }
        None => 0,
    }
}

impl PetriNetKind for PushdownColoredPetriNet {
    fn kind_name() -> &'static str {
        "PushdownColoredPetriNet"
    }

    fn description() -> &'static str {
        "Pushdown Colored Petri Net (PCPN) with type colors and stack operations"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_color_to_string() {
        let prim = TokenColor::Primitive("u8".to_string());
        assert_eq!(prim.to_string(), "u8");

        let comp = TokenColor::Composite {
            name: "Vec".to_string(),
            type_args: vec![TokenColor::Primitive("u8".to_string())],
        };
        assert_eq!(comp.to_string(), "Vec<u8>");

        let tuple = TokenColor::Tuple(vec![
            TokenColor::Primitive("u8".to_string()),
            TokenColor::Primitive("bool".to_string()),
        ]);
        assert_eq!(tuple.to_string(), "(u8, bool)");
    }

    #[test]
    fn test_empty_pcpn() {
        let pcpn = PushdownColoredPetriNet::new();
        assert_eq!(pcpn.places.len(), 0);
        assert_eq!(pcpn.transitions.len(), 0);
        assert_eq!(pcpn.arcs.len(), 0);
        assert_eq!(pcpn.color_set.len(), 0);
    }

    #[test]
    fn test_add_place_and_transition() {
        let mut pcpn = PushdownColoredPetriNet::new();
        let p0 = pcpn.add_place("Place0".to_string());
        let t0 = pcpn.add_transition("Trans0".to_string());

        assert_eq!(p0, 0);
        assert_eq!(t0, 0);
        assert_eq!(pcpn.places.len(), 1);
        assert_eq!(pcpn.transitions.len(), 1);
    }

    #[test]
    fn test_colored_arcs() {
        let mut pcpn = PushdownColoredPetriNet::new();
        let p0 = pcpn.add_place("Input".to_string());
        let t0 = pcpn.add_transition("Process".to_string());
        let p1 = pcpn.add_place("Output".to_string());

        let color = TokenColor::Primitive("u8".to_string());
        pcpn.add_input_arc(p0, t0, EdgeMode::Move, 1, None, Some(color.clone()));
        pcpn.add_output_arc(t0, p1, EdgeMode::Move, 1, None, Some(color.clone()));

        assert_eq!(pcpn.arcs.len(), 2);
        assert!(pcpn.arcs[0].is_input_arc);
        assert!(!pcpn.arcs[1].is_input_arc);
        assert_eq!(pcpn.color_set.len(), 1);
    }

    #[test]
    fn test_stack_operation() {
        let mut pcpn = PushdownColoredPetriNet::new();
        let t0 = pcpn.add_transition("Function".to_string());
        pcpn.set_stack_operation(t0, StackOperation::Push);

        assert_eq!(pcpn.stack_operations.get(&t0), Some(&StackOperation::Push));
    }
}
