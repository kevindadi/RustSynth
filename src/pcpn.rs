//! PCPN (Pushdown Colored Petri Net) - 下推着色 Petri 网
//!
//! 核心设计（工程化简化版）：
//!
//! ## 类型宇宙（已单态）
//! Ty ::= T | RefShr(T) | RefMut(T)
//!
//! ## Places（能力区分）
//! Cap ::= Own | Frz | Blk
//! Place ::= p_{cap, Ty}
//!
//! - Own(T): 拥有 T 的所有权
//! - Frz(T): T 被冻结（有活跃的共享借用）
//! - Blk(T): T 被阻塞（有活跃的可变借用）
//! - Own(RefShr(T)): 拥有 &T 引用
//! - Own(RefMut(T)): 拥有 &mut T 引用
//!
//! ## 函数参数绑定规则（关键）
//! | Rust 参数 | 连接到的 Place |
//! |----------|---------------|
//! | T        | Own(T)        |
//! | &T       | Own(RefShr(T))|
//! | &mut T   | Own(RefMut(T))|
//!
//! ## 结构性变迁
//! - BorrowShrFirst: Own(T) → Frz(T) + Own(RefShr(T))
//! - BorrowShrNext: Frz(T) → Frz(T) + Own(RefShr(T))
//! - EndShrKeepFrz: Frz(T) + Own(RefShr(T)) → Frz(T)
//! - EndShrUnfreeze: Frz(T) + Own(RefShr(T)) → Own(T)
//! - BorrowMut: Own(T)[bind_mut] → Blk(T) + Own(RefMut(T))
//! - EndMut: Blk(T) + Own(RefMut(T)) → Own(T)
//! - MakeMutByMove: Own(T, mut=false) → Own(T, mut=true)
//! - MakeMutByCopy: Own(T) → Own(T) + Own(T, mut=true)  [T: Copy]
//! - Drop: Own(T) → ε
//! - CopyUse: Own(T) → Own(T) + Own(T)  [T: Copy]

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::apigraph::{ApiGraph, FunctionNode};
use crate::type_model::{PassingMode, TypeKey};

/// Place 标识
pub type PlaceId = usize;

/// Transition 标识
pub type TransitionId = usize;

/// Capability (能力状态)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// 拥有所有权 (owned, 可用)
    Own,
    /// 冻结状态 (有活跃的共享借用)
    Frz,
    /// 阻塞状态 (有活跃的可变借用)
    Blk,
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Capability::Own => write!(f, "own"),
            Capability::Frz => write!(f, "frz"),
            Capability::Blk => write!(f, "blk"),
        }
    }
}

/// PCPN Place (库所)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Place {
    /// Place 唯一标识
    pub id: PlaceId,
    /// 类型键 (完整类型，可能是 T, RefShr(T), RefMut(T))
    pub type_key: TypeKey,
    /// 该库所对应的 capability
    pub capability: Capability,
    /// 是否是 primitive 类型
    pub is_primitive: bool,
    /// 是否是引用类型
    pub is_ref: bool,
}

impl Place {
    /// 获取 Place 的显示名称
    pub fn display_name(&self) -> String {
        format!("{}[{}]", self.type_key.short_name(), self.capability)
    }
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
    pub guard: Option<Guard>,
}

/// 变迁类型
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionKind {
    /// API 调用
    ApiCall {
        fn_id: usize,
        /// 是否是 const 方法（自我使能）
        is_const: bool,
    },
    /// 创建 primitive 常量
    CreatePrimitive { type_key: TypeKey },

    // ========== 借用相关变迁 ==========
    /// 首次共享借用: Own(T) → Frz(T) + Own(RefShr(T))
    BorrowShrFirst { base_type: TypeKey },
    /// 后续共享借用: Frz(T) → Frz(T) + Own(RefShr(T))
    BorrowShrNext { base_type: TypeKey },
    /// 结束共享借用（保持冻结）: Frz(T) + Own(RefShr(T)) → Frz(T)
    EndShrKeepFrz { base_type: TypeKey },
    /// 结束共享借用（解冻）: Frz(T) + Own(RefShr(T)) → Own(T)
    EndShrUnfreeze { base_type: TypeKey },
    /// 可变借用: Own(T) → Blk(T) + Own(RefMut(T))
    BorrowMut { base_type: TypeKey },
    /// 结束可变借用: Blk(T) + Own(RefMut(T)) → Own(T)
    EndMut { base_type: TypeKey },

    // ========== let mut 相关变迁 ==========
    /// 通过 Move 创建 mut 绑定: Own(T, mut=false) → Own(T, mut=true)
    MakeMutByMove { type_key: TypeKey },
    /// 通过 Copy 创建 mut 绑定: Own(T) → Own(T) + Own(T, mut=true)
    MakeMutByCopy { type_key: TypeKey },

    // ========== 其他变迁 ==========
    /// Drop: Own(T) → ε
    Drop { type_key: TypeKey },
    /// Copy 使用: Own(T) → Own(T) + Own(T)
    CopyUse { type_key: TypeKey },
    /// Clone: Own(T) + ε → Own(T) + Own(T)
    Clone { type_key: TypeKey },
}

/// 守卫条件
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Guard {
    /// 需要 token 的 bind_mut = true
    RequireBindMut,
    /// 需要检查没有活跃借用
    NoActiveBorrow,
    /// 需要检查没有其他共享借用
    NoOtherShrBorrow,
    /// 自定义守卫
    Custom(String),
}

impl std::fmt::Display for Guard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Guard::RequireBindMut => write!(f, "bind_mut"),
            Guard::NoActiveBorrow => write!(f, "can_drop"),
            Guard::NoOtherShrBorrow => write!(f, "last_shr"),
            Guard::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// 弧 (连接 Place 和 Transition)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Arc {
    /// 连接的 Place
    pub place_id: PlaceId,
    /// 是否消耗 token (false = 读取/测试弧)
    pub consumes: bool,
    /// 弧的注解（用于代码生成）
    pub annotation: Option<ArcAnnotation>,
}

/// 弧的注解
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ArcAnnotation {
    /// 参数位置
    Param { index: usize, name: String },
    /// Self 参数
    SelfParam,
    /// 返回值
    Return,
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
    /// 初始标识 (可以凭空创建 token 的 places，如 primitive)
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
    /// 1. 对于每个基础类型 T：创建 Own(T), Frz(T), Blk(T)
    /// 2. 对于引用类型 &T/&mut T：创建 Own(RefShr(T)), Own(RefMut(T))
    /// 3. 函数参数直接连接到正确的库所
    pub fn from_api_graph(graph: &ApiGraph) -> Self {
        let mut pcpn = Pcpn::new();

        // 收集所有需要的类型（包括引用类型）
        let mut all_types: IndexSet<TypeKey> = IndexSet::new();
        let mut base_types: IndexSet<TypeKey> = IndexSet::new();

        for type_node in &graph.type_nodes {
            let ty = &type_node.type_key;
            all_types.insert(ty.clone());
            base_types.insert(ty.clone());

            // 如果类型不是引用，也需要为其创建引用类型的库所
            if !ty.is_ref() {
                all_types.insert(TypeKey::ref_shr(ty.clone()));
                all_types.insert(TypeKey::ref_mut(ty.clone()));
            }
        }

        // 1. 为每个基础类型创建三个库所 (Own, Frz, Blk)
        for ty in &base_types {
            if !ty.is_ref() {
                pcpn.create_base_type_places(ty);
            }
        }

        // 2. 为每个引用类型创建 Own 库所
        for ty in &all_types {
            if ty.is_ref() {
                pcpn.create_ref_type_place(ty);
            }
        }

        // 3. 为每个函数创建变迁
        for fn_node in &graph.fn_nodes {
            pcpn.create_function_transition(graph, fn_node);
        }

        // 4. 为非 primitive 基础类型添加结构性变迁
        for ty in &base_types {
            if !ty.is_primitive() && !ty.is_ref() {
                pcpn.create_structural_transitions(ty);
            }
        }

        // 5. 为 primitive 类型添加创建变迁
        for &place_id in &pcpn.initial_places.clone() {
            let place = &pcpn.places[place_id];
            if place.capability == Capability::Own && !place.is_ref {
                let type_key = place.type_key.clone();
                pcpn.create_primitive_transition(&type_key, place_id);
            }
        }

        pcpn
    }

    /// 为基础类型创建三个库所: Own(T), Frz(T), Blk(T)
    fn create_base_type_places(&mut self, type_key: &TypeKey) {
        let is_primitive = type_key.is_primitive();

        for cap in [Capability::Own, Capability::Frz, Capability::Blk] {
            let place_id = self.places.len();
            self.places.push(Place {
                id: place_id,
                type_key: type_key.clone(),
                capability: cap,
                is_primitive,
                is_ref: false,
            });
            self.type_cap_to_place
                .insert((type_key.clone(), cap), place_id);

            // Primitive 类型的 Own 库所作为初始 place
            if is_primitive && cap == Capability::Own {
                self.initial_places.push(place_id);
            }
        }
    }

    /// 为引用类型创建 Own 库所: Own(RefShr(T)), Own(RefMut(T))
    fn create_ref_type_place(&mut self, type_key: &TypeKey) {
        let place_id = self.places.len();
        self.places.push(Place {
            id: place_id,
            type_key: type_key.clone(),
            capability: Capability::Own,
            is_primitive: false,
            is_ref: true,
        });
        self.type_cap_to_place
            .insert((type_key.clone(), Capability::Own), place_id);
    }

    /// 获取类型对应的 Place ID
    fn get_place(&self, type_key: &TypeKey, cap: Capability) -> Option<PlaceId> {
        self.type_cap_to_place
            .get(&(type_key.clone(), cap))
            .copied()
    }

    /// 为函数创建变迁
    ///
    /// 关键规则：
    /// - T 参数 → 连接 Own(T)
    /// - &T 参数 → 连接 Own(RefShr(T))
    /// - &mut T 参数 → 连接 Own(RefMut(T))
    fn create_function_transition(&mut self, graph: &ApiGraph, fn_node: &FunctionNode) {
        let trans_id = self.transitions.len();
        let mut input_arcs = Vec::new();
        let mut output_arcs = Vec::new();

        // 处理输入边
        let input_edges = graph.get_input_edges(fn_node.id);
        for edge in input_edges {
            let base_type = &graph.type_nodes[edge.type_node].type_key;

            // 根据 PassingMode 确定连接的 Place
            let (place_type, consumes) = match &edge.passing_mode {
                PassingMode::Move => (base_type.clone(), true),
                PassingMode::Copy => (base_type.clone(), false),
                PassingMode::BorrowShr => (TypeKey::ref_shr(base_type.clone()), true),
                PassingMode::BorrowMut => (TypeKey::ref_mut(base_type.clone()), true),
                _ => continue,
            };

            // 确保引用类型的 Place 存在
            let place_id = if let Some(id) = self.get_place(&place_type, Capability::Own) {
                id
            } else {
                // 创建缺失的 Place
                self.create_ref_type_place(&place_type);
                self.get_place(&place_type, Capability::Own).unwrap()
            };

            let annotation = edge.param_index.map(|idx| {
                if fn_node.self_param.is_some() && idx == 0 {
                    ArcAnnotation::SelfParam
                } else {
                    let param_idx = if fn_node.self_param.is_some() {
                        idx - 1
                    } else {
                        idx
                    };
                    let name = fn_node
                        .params
                        .get(param_idx)
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| format!("arg{}", param_idx));
                    ArcAnnotation::Param { index: idx, name }
                }
            });

            input_arcs.push(Arc {
                place_id,
                consumes,
                annotation,
            });
        }

        // 处理输出边
        let output_edges = graph.get_output_edges(fn_node.id);
        for edge in output_edges {
            let base_type = &graph.type_nodes[edge.type_node].type_key;

            // 根据返回模式确定连接的 Place
            let place_type = match &edge.passing_mode {
                PassingMode::ReturnOwned => base_type.clone(),
                PassingMode::ReturnBorrowShr => TypeKey::ref_shr(base_type.clone()),
                PassingMode::ReturnBorrowMut => TypeKey::ref_mut(base_type.clone()),
                _ => continue,
            };

            // 确保 Place 存在
            let place_id = if let Some(id) = self.get_place(&place_type, Capability::Own) {
                id
            } else {
                self.create_ref_type_place(&place_type);
                self.get_place(&place_type, Capability::Own).unwrap()
            };

            output_arcs.push(Arc {
                place_id,
                consumes: false,
                annotation: Some(ArcAnnotation::Return),
            });
        }

        // 检查是否是 const 方法（简化：暂时都视为非 const）
        let is_const = false;

        self.transitions.push(Transition {
            id: trans_id,
            name: fn_node.path.clone(),
            kind: TransitionKind::ApiCall {
                fn_id: fn_node.id,
                is_const,
            },
            input_arcs,
            output_arcs,
            guard: None,
        });
    }

    /// 创建结构性变迁
    fn create_structural_transitions(&mut self, base_type: &TypeKey) {
        let short_name = base_type.short_name();
        let is_copy = base_type.is_copy();

        // 获取基础类型的库所
        let own_place = self.get_place(base_type, Capability::Own).unwrap();
        let frz_place = self.get_place(base_type, Capability::Frz).unwrap();
        let blk_place = self.get_place(base_type, Capability::Blk).unwrap();

        // 获取引用类型的库所
        let ref_shr_type = TypeKey::ref_shr(base_type.clone());
        let ref_mut_type = TypeKey::ref_mut(base_type.clone());

        // 确保引用类型的 Place 存在
        let ref_shr_place = self
            .get_place(&ref_shr_type, Capability::Own)
            .unwrap_or_else(|| {
                self.create_ref_type_place(&ref_shr_type);
                self.get_place(&ref_shr_type, Capability::Own).unwrap()
            });
        let ref_mut_place = self
            .get_place(&ref_mut_type, Capability::Own)
            .unwrap_or_else(|| {
                self.create_ref_type_place(&ref_mut_type);
                self.get_place(&ref_mut_type, Capability::Own).unwrap()
            });

        // BorrowShrFirst: Own(T) → Frz(T) + Own(RefShr(T))
        self.add_transition(
            format!("&{} [first]", short_name),
            TransitionKind::BorrowShrFirst {
                base_type: base_type.clone(),
            },
            vec![Arc {
                place_id: own_place,
                consumes: true,
                annotation: None,
            }],
            vec![
                Arc {
                    place_id: frz_place,
                    consumes: false,
                    annotation: None,
                },
                Arc {
                    place_id: ref_shr_place,
                    consumes: false,
                    annotation: None,
                },
            ],
            None,
        );

        // BorrowShrNext: Frz(T) → Frz(T) + Own(RefShr(T))
        self.add_transition(
            format!("&{} [next]", short_name),
            TransitionKind::BorrowShrNext {
                base_type: base_type.clone(),
            },
            vec![Arc {
                place_id: frz_place,
                consumes: false,
                annotation: None,
            }],
            vec![Arc {
                place_id: ref_shr_place,
                consumes: false,
                annotation: None,
            }],
            None,
        );

        // EndShrKeepFrz: Frz(T) + Own(RefShr(T)) → Frz(T)
        // 当还有其他共享借用时，保持冻结状态
        self.add_transition(
            format!("drop &{} [keep_frz]", short_name),
            TransitionKind::EndShrKeepFrz {
                base_type: base_type.clone(),
            },
            vec![
                Arc {
                    place_id: frz_place,
                    consumes: false,
                    annotation: None,
                },
                Arc {
                    place_id: ref_shr_place,
                    consumes: true,
                    annotation: None,
                },
            ],
            vec![],
            Some(Guard::Custom("has_other_shr".to_string())),
        );

        // EndShrUnfreeze: Frz(T) + Own(RefShr(T)) → Own(T)
        // 当这是最后一个共享借用时，解冻
        self.add_transition(
            format!("drop &{} [unfreeze]", short_name),
            TransitionKind::EndShrUnfreeze {
                base_type: base_type.clone(),
            },
            vec![
                Arc {
                    place_id: frz_place,
                    consumes: true,
                    annotation: None,
                },
                Arc {
                    place_id: ref_shr_place,
                    consumes: true,
                    annotation: None,
                },
            ],
            vec![Arc {
                place_id: own_place,
                consumes: false,
                annotation: None,
            }],
            Some(Guard::NoOtherShrBorrow),
        );

        // BorrowMut: Own(T) → Blk(T) + Own(RefMut(T))
        self.add_transition(
            format!("&mut {}", short_name),
            TransitionKind::BorrowMut {
                base_type: base_type.clone(),
            },
            vec![Arc {
                place_id: own_place,
                consumes: true,
                annotation: None,
            }],
            vec![
                Arc {
                    place_id: blk_place,
                    consumes: false,
                    annotation: None,
                },
                Arc {
                    place_id: ref_mut_place,
                    consumes: false,
                    annotation: None,
                },
            ],
            Some(Guard::RequireBindMut),
        );

        // EndMut: Blk(T) + Own(RefMut(T)) → Own(T)
        self.add_transition(
            format!("drop &mut {}", short_name),
            TransitionKind::EndMut {
                base_type: base_type.clone(),
            },
            vec![
                Arc {
                    place_id: blk_place,
                    consumes: true,
                    annotation: None,
                },
                Arc {
                    place_id: ref_mut_place,
                    consumes: true,
                    annotation: None,
                },
            ],
            vec![Arc {
                place_id: own_place,
                consumes: false,
                annotation: None,
            }],
            None,
        );

        // MakeMutByMove: Own(T, mut=false) → Own(T, mut=true)
        // 这个变迁不改变库所中的 token，而是改变 token 的 bind_mut 属性
        self.add_transition(
            format!("let mut = {}", short_name),
            TransitionKind::MakeMutByMove {
                type_key: base_type.clone(),
            },
            vec![Arc {
                place_id: own_place,
                consumes: true,
                annotation: None,
            }],
            vec![Arc {
                place_id: own_place,
                consumes: false,
                annotation: None,
            }],
            None,
        );

        // Drop: Own(T) → ε
        self.add_transition(
            format!("drop {}", short_name),
            TransitionKind::Drop {
                type_key: base_type.clone(),
            },
            vec![Arc {
                place_id: own_place,
                consumes: true,
                annotation: None,
            }],
            vec![],
            Some(Guard::NoActiveBorrow),
        );

        // Copy 类型特有的变迁
        if is_copy {
            // CopyUse: Own(T) → Own(T) + Own(T)
            self.add_transition(
                format!("copy {}", short_name),
                TransitionKind::CopyUse {
                    type_key: base_type.clone(),
                },
                vec![Arc {
                    place_id: own_place,
                    consumes: false,
                    annotation: None,
                }],
                vec![Arc {
                    place_id: own_place,
                    consumes: false,
                    annotation: None,
                }],
                None,
            );

            // MakeMutByCopy: Own(T) → Own(T) + Own(T, mut=true)
            self.add_transition(
                format!("let mut copy {}", short_name),
                TransitionKind::MakeMutByCopy {
                    type_key: base_type.clone(),
                },
                vec![Arc {
                    place_id: own_place,
                    consumes: false,
                    annotation: None,
                }],
                vec![Arc {
                    place_id: own_place,
                    consumes: false,
                    annotation: None,
                }],
                None,
            );
        }
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
                annotation: None,
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
        guard: Option<Guard>,
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
        dot.push_str("  // PCPN: Own/Frz/Blk capability places\n\n");

        // Places
        dot.push_str("  // ========== Places ==========\n");
        for place in &self.places {
            let fillcolor = if place.is_ref {
                match place.type_key {
                    TypeKey::RefShr(_) => "lightcyan",
                    TypeKey::RefMut(_) => "mistyrose",
                    _ => "white",
                }
            } else {
                match place.capability {
                    Capability::Own => {
                        if place.is_primitive {
                            "lightgray"
                        } else {
                            "lightblue"
                        }
                    }
                    Capability::Frz => "lightyellow",
                    Capability::Blk => "lightcoral",
                }
            };

            let peripheries = if place.capability == Capability::Own {
                2
            } else {
                1
            };

            dot.push_str(&format!(
                "  p{} [label=\"{}\", shape=circle, style=filled, fillcolor={}, peripheries={}];\n",
                place.id,
                place.display_name(),
                fillcolor,
                peripheries
            ));
        }
        dot.push_str("\n");

        // Transitions
        dot.push_str("  // ========== Transitions ==========\n");
        for trans in &self.transitions {
            let (color, shape) = match &trans.kind {
                TransitionKind::ApiCall { .. } => ("palegreen", "box"),
                TransitionKind::CreatePrimitive { .. } => ("lightcyan", "diamond"),
                TransitionKind::BorrowShrFirst { .. } | TransitionKind::BorrowShrNext { .. } => {
                    ("lavender", "box")
                }
                TransitionKind::BorrowMut { .. } => ("pink", "box"),
                TransitionKind::EndShrKeepFrz { .. }
                | TransitionKind::EndShrUnfreeze { .. }
                | TransitionKind::EndMut { .. } => ("honeydew", "box"),
                TransitionKind::MakeMutByMove { .. } | TransitionKind::MakeMutByCopy { .. } => {
                    ("lightyellow", "box")
                }
                TransitionKind::Drop { .. } => ("gray90", "box"),
                TransitionKind::CopyUse { .. } | TransitionKind::Clone { .. } => {
                    ("paleturquoise", "box")
                }
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
                let color = self.arc_color(arc.place_id);

                dot.push_str(&format!(
                    "  p{} -> t{} [style={}, arrowhead={}, color=\"{}\"];\n",
                    arc.place_id, trans.id, style, arrow, color
                ));
            }

            for arc in &trans.output_arcs {
                let color = self.arc_color(arc.place_id);
                dot.push_str(&format!(
                    "  t{} -> p{} [color=\"{}\"];\n",
                    trans.id, arc.place_id, color
                ));
            }
        }

        dot.push_str("}\n");
        dot
    }

    fn arc_color(&self, place_id: PlaceId) -> &'static str {
        if let Some(place) = self.places.get(place_id) {
            if place.is_ref {
                match &place.type_key {
                    TypeKey::RefShr(_) => "blue",
                    TypeKey::RefMut(_) => "red",
                    _ => "black",
                }
            } else {
                match place.capability {
                    Capability::Own => "black",
                    Capability::Frz => "orange",
                    Capability::Blk => "darkred",
                }
            }
        } else {
            "black"
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

        let ref_places = self.places.iter().filter(|p| p.is_ref).count();
        let base_places = self.places.len() - ref_places;

        PcpnStats {
            num_places: self.places.len(),
            num_base_places: base_places,
            num_ref_places: ref_places,
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
    pub num_base_places: usize,
    pub num_ref_places: usize,
    pub num_transitions: usize,
    pub num_api_transitions: usize,
    pub num_structural_transitions: usize,
    pub num_primitive_places: usize,
}

impl std::fmt::Display for PcpnStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PCPN: {} places ({} base + {} ref, {} primitive), {} transitions ({} API, {} structural)",
            self.num_places,
            self.num_base_places,
            self.num_ref_places,
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
    fn test_pcpn_places() {
        let mut pcpn = Pcpn::new();
        let counter_type = TypeKey::path("Counter");

        pcpn.create_base_type_places(&counter_type);

        // 应该有 3 个库所: Own(Counter), Frz(Counter), Blk(Counter)
        assert_eq!(pcpn.places.len(), 3);

        // 检查 Own 库所
        let own_place = pcpn.get_place(&counter_type, Capability::Own);
        assert!(own_place.is_some());

        // 检查 Frz 库所
        let frz_place = pcpn.get_place(&counter_type, Capability::Frz);
        assert!(frz_place.is_some());

        // 检查 Blk 库所
        let blk_place = pcpn.get_place(&counter_type, Capability::Blk);
        assert!(blk_place.is_some());
    }

    #[test]
    fn test_ref_type_place() {
        let mut pcpn = Pcpn::new();
        let ref_shr_counter = TypeKey::ref_shr(TypeKey::path("Counter"));

        pcpn.create_ref_type_place(&ref_shr_counter);

        // 应该只有 1 个库所: Own(RefShr(Counter))
        assert_eq!(pcpn.places.len(), 1);
        assert!(pcpn.places[0].is_ref);
    }
}
