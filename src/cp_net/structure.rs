/// CP-Net (Colored Petri Net with Trait Hub) 数据结构定义
///
/// 这个模块定义了用于模糊测试的 Petri 网结构，包括：
/// - Place（库所）：代表具体类型或 Trait Hub
/// - Transition（变迁）：代表函数调用或类型转换
/// - Arc（弧）：描述数据流动方式

use crate::ir_graph::structure::IrGraph;
use crate::petri_net_traits::{FromIrGraph, PetriNetExport, PetriNetKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Place（库所）：代表类型实例的存储位置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Place {
    /// 唯一标识符（TypeNode 的 Hash 或 Trait ID 的字符串）
    pub id: String,

    /// 类型信息的简要描述
    pub type_info: String,

    /// 是否是 Trait Hub（虚拟库所）
    /// 如果为 true，则该 Place 代表一个 Trait，可以接收所有实现该 Trait 的类型
    pub is_trait_hub: bool,

    /// 如果是 Trait Hub，存储对应的 Trait ID
    pub trait_id: Option<String>,

    /// 完整的类型路径（用于代码生成）
    pub resolved_path: Option<String>,

    /// 是否是原语类型（可以直接由 Fuzzer 生成）
    pub is_source: bool,

    /// 是否实现了 Copy trait
    pub is_copy: bool,
}

/// Transition（变迁）：代表一个操作
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    /// 唯一标识符
    pub id: String,

    /// 操作名称
    pub name: String,

    /// 变迁类型
    pub kind: TransitionKind,

    /// 泛型映射：参数名 -> 具体类型名称
    pub generic_map: HashMap<String, String>,
}

/// Transition 的类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransitionKind {
    /// 普通函数调用
    Call,

    /// 类型转换（Trait 实现上转）
    /// 例如：Type A -> Trait X
    ImplCast {
        /// 源类型 ID
        from_type: String,
        /// 目标 Trait ID
        to_trait: String,
    },

    /// 构造器（结构体、枚举变体等）
    Constructor,

    /// 字段访问器
    FieldAccessor,

    /// 方法调用
    MethodCall,

    /// 关联函数
    AssocFn,
}

/// Arc（弧）：连接 Place 和 Transition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arc {
    /// 源节点 ID（Place 或 Transition）
    pub source: String,

    /// 目标节点 ID（Transition 或 Place）
    pub target: String,

    /// 弧的类型（描述数据流动方式）
    pub arc_type: ArcType,

    /// 权重（通常为 1）
    pub weight: u32,

    /// 参数索引（如果是输入弧，对应函数的第几个参数）
    pub param_index: Option<usize>,
}

/// 弧的类型：描述数据如何传递
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArcType {
    /// Input（消耗 Token）：Move 语义
    Input,

    /// Output（生产 Token）：返回值
    Output,

    /// Read（只读，不消耗 Token）：共享引用 &T
    Read,

    /// ReadWrite（读写）：可变引用 &mut T
    /// 可以实现为一对 Input/Output 弧
    ReadWrite,
}

/// CP-Petri Net（带 Trait Hub 的 Colored Petri Net）
#[derive(Debug, Serialize, Deserialize)]
pub struct CpPetriNet {
    /// 所有 Place 节点
    pub places: Vec<Place>,

    /// 所有 Transition 节点
    pub transitions: Vec<Transition>,

    /// 所有 Arc
    pub arcs: Vec<Arc>,

    /// Place ID 到索引的映射（快速查找）
    #[serde(skip)]
    pub place_index: HashMap<String, usize>,

    /// Transition ID 到索引的映射（快速查找）
    #[serde(skip)]
    pub transition_index: HashMap<String, usize>,
}

impl CpPetriNet {
    /// 创建一个新的空 Petri Net
    pub fn new() -> Self {
        Self {
            places: Vec::new(),
            transitions: Vec::new(),
            arcs: Vec::new(),
            place_index: HashMap::new(),
            transition_index: HashMap::new(),
        }
    }

    /// 添加一个 Place
    pub fn add_place(&mut self, place: Place) {
        let id = place.id.clone();
        let index = self.places.len();
        self.places.push(place);
        self.place_index.insert(id, index);
    }

    /// 添加一个 Transition
    pub fn add_transition(&mut self, transition: Transition) {
        let id = transition.id.clone();
        let index = self.transitions.len();
        self.transitions.push(transition);
        self.transition_index.insert(id, index);
    }

    /// 添加一条 Arc
    pub fn add_arc(&mut self, arc: Arc) {
        self.arcs.push(arc);
    }

    /// 根据 ID 查找 Place
    pub fn get_place(&self, id: &str) -> Option<&Place> {
        self.place_index
            .get(id)
            .and_then(|&idx| self.places.get(idx))
    }

    /// 根据 ID 查找 Transition
    pub fn get_transition(&self, id: &str) -> Option<&Transition> {
        self.transition_index
            .get(id)
            .and_then(|&idx| self.transitions.get(idx))
    }

    /// 导出为 JSON 字符串
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// 重建索引（反序列化后需要调用）
    pub fn rebuild_indices(&mut self) {
        self.place_index.clear();
        self.transition_index.clear();

        for (idx, place) in self.places.iter().enumerate() {
            self.place_index.insert(place.id.clone(), idx);
        }

        for (idx, transition) in self.transitions.iter().enumerate() {
            self.transition_index.insert(transition.id.clone(), idx);
        }
    }
}

impl Default for CpPetriNet {
    fn default() -> Self {
        Self::new()
    }
}

// ============ Trait 实现 ============

impl FromIrGraph for CpPetriNet {
    fn from_ir_graph(ir: &IrGraph) -> Self {
        crate::cp_net::builder::CpNetBuilder::from_ir(ir)
    }
}

impl PetriNetKind for CpPetriNet {
    fn kind_name() -> &'static str {
        "CP-Net"
    }
    
    fn description() -> &'static str {
        "Colored Petri Net with Trait Hub for Fuzzing Path Exploration"
    }
}

impl PetriNetExport for CpPetriNet {
    fn to_dot(&self) -> String {
        // 调用 export.rs 中定义的方法
        self.to_dot()
    }
    
    fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
    
    fn print_stats(&self) {
        // 调用 export.rs 中定义的方法
        self.print_stats()
    }
}
