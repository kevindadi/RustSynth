//! Petri 网结构定义
//!
//! 定义完整的下推着色 Petri 网结构

use std::collections::HashMap;
use super::types::{TypeId, TypeRegistry};
use super::place::{PlaceId, Place, PlaceKind};
use super::transition::{TransitionId, Transition, StructuralKind};
use super::arc::{Arc, ArcId, ArcKind};
use super::marking::{Marking, Token, ValueIdGen};

/// 下推着色 Petri 网
///
/// N = (P, T, F, C, G, Stack)
#[derive(Debug, Clone)]
pub struct PcpnNet {
    /// 类型注册表
    pub types: TypeRegistry,

    /// 库所集合 P
    places: HashMap<PlaceId, Place>,
    /// 变迁集合 T
    transitions: HashMap<TransitionId, Transition>,
    /// 弧集合 F（流关系）
    arcs: Vec<Arc>,

    /// 变迁的输入弧索引: transition_id -> [arc_indices]
    input_arcs: HashMap<TransitionId, Vec<usize>>,
    /// 变迁的输出弧索引: transition_id -> [arc_indices]
    output_arcs: HashMap<TransitionId, Vec<usize>>,

    /// 初始标记
    pub initial_marking: Marking,
    /// 值 ID 生成器
    pub value_gen: ValueIdGen,

    /// ID 计数器
    next_place_id: u32,
    next_transition_id: u32,
    next_arc_id: u32,
}

impl Default for PcpnNet {
    fn default() -> Self {
        Self::new()
    }
}

impl PcpnNet {
    pub fn new() -> Self {
        PcpnNet {
            types: TypeRegistry::new(),
            places: HashMap::new(),
            transitions: HashMap::new(),
            arcs: Vec::new(),
            input_arcs: HashMap::new(),
            output_arcs: HashMap::new(),
            initial_marking: Marking::new(),
            value_gen: ValueIdGen::new(),
            next_place_id: 0,
            next_transition_id: 0,
            next_arc_id: 0,
        }
    }

    // ==================== 库所操作 ====================

    /// 添加库所
    pub fn add_place(&mut self, name: String, kind: PlaceKind, type_id: TypeId) -> PlaceId {
        let id = PlaceId::new(self.next_place_id);
        self.next_place_id += 1;

        let place = Place {
            id,
            name,
            kind,
            type_id,
            capacity: None,
        };
        self.places.insert(id, place);
        id
    }

    /// 添加所有权库所
    pub fn add_own_place(&mut self, name: String, type_id: TypeId) -> PlaceId {
        self.add_place(name, PlaceKind::Own, type_id)
    }

    /// 添加自动构造库所
    pub fn add_auto_construct_place(&mut self, name: String, type_id: TypeId) -> PlaceId {
        self.add_place(name, PlaceKind::AutoConstruct, type_id)
    }

    /// 获取库所
    pub fn get_place(&self, id: PlaceId) -> Option<&Place> {
        self.places.get(&id)
    }

    /// 获取所有库所
    pub fn places(&self) -> impl Iterator<Item = &Place> {
        self.places.values()
    }

    /// 根据类型获取库所
    pub fn places_by_type(&self, type_id: TypeId) -> Vec<&Place> {
        self.places.values().filter(|p| p.type_id == type_id).collect()
    }

    // ==================== 变迁操作 ====================

    /// 添加变迁
    pub fn add_transition(&mut self, transition: Transition) -> TransitionId {
        let id = transition.id;
        self.transitions.insert(id, transition);
        self.input_arcs.entry(id).or_default();
        self.output_arcs.entry(id).or_default();
        id
    }

    /// 创建并添加结构变迁
    pub fn add_structural_transition(
        &mut self,
        kind: StructuralKind,
        type_id: TypeId,
    ) -> TransitionId {
        let id = TransitionId::new(self.next_transition_id);
        self.next_transition_id += 1;

        let transition = Transition::structural(id, kind, type_id);
        self.add_transition(transition)
    }

    /// 获取变迁
    pub fn get_transition(&self, id: TransitionId) -> Option<&Transition> {
        self.transitions.get(&id)
    }

    /// 获取所有变迁
    pub fn transitions(&self) -> impl Iterator<Item = &Transition> {
        self.transitions.values()
    }

    /// 获取所有 API 调用变迁
    pub fn api_transitions(&self) -> impl Iterator<Item = &Transition> {
        self.transitions.values().filter(|t| t.is_api_call())
    }

    /// 分配新的变迁 ID
    pub fn next_transition_id(&mut self) -> TransitionId {
        let id = TransitionId::new(self.next_transition_id);
        self.next_transition_id += 1;
        id
    }

    // ==================== 弧操作 ====================

    /// 添加弧
    fn add_arc(&mut self, arc: Arc) -> ArcId {
        let id = arc.id;
        let trans_id = arc.transition();

        if arc.is_input {
            self.input_arcs.entry(trans_id).or_default().push(self.arcs.len());
        } else {
            self.output_arcs.entry(trans_id).or_default().push(self.arcs.len());
        }

        self.arcs.push(arc);
        id
    }

    /// 添加输入弧
    pub fn add_input_arc(
        &mut self,
        place: PlaceId,
        transition: TransitionId,
        kind: ArcKind,
        weight: usize,
        color: Option<TypeId>,
    ) -> ArcId {
        let id = ArcId(self.next_arc_id);
        self.next_arc_id += 1;

        let arc = Arc::input(id, place, transition, kind, weight, color);
        self.add_arc(arc)
    }

    /// 添加输出弧
    pub fn add_output_arc(
        &mut self,
        transition: TransitionId,
        place: PlaceId,
        kind: ArcKind,
        weight: usize,
        color: Option<TypeId>,
    ) -> ArcId {
        let id = ArcId(self.next_arc_id);
        self.next_arc_id += 1;

        let arc = Arc::output(id, transition, place, kind, weight, color);
        self.add_arc(arc)
    }

    /// 获取变迁的输入弧
    pub fn get_input_arcs(&self, transition: TransitionId) -> Vec<&Arc> {
        self.input_arcs
            .get(&transition)
            .map(|indices| indices.iter().map(|&i| &self.arcs[i]).collect())
            .unwrap_or_default()
    }

    /// 获取变迁的输出弧
    pub fn get_output_arcs(&self, transition: TransitionId) -> Vec<&Arc> {
        self.output_arcs
            .get(&transition)
            .map(|indices| indices.iter().map(|&i| &self.arcs[i]).collect())
            .unwrap_or_default()
    }

    // ==================== 初始标记操作 ====================

    /// 设置初始标记
    pub fn set_initial_token(&mut self, place: PlaceId, type_id: TypeId, count: usize) {
        for _ in 0..count {
            let value_id = self.value_gen.next();
            let token = Token { type_id, value_id };
            self.initial_marking.add(place, token);
        }
    }

    // ==================== 统计信息 ====================

    /// 获取网的统计信息
    pub fn stats(&self) -> NetStats {
        let api_count = self.api_transitions().count();
        let structural_count = self.transitions.len() - api_count;

        NetStats {
            place_count: self.places.len(),
            transition_count: self.transitions.len(),
            arc_count: self.arcs.len(),
            type_count: self.types.types.len(),
            api_transition_count: api_count,
            structural_transition_count: structural_count,
        }
    }
}

/// 网统计信息
#[derive(Debug, Clone)]
pub struct NetStats {
    pub place_count: usize,
    pub transition_count: usize,
    pub arc_count: usize,
    pub type_count: usize,
    pub api_transition_count: usize,
    pub structural_transition_count: usize,
}

impl std::fmt::Display for NetStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PCPN: {} places, {} transitions ({} API, {} structural), {} arcs, {} types",
            self.place_count,
            self.transition_count,
            self.api_transition_count,
            self.structural_transition_count,
            self.arc_count,
            self.type_count
        )
    }
}

