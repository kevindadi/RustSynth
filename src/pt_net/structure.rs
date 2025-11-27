use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 唯一的 Place 标识符
pub type PlaceId = u64;
/// 唯一的 Transition 标识符
pub type TransitionId = u64;

/// 边的类型，描述数据流动的语义
/// (RawPtr 由 EdgeData.is_raw_ptr 控制)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// 函数获取所有权 (Move semantics)
    Move,
    /// 函数获取不可变引用 (&T)
    Ref,
    /// 函数获取可变引用 (&mut T)
    MutRef,
}

/// Transition 类型 (对应 IR 中的 OpKind)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransitionKind {
    FnCall,
    StructCtor,
    VariantCtor,
    UnionCtor,
    FieldAccessor,
    MethodCall,
    AssocFn,
    /// Constant 别名变迁
    ConstantAlias,
    /// Static 别名变迁
    StaticAlias,
}

/// Place 节点数据: 代表具体的类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceData {
    /// 唯一标识 (TypeNode 的 hash)
    pub id: PlaceId,
    /// 规范的具体类型名称 (e.g., "std::vec::Vec<u8>")
    pub type_name: String,
    /// 是否为 fuzzing 原语 (如 u8, usize)
    pub is_source: bool,
    /// 是否实现了 Copy trait
    pub is_copy: bool,
}

/// Transition 节点数据: 代表函数调用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionData {
    /// 唯一标识 (OpNode.id)
    pub id: TransitionId,
    /// 函数全名 (e.g., "std::fs::File::open")
    pub func_name: String,
    /// Transition 类型
    pub kind: TransitionKind,
    /// 泛型映射: 参数名 -> 具体类型 (e.g., "R" -> "File")
    pub generic_map: HashMap<String, String>,
}

/// 节点负载: Petri Net 中的节点要么是 Place，要么是 Transition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodePayload {
    Place(PlaceData),
    Transition(TransitionData),
}

impl NodePayload {
    pub fn as_place(&self) -> Option<&PlaceData> {
        match self {
            NodePayload::Place(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_transition(&self) -> Option<&TransitionData> {
        match self {
            NodePayload::Transition(d) => Some(d),
            _ => None,
        }
    }
}

/// 边数据: 描述数据如何传输以及参数/返回值的顺序
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeData {
    /// 边的类型 (Move, Ref, MutRef)
    pub kind: EdgeKind,
    /// 参数位置索引 或 返回值元组索引
    pub index: usize,
    /// 是否是指针类型 (如果是 true，可能需要 cast)
    pub is_raw_ptr: bool,
}

/// Petri Net
#[derive(Debug, Serialize, Deserialize)]
pub struct PetriNet {
    /// 底层的 petgraph DiGraph
    /// 使用 u32 作为索引以节省内存
    pub graph: DiGraph<NodePayload, EdgeData, u32>,

    /// Place ID 到 NodeIndex 的快速查找表
    pub place_map: HashMap<PlaceId, NodeIndex>,

    /// Transition ID 到 NodeIndex 的快速查找表
    pub transition_map: HashMap<TransitionId, NodeIndex>,
}

impl PetriNet {
    /// 创建一个新的空 Petri Net
    pub fn new() -> Self {
        Self {
            graph: DiGraph::default(),
            place_map: HashMap::new(),
            transition_map: HashMap::new(),
        }
    }

    /// 添加一个 Place 节点
    pub fn add_place(&mut self, data: PlaceData) -> NodeIndex {
        let id = data.id;
        // 如果已经存在，直接返回索引
        if let Some(&idx) = self.place_map.get(&id) {
            return idx;
        }
        let idx = self.graph.add_node(NodePayload::Place(data));
        self.place_map.insert(id, idx);
        idx
    }

    /// 添加一个 Transition 节点
    pub fn add_transition(&mut self, data: TransitionData) -> NodeIndex {
        let id = data.id;
        // Transition ID 应该是唯一的，但如果重复添加，也返回现有索引
        if let Some(&idx) = self.transition_map.get(&id) {
            return idx;
        }
        let idx = self.graph.add_node(NodePayload::Transition(data));
        self.transition_map.insert(id, idx);
        idx
    }

    /// 连接两个节点
    pub fn connect(&mut self, from: NodeIndex, to: NodeIndex, edge: EdgeData) {
        self.graph.add_edge(from, to, edge);
    }

    /// 根据 Place ID 获取 NodeIndex
    pub fn get_place_index(&self, id: PlaceId) -> Option<NodeIndex> {
        self.place_map.get(&id).copied()
    }

    /// 根据 Transition ID 获取 NodeIndex
    pub fn get_transition_index(&self, id: TransitionId) -> Option<NodeIndex> {
        self.transition_map.get(&id).copied()
    }
}

impl Default for PetriNet {
    fn default() -> Self {
        Self::new()
    }
}
