//! PCPN (Pushdown Colored Petri Net) - 下推着色 Petri 网
//!
//! 由 API Graph 转换而来：
//! - Place: 每种类型有三个库所 T[own], T[shr], T[mut]
//! - Transition: 每个函数对应一个变迁 + 结构性变迁 (borrow/drop)
//! - Token: 着色 token，颜色 = VarId
//!
//! 转换规则：
//! - API Graph 的类型节点 T → PCPN 的三个库所 T[own], T[shr], T[mut]
//! - API Graph 的函数节点 → PCPN 的变迁
//! - API Graph 的边 (PassingMode) → PCPN 的弧 + 结构性变迁

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::apigraph::{ApiGraph, EdgeDirection, FunctionNode};
use crate::type_model::{PassingMode, TypeKey};

/// Place 标识
pub type PlaceId = usize;

/// Transition 标识
pub type TransitionId = usize;

/// Capability (所有权/借用模式)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// 拥有所有权 (owned)
    Own,
    /// 共享借用 (&T)
    Shr,
    /// 可变借用 (&mut T)
    Mut,
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Capability::Own => write!(f, "own"),
            Capability::Shr => write!(f, "&"),
            Capability::Mut => write!(f, "&mut"),
        }
    }
}

/// PCPN Place (库所)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Place {
    /// Place 唯一标识
    pub id: PlaceId,
    /// 类型键 (base type)
    pub type_key: TypeKey,
    /// 该库所对应的 capability
    pub capability: Capability,
    /// 是否是 primitive 类型
    pub is_primitive: bool,
}

/// PCPN Transition (变迁)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transition {
    /// Transition 唯一标识
    pub id: TransitionId,
    /// 名称
    pub name: String,
    /// 变迁类型
    pub kind: TransitionKind,
    /// 输入弧
    pub input_arcs: Vec<Arc>,
    /// 输出弧
    pub output_arcs: Vec<Arc>,
    /// 守卫条件 (可选)
    pub guard: Option<String>,
}

/// 变迁类型
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionKind {
    /// API 调用
    ApiCall { fn_id: usize },
    /// 创建 primitive 常量
    CreatePrimitive { type_key: TypeKey },
    /// 共享借用: T[own] → T[own] + T[shr]
    BorrowShr { type_key: TypeKey },
    /// 可变借用: T[own] → T[mut]
    BorrowMut { type_key: TypeKey },
    /// 结束共享借用: T[shr] → ε
    EndBorrowShr { type_key: TypeKey },
    /// 结束可变借用: T[mut] → T[own]
    EndBorrowMut { type_key: TypeKey },
    /// Drop: T[own] → ε
    Drop { type_key: TypeKey },
}

/// 弧 (连接 Place 和 Transition)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Arc {
    /// 连接的 Place
    pub place_id: PlaceId,
    /// 是否消耗 token (false = 读取/测试弧)
    pub consumes: bool,
}

/// PCPN 网络
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pcpn {
    /// 所有库所
    pub places: Vec<Place>,
    /// 所有变迁
    pub transitions: Vec<Transition>,
    /// (类型, Capability) 到库所的映射
    #[serde(skip)]
    pub type_cap_to_place: HashMap<(TypeKey, Capability), PlaceId>,
    /// 初始标识 (primitive places)
    pub initial_places: Vec<PlaceId>,
}

impl Default for Pcpn {
    fn default() -> Self {
        Self::new()
    }
}

impl Pcpn {
    /// 创建空的 PCPN
    pub fn new() -> Self {
        Pcpn {
            places: Vec::new(),
            transitions: Vec::new(),
            type_cap_to_place: HashMap::new(),
            initial_places: Vec::new(),
        }
    }

    /// 从 ApiGraph 转换为 PCPN
    ///
    /// 转换规则：
    /// 1. 每个类型节点 T → 三个库所 T[own], T[shr], T[mut]
    /// 2. 每个函数节点 → 一个变迁
    /// 3. 边的 PassingMode 决定弧的连接方式
    pub fn from_api_graph(graph: &ApiGraph) -> Self {
        let mut pcpn = Pcpn::new();

        // 1. 为每个类型创建三个库所
        for type_node in &graph.type_nodes {
            pcpn.create_type_places(&type_node.type_key, type_node.is_primitive);
        }

        // 2. 为每个函数创建变迁
        for fn_node in &graph.fn_nodes {
            pcpn.create_function_transition(graph, fn_node);
        }

        // 3. 为非 primitive 类型添加结构性变迁
        let non_primitive_types: Vec<TypeKey> = graph
            .type_nodes
            .iter()
            .filter(|t| !t.is_primitive)
            .map(|t| t.type_key.clone())
            .collect();

        for type_key in non_primitive_types {
            pcpn.create_structural_transitions(&type_key);
        }

        // 4. 为 primitive 类型添加创建变迁
        for &place_id in &pcpn.initial_places.clone() {
            let type_key = pcpn.places[place_id].type_key.clone();
            pcpn.create_primitive_transition(&type_key, place_id);
        }

        pcpn
    }

    /// 为类型创建三个库所
    fn create_type_places(&mut self, type_key: &TypeKey, is_primitive: bool) {
        for cap in [Capability::Own, Capability::Shr, Capability::Mut] {
            let place_id = self.places.len();
            self.places.push(Place {
                id: place_id,
                type_key: type_key.clone(),
                capability: cap,
                is_primitive,
            });
            self.type_cap_to_place
                .insert((type_key.clone(), cap), place_id);

            // Primitive 类型的 own 库所作为初始 place
            if is_primitive && cap == Capability::Own {
                self.initial_places.push(place_id);
            }
        }
    }

    /// 为函数创建变迁
    fn create_function_transition(&mut self, graph: &ApiGraph, fn_node: &FunctionNode) {
        let trans_id = self.transitions.len();
        let mut input_arcs = Vec::new();
        let mut output_arcs = Vec::new();

        // 获取函数的边
        let input_edges = graph.get_input_edges(fn_node.id);
        let output_edges = graph.get_output_edges(fn_node.id);

        // 处理输入边
        for edge in input_edges {
            let type_key = &graph.type_nodes[edge.type_node].type_key;
            let (place_cap, consumes) = match edge.passing_mode {
                PassingMode::Move => (Capability::Own, true),
                PassingMode::Copy => (Capability::Own, false), // Copy 不消耗
                PassingMode::BorrowShr => (Capability::Shr, true),
                PassingMode::BorrowMut => (Capability::Mut, true),
                _ => continue,
            };

            if let Some(&place_id) = self.type_cap_to_place.get(&(type_key.clone(), place_cap)) {
                input_arcs.push(Arc { place_id, consumes });
            }
        }

        // 处理输出边
        for edge in output_edges {
            let type_key = &graph.type_nodes[edge.type_node].type_key;
            let place_cap = match edge.passing_mode {
                PassingMode::ReturnOwned => Capability::Own,
                PassingMode::ReturnBorrowShr => Capability::Shr,
                PassingMode::ReturnBorrowMut => Capability::Mut,
                _ => continue,
            };

            if let Some(&place_id) = self.type_cap_to_place.get(&(type_key.clone(), place_cap)) {
                output_arcs.push(Arc {
                    place_id,
                    consumes: false,
                });
            }
        }

        self.transitions.push(Transition {
            id: trans_id,
            name: fn_node.path.clone(), // 使用完整路径 (Type::method 或 function)
            kind: TransitionKind::ApiCall { fn_id: fn_node.id },
            input_arcs,
            output_arcs,
            guard: None,
        });
    }

    /// 创建结构性变迁
    fn create_structural_transitions(&mut self, type_key: &TypeKey) {
        let own_place = *self
            .type_cap_to_place
            .get(&(type_key.clone(), Capability::Own))
            .unwrap();
        let shr_place = *self
            .type_cap_to_place
            .get(&(type_key.clone(), Capability::Shr))
            .unwrap();
        let mut_place = *self
            .type_cap_to_place
            .get(&(type_key.clone(), Capability::Mut))
            .unwrap();

        let short_name = type_key.short_name();

        // BorrowShr: T[own] → T[own] + T[shr]
        self.add_transition(
            format!("&{}", short_name),
            TransitionKind::BorrowShr {
                type_key: type_key.clone(),
            },
            vec![Arc {
                place_id: own_place,
                consumes: false,
            }],
            vec![Arc {
                place_id: shr_place,
                consumes: false,
            }],
            Some("can_borrow_shr".to_string()),
        );

        // BorrowMut: T[own] → T[mut]
        self.add_transition(
            format!("&mut {}", short_name),
            TransitionKind::BorrowMut {
                type_key: type_key.clone(),
            },
            vec![Arc {
                place_id: own_place,
                consumes: true,
            }],
            vec![Arc {
                place_id: mut_place,
                consumes: false,
            }],
            Some("can_borrow_mut".to_string()),
        );

        // EndBorrowShr: T[shr] → ε
        self.add_transition(
            format!("end &{}", short_name),
            TransitionKind::EndBorrowShr {
                type_key: type_key.clone(),
            },
            vec![Arc {
                place_id: shr_place,
                consumes: true,
            }],
            vec![],
            None,
        );

        // EndBorrowMut: T[mut] → T[own]
        self.add_transition(
            format!("end &mut {}", short_name),
            TransitionKind::EndBorrowMut {
                type_key: type_key.clone(),
            },
            vec![Arc {
                place_id: mut_place,
                consumes: true,
            }],
            vec![Arc {
                place_id: own_place,
                consumes: false,
            }],
            None,
        );

        // Drop: T[own] → ε
        self.add_transition(
            format!("drop {}", short_name),
            TransitionKind::Drop {
                type_key: type_key.clone(),
            },
            vec![Arc {
                place_id: own_place,
                consumes: true,
            }],
            vec![],
            Some("can_drop".to_string()),
        );
    }

    /// 创建 primitive 常量变迁
    fn create_primitive_transition(&mut self, type_key: &TypeKey, place_id: PlaceId) {
        self.add_transition(
            format!("const {}", type_key.short_name()),
            TransitionKind::CreatePrimitive {
                type_key: type_key.clone(),
            },
            vec![],
            vec![Arc {
                place_id,
                consumes: false,
            }],
            None,
        );
    }

    /// 添加变迁
    fn add_transition(
        &mut self,
        name: String,
        kind: TransitionKind,
        input_arcs: Vec<Arc>,
        output_arcs: Vec<Arc>,
        guard: Option<String>,
    ) {
        let id = self.transitions.len();
        self.transitions.push(Transition {
            id,
            name,
            kind,
            input_arcs,
            output_arcs,
            guard,
        });
    }

    /// 生成 DOT 格式
    pub fn to_dot(&self) -> String {
        let mut dot = String::new();
        dot.push_str("digraph PCPN {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  fontname=\"Helvetica\";\n");
        dot.push_str("  node [fontname=\"Helvetica\"];\n");
        dot.push_str("  edge [fontname=\"Helvetica\"];\n");
        dot.push_str("  // PCPN: T[own], T[shr], T[mut] places per type\n\n");

        // Places
        dot.push_str("  // ========== Places ==========\n");
        for place in &self.places {
            let (fillcolor, peripheries) = match place.capability {
                Capability::Own => {
                    if place.is_primitive {
                        ("lightgray", 2)
                    } else {
                        ("lightblue", 2)
                    }
                }
                Capability::Shr => ("lightcyan", 1),
                Capability::Mut => ("mistyrose", 1),
            };

            let label = format!("{}[{}]", place.type_key.short_name(), place.capability);

            dot.push_str(&format!(
                "  p{} [label=\"{}\", shape=circle, style=filled, fillcolor={}, peripheries={}];\n",
                place.id, label, fillcolor, peripheries
            ));
        }
        dot.push_str("\n");

        // Transitions
        dot.push_str("  // ========== Transitions ==========\n");
        for trans in &self.transitions {
            let (color, shape) = match &trans.kind {
                TransitionKind::ApiCall { .. } => ("palegreen", "box"),
                TransitionKind::CreatePrimitive { .. } => ("lightcyan", "diamond"),
                TransitionKind::BorrowShr { .. } => ("lavender", "box"),
                TransitionKind::BorrowMut { .. } => ("pink", "box"),
                TransitionKind::EndBorrowShr { .. } | TransitionKind::EndBorrowMut { .. } => {
                    ("honeydew", "box")
                }
                TransitionKind::Drop { .. } => ("gray90", "box"),
            };

            let label = if let Some(guard) = &trans.guard {
                format!("{}\\n[{}]", trans.name, guard)
            } else {
                trans.name.clone()
            };

            dot.push_str(&format!(
                "  t{} [label=\"{}\", shape={}, style=filled, fillcolor={}];\n",
                trans.id, label, shape, color
            ));
        }
        dot.push_str("\n");

        // Arcs
        dot.push_str("  // ========== Arcs ==========\n");
        for trans in &self.transitions {
            for arc in &trans.input_arcs {
                let style = if arc.consumes { "solid" } else { "dashed" };
                let arrow = if arc.consumes { "normal" } else { "odot" };
                let color = Self::cap_color(self.places[arc.place_id].capability);

                dot.push_str(&format!(
                    "  p{} -> t{} [style={}, arrowhead={}, color=\"{}\"];\n",
                    arc.place_id, trans.id, style, arrow, color
                ));
            }

            for arc in &trans.output_arcs {
                let color = Self::cap_color(self.places[arc.place_id].capability);
                dot.push_str(&format!(
                    "  t{} -> p{} [color=\"{}\"];\n",
                    trans.id, arc.place_id, color
                ));
            }
        }

        dot.push_str("}\n");
        dot
    }

    fn cap_color(cap: Capability) -> &'static str {
        match cap {
            Capability::Own => "black",
            Capability::Shr => "blue",
            Capability::Mut => "red",
        }
    }

    /// 统计信息
    pub fn stats(&self) -> PcpnStats {
        let api_trans = self
            .transitions
            .iter()
            .filter(|t| matches!(t.kind, TransitionKind::ApiCall { .. }))
            .count();
        let structural_trans = self.transitions.len() - api_trans;

        PcpnStats {
            num_places: self.places.len(),
            num_transitions: self.transitions.len(),
            num_api_transitions: api_trans,
            num_structural_transitions: structural_trans,
            num_primitive_places: self.initial_places.len(),
        }
    }
}

/// PCPN 统计
#[derive(Debug)]
pub struct PcpnStats {
    pub num_places: usize,
    pub num_transitions: usize,
    pub num_api_transitions: usize,
    pub num_structural_transitions: usize,
    pub num_primitive_places: usize,
}

impl std::fmt::Display for PcpnStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PCPN: {} places ({} primitive), {} transitions ({} API, {} structural)",
            self.num_places,
            self.num_primitive_places,
            self.num_transitions,
            self.num_api_transitions,
            self.num_structural_transitions
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apigraph::{ApiEdge, ApiGraph, FunctionNode, TypeNode};

    #[test]
    fn test_pcpn_from_api_graph() {
        let mut graph = ApiGraph::new();

        // 添加类型节点
        graph.type_nodes.push(TypeNode {
            id: 0,
            type_key: TypeKey::path("Counter"),
            is_primitive: false,
            is_copy: false,
        });
        graph.type_to_node.insert(TypeKey::path("Counter"), 0);

        // 添加函数节点
        graph.fn_nodes.push(FunctionNode {
            id: 0,
            path: "Counter::new".to_string(),
            name: "new".to_string(),
            is_method: false,
            is_entry: true,
            params: vec![],
            self_param: None,
            return_type: Some(TypeKey::path("Counter")),
            return_mode: Some(PassingMode::ReturnOwned),
        });

        // 添加边
        graph.edges.push(ApiEdge {
            fn_node: 0,
            type_node: 0,
            direction: EdgeDirection::Output,
            passing_mode: PassingMode::ReturnOwned,
            param_index: None,
        });

        let pcpn = Pcpn::from_api_graph(&graph);

        // 检查：1 类型 × 3 capabilities = 3 places
        assert_eq!(pcpn.places.len(), 3);
        // 检查：1 API + 5 structural = 6 transitions
        assert_eq!(pcpn.transitions.len(), 6);
    }
}
