//! PCPN (Parameterized Colored Petri Net) - 参数化着色 Petri 网
//!
//! 核心结构：
//! - Place: 每种类型 T 有三个库所：T[own], T[shr], T[mut]
//!   - T[own]: 持有所有权的 token
//!   - T[shr]: 共享引用 token
//!   - T[mut]: 可变引用 token
//! - Token: 着色 token，颜色 = VarId，定义在 model::Token
//! - Transition: API 变迁或结构性变迁
//! - Arc: 连接 Place 和 Transition
//!
//! 借用语义（正确建模）：
//! - borrow_shr: T[own] → T[own] + T[shr]（owner 不消失，产生引用）
//! - borrow_mut: T[own] → T[mut]（owner 冻结，产生可变引用）
//! - end_shr: T[shr] → ε（引用结束）
//! - end_mut: T[mut] → T[own]（归还可变引用，恢复 owner）
//! - drop: T[own] → ε（消耗所有权）

use indexmap::{IndexMap, IndexSet};
use std::collections::HashMap;

use crate::api_extract::{ApiSignature, ParamMode, ReturnMode};
use crate::api_graph::{ApiGraph, ApiNode, ApiSource};
use crate::model::{Capability, TypeKey};

/// PCPN Place (库所)
/// 每种类型有三个库所：own, shr, mut
#[derive(Debug, Clone)]
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

/// Place 标识
pub type PlaceId = usize;

/// Transition 标识
pub type TransitionId = usize;

/// PCPN Transition (变迁)
#[derive(Debug, Clone)]
pub struct PcpnTransition {
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionKind {
    /// API 调用
    ApiCall { api_index: usize, source: ApiSource },
    /// 创建 primitive 常量
    CreatePrimitive { type_key: TypeKey },
    /// 共享借用: own -> own + shr
    BorrowShr { type_key: TypeKey },
    /// 可变借用: own -> mut (own 被冻结)
    BorrowMut { type_key: TypeKey },
    /// 结束共享借用: shr + own -> own
    EndBorrowShr { type_key: TypeKey },
    /// 结束可变借用: mut -> own (解冻)
    EndBorrowMut { type_key: TypeKey },
    /// Drop: own -> (消除)
    Drop { type_key: TypeKey },
}

/// 弧 (连接 Place 和 Transition)
#[derive(Debug, Clone)]
pub struct Arc {
    /// 连接的 Place (库所已经区分了 capability)
    pub place_id: PlaceId,
    /// 权重 (通常为 1)
    pub weight: usize,
    /// 是否消耗 token (false = 读取/测试弧)
    pub consumes: bool,
}

/// PCPN 网络
#[derive(Debug)]
pub struct Pcpn {
    /// 所有库所
    pub places: Vec<Place>,
    /// 所有变迁
    pub transitions: Vec<PcpnTransition>,
    /// (类型, Capability) 到库所的映射
    pub type_cap_to_place: HashMap<(TypeKey, Capability), PlaceId>,
    /// API 到变迁的映射
    pub api_to_transition: HashMap<usize, TransitionId>,
    /// 初始标识 (primitive places 可以无限产生 tokens)
    pub initial_places: Vec<PlaceId>,
}

impl Pcpn {
    /// 从 ApiGraph 构建 PCPN
    /// 每种类型创建三个库所：own, shr, mut
    pub fn from_api_graph(graph: &ApiGraph) -> Self {
        let mut pcpn = Pcpn {
            places: Vec::new(),
            transitions: Vec::new(),
            type_cap_to_place: HashMap::new(),
            api_to_transition: HashMap::new(),
            initial_places: Vec::new(),
        };

        // 1. 收集所有类型
        let mut all_types = IndexSet::new();
        for node in &graph.nodes {
            for (ty, _) in &node.inputs {
                all_types.insert(ty.clone());
            }
            for (ty, _) in &node.outputs {
                all_types.insert(ty.clone());
            }
        }

        // 2. 为每种类型创建三个库所: own, shr, mut
        for type_key in &all_types {
            let is_primitive = Self::is_primitive(type_key);

            for cap in [Capability::Own, Capability::Shr, Capability::Mut] {
                let place_id = pcpn.places.len();
                pcpn.places.push(Place {
                    id: place_id,
                    type_key: type_key.clone(),
                    capability: cap,
                    is_primitive,
                });
                pcpn.type_cap_to_place
                    .insert((type_key.clone(), cap), place_id);

                // Primitive 类型的 own 库所可以作为初始 place
                if is_primitive && cap == Capability::Own {
                    pcpn.initial_places.push(place_id);
                }
            }
        }

        // 3. 为每个 API 创建 Transition
        for node in &graph.nodes {
            let trans_id = pcpn.transitions.len();

            // 构建输入弧 - 根据 capability 连接到对应的库所
            let input_arcs: Vec<Arc> = node
                .inputs
                .iter()
                .map(|(ty, cap)| {
                    let place_id = *pcpn.type_cap_to_place.get(&(ty.clone(), *cap)).unwrap();
                    Arc {
                        place_id,
                        weight: 1,
                        // shr/mut 引用在使用后消耗，own 要看是否 move
                        consumes: true,
                    }
                })
                .collect();

            // 构建输出弧
            let output_arcs: Vec<Arc> = node
                .outputs
                .iter()
                .map(|(ty, cap)| {
                    let place_id = *pcpn.type_cap_to_place.get(&(ty.clone(), *cap)).unwrap();
                    Arc {
                        place_id,
                        weight: 1,
                        consumes: false,
                    }
                })
                .collect();

            pcpn.transitions.push(PcpnTransition {
                id: trans_id,
                name: Self::format_transition_name(&node.api, &node.source),
                kind: TransitionKind::ApiCall {
                    api_index: node.index,
                    source: node.source.clone(),
                },
                input_arcs,
                output_arcs,
                guard: None,
            });
            pcpn.api_to_transition.insert(node.index, trans_id);
        }

        // 4. 为每种非 primitive 类型添加结构性变迁
        let non_primitive_types: Vec<TypeKey> = all_types
            .iter()
            .filter(|ty| !Self::is_primitive(ty))
            .cloned()
            .collect();

        for type_key in non_primitive_types {
            let own_place = *pcpn
                .type_cap_to_place
                .get(&(type_key.clone(), Capability::Own))
                .unwrap();
            let shr_place = *pcpn
                .type_cap_to_place
                .get(&(type_key.clone(), Capability::Shr))
                .unwrap();
            let mut_place = *pcpn
                .type_cap_to_place
                .get(&(type_key.clone(), Capability::Mut))
                .unwrap();

            // BorrowShr: T[own] -> T[own] + T[shr] (owner 不消失，产生引用)
            pcpn.add_structural_transition(
                TransitionKind::BorrowShr {
                    type_key: type_key.clone(),
                },
                format!("&{}", Self::simplify_type(&type_key)),
                vec![Arc {
                    place_id: own_place,
                    weight: 1,
                    consumes: false, // 读取 own，不消耗
                }],
                vec![Arc {
                    place_id: shr_place,
                    weight: 1,
                    consumes: false,
                }],
                Some("can_borrow_shr".to_string()),
            );

            // BorrowMut: T[own] -> T[mut] (owner 冻结)
            pcpn.add_structural_transition(
                TransitionKind::BorrowMut {
                    type_key: type_key.clone(),
                },
                format!("&mut {}", Self::simplify_type(&type_key)),
                vec![Arc {
                    place_id: own_place,
                    weight: 1,
                    consumes: true, // 消耗 own (冻结)
                }],
                vec![Arc {
                    place_id: mut_place,
                    weight: 1,
                    consumes: false,
                }],
                Some("can_borrow_mut".to_string()),
            );

            // EndBorrowShr: T[shr] -> ε (引用结束)
            pcpn.add_structural_transition(
                TransitionKind::EndBorrowShr {
                    type_key: type_key.clone(),
                },
                format!("end &{}", Self::simplify_type(&type_key)),
                vec![Arc {
                    place_id: shr_place,
                    weight: 1,
                    consumes: true,
                }],
                vec![],
                None,
            );

            // EndBorrowMut: T[mut] -> T[own] (归还可变引用，恢复 owner)
            pcpn.add_structural_transition(
                TransitionKind::EndBorrowMut {
                    type_key: type_key.clone(),
                },
                format!("end &mut {}", Self::simplify_type(&type_key)),
                vec![Arc {
                    place_id: mut_place,
                    weight: 1,
                    consumes: true,
                }],
                vec![Arc {
                    place_id: own_place,
                    weight: 1,
                    consumes: false,
                }],
                None,
            );

            // Drop: T[own] -> ε (消耗所有权)
            pcpn.add_structural_transition(
                TransitionKind::Drop {
                    type_key: type_key.clone(),
                },
                format!("drop {}", Self::simplify_type(&type_key)),
                vec![Arc {
                    place_id: own_place,
                    weight: 1,
                    consumes: true,
                }],
                vec![],
                Some("can_drop".to_string()),
            );
        }

        // 5. 为 primitive 类型添加创建变迁
        for &place_id in &pcpn.initial_places.clone() {
            let type_key = pcpn.places[place_id].type_key.clone();
            pcpn.add_structural_transition(
                TransitionKind::CreatePrimitive {
                    type_key: type_key.clone(),
                },
                format!("const {}", Self::simplify_type(&type_key)),
                vec![], // 无输入
                vec![Arc {
                    place_id,
                    weight: 1,
                    consumes: false,
                }],
                None,
            );
        }

        pcpn
    }

    /// 添加结构性变迁
    fn add_structural_transition(
        &mut self,
        kind: TransitionKind,
        name: String,
        input_arcs: Vec<Arc>,
        output_arcs: Vec<Arc>,
        guard: Option<String>,
    ) {
        let trans_id = self.transitions.len();
        self.transitions.push(PcpnTransition {
            id: trans_id,
            name,
            kind,
            input_arcs,
            output_arcs,
            guard,
        });
    }

    /// 判断是否是 primitive 类型
    fn is_primitive(ty: &str) -> bool {
        matches!(
            ty,
            "bool"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "f32"
                | "f64"
                | "()"
                | "char"
                | "str"
        )
    }

    /// 简化类型名
    fn simplify_type(ty: &str) -> String {
        ty.split("::").last().unwrap_or(ty).to_string()
    }

    /// 格式化变迁名称
    fn format_transition_name(api: &ApiSignature, source: &ApiSource) -> String {
        let base_name = api.full_path.split("::").last().unwrap_or(&api.full_path);
        match source {
            ApiSource::Normal => base_name.to_string(),
            ApiSource::TraitImpl { trait_name } => format!("{}::{}", trait_name, base_name),
            ApiSource::FieldAccess {
                struct_name,
                field_name,
            } => {
                format!("{}.{}", Self::simplify_type(struct_name), field_name)
            }
        }
    }

    /// 生成 DOT 格式输出
    pub fn to_dot(&self) -> String {
        let mut dot = String::new();
        dot.push_str("digraph PCPN {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  fontname=\"Helvetica\";\n");
        dot.push_str("  node [fontname=\"Helvetica\"];\n");
        dot.push_str("  edge [fontname=\"Helvetica\"];\n");
        dot.push_str("  // PCPN: 每种类型有三个库所 T[own], T[shr], T[mut]\n\n");

        // Place 样式 - 类型库所，按 capability 着色
        dot.push_str("  // ========== Places (库所 = 类型 × Capability) ==========\n");
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

            let cap_suffix = match place.capability {
                Capability::Own => "[own]",
                Capability::Shr => "[&]",
                Capability::Mut => "[&mut]",
            };
            let label = format!("{}{}", Self::simplify_type(&place.type_key), cap_suffix);

            dot.push_str(&format!(
                "  p{} [label=\"{}\", shape=circle, style=filled, fillcolor={}, peripheries={}, width=0.9];\n",
                place.id, label, fillcolor, peripheries
            ));
        }
        dot.push_str("\n");

        // Transition 样式
        dot.push_str("  // ========== Transitions (变迁) ==========\n");
        for trans in &self.transitions {
            let (color, shape) = match &trans.kind {
                TransitionKind::ApiCall { source, .. } => match source {
                    ApiSource::Normal => ("palegreen", "box"),
                    ApiSource::TraitImpl { .. } => ("lightyellow", "box"),
                    ApiSource::FieldAccess { .. } => ("peachpuff", "box"),
                },
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

        // 弧 (Arcs)
        dot.push_str("  // ========== Arcs (弧) ==========\n");
        dot.push_str("  // 实线=消耗 token, 虚线=读取 token (不消耗)\n");
        for trans in &self.transitions {
            // 输入弧: Place -> Transition
            for arc in &trans.input_arcs {
                let style = if arc.consumes { "solid" } else { "dashed" };
                let arrow = if arc.consumes { "normal" } else { "odot" };
                // 弧颜色根据库所的 capability
                let arc_color = Self::capability_color(self.places[arc.place_id].capability);

                dot.push_str(&format!(
                    "  p{} -> t{} [style={}, arrowhead={}, color=\"{}\", penwidth=1.5];\n",
                    arc.place_id, trans.id, style, arrow, arc_color
                ));
            }

            // 输出弧: Transition -> Place
            for arc in &trans.output_arcs {
                let arc_color = Self::capability_color(self.places[arc.place_id].capability);
                dot.push_str(&format!(
                    "  t{} -> p{} [color=\"{}\", penwidth=1.5];\n",
                    trans.id, arc.place_id, arc_color
                ));
            }
        }

        dot.push_str("}\n");
        dot
    }

    /// Capability 到标签
    fn capability_label(cap: Capability) -> &'static str {
        match cap {
            Capability::Own => "own",
            Capability::Shr => "&",
            Capability::Mut => "&mut",
        }
    }

    /// Capability 到颜色
    fn capability_color(cap: Capability) -> &'static str {
        match cap {
            Capability::Own => "black",
            Capability::Shr => "blue",
            Capability::Mut => "red",
        }
    }

    /// 生成统计信息
    pub fn stats(&self) -> PcpnStats {
        let api_transitions = self
            .transitions
            .iter()
            .filter(|t| matches!(t.kind, TransitionKind::ApiCall { .. }))
            .count();
        let structural_transitions = self.transitions.len() - api_transitions;

        PcpnStats {
            num_places: self.places.len(),
            num_transitions: self.transitions.len(),
            num_api_transitions: api_transitions,
            num_structural_transitions: structural_transitions,
            num_primitive_places: self.initial_places.len(),
        }
    }
}

/// PCPN 统计信息
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

    #[test]
    fn test_is_primitive() {
        assert!(Pcpn::is_primitive("i32"));
        assert!(Pcpn::is_primitive("bool"));
        assert!(!Pcpn::is_primitive("Counter"));
        assert!(!Pcpn::is_primitive("std::vec::Vec"));
    }
}
