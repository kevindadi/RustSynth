//! PCPN 仿真器 - 简化版
//!
//! ## 设计原则（新版）
//!
//! ### Token 表示
//! - Token 只表示"一个可用的资源实例"
//! - 仅包含类型信息（完整引用层级）
//!
//! ### Place 设计
//! - 每个类型 Ty 一个 Place
//! - P[T], P[&T], P[&mut T], P[&&T], ...
//!
//! ### 初始标记
//! - 每个出现的 primitive 类型：1 个 token，budget = 1
//! - 其他类型：初始 0 个，通过 API 产生
//!
//! ### Copy 语义（返还弧）
//! - Copy 类型参数：不消耗（pre-1, post+1）
//! - 非 Copy 类型参数：消耗（move）
//! - 引用参数：总是消耗
//!
//! ### Firing 判定
//! 1. structural_enabled: pre places 有足够 token
//! 2. budget_ok: 发生后不超过 budget
//! 3. dup_limit_ok: Copy/Clone 次数限制
//!
//! ### 输出
//! - 只输出抽象 trace（transition + consumes + produces）

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use crate::pcpn::{Pcpn, PlaceId, Transition, TransitionKind};
use crate::type_model::TypeKey;

// ==================== Trace Firing ====================

/// Trace 中的一步 firing
#[derive(Clone, Debug)]
pub struct TraceFiring {
    /// Transition 名称
    pub name: String,
    /// 消耗的类型
    pub consumes: Vec<TypeKey>,
    /// 产生的类型
    pub produces: Vec<TypeKey>,
}

impl fmt::Display for TraceFiring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        if !self.consumes.is_empty() {
            let c: Vec<_> = self.consumes.iter().map(|ty| ty.short_name()).collect();
            write!(f, " consumes [{}]", c.join(", "))?;
        }
        if !self.produces.is_empty() {
            let p: Vec<_> = self.produces.iter().map(|ty| ty.short_name()).collect();
            write!(f, " → [{}]", p.join(", "))?;
        }
        Ok(())
    }
}

// ==================== 仿真状态 ====================

/// 仿真状态 - 简化版
#[derive(Clone, Debug)]
pub struct SimState {
    /// Marking: Place -> token 数量
    pub marking: HashMap<PlaceId, usize>,
    /// Dup（Copy/Clone）使用计数: TypeKey -> 次数
    pub dup_count: HashMap<TypeKey, usize>,
}

impl SimState {
    pub fn new() -> Self {
        SimState {
            marking: HashMap::new(),
            dup_count: HashMap::new(),
        }
    }

    /// 获取 place 的 token 数量
    pub fn count(&self, place: PlaceId) -> usize {
        *self.marking.get(&place).unwrap_or(&0)
    }

    /// 添加 token
    pub fn add(&mut self, place: PlaceId, count: usize) {
        *self.marking.entry(place).or_insert(0) += count;
    }

    /// 移除 token
    pub fn remove(&mut self, place: PlaceId, count: usize) -> bool {
        if let Some(current) = self.marking.get_mut(&place) {
            if *current >= count {
                *current -= count;
                return true;
            }
        }
        false
    }

    /// 获取 Dup 使用次数
    pub fn get_dup_count(&self, ty: &TypeKey) -> usize {
        *self.dup_count.get(ty).unwrap_or(&0)
    }

    /// 增加 Dup 使用次数
    pub fn inc_dup_count(&mut self, ty: &TypeKey) {
        *self.dup_count.entry(ty.clone()).or_insert(0) += 1;
    }

    /// 计算状态的哈希键（用于去重）
    pub fn hash_key(&self) -> String {
        let mut parts: Vec<String> = self
            .marking
            .iter()
            .filter(|&(_, &c)| c > 0)
            .map(|(p, c)| format!("p{}:{}", p, c))
            .collect();
        parts.sort();
        parts.join("|")
    }
}

impl Default for SimState {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 仿真配置 ====================

/// 仿真配置
#[derive(Clone, Copy, Debug)]
pub struct SimConfig {
    /// Dup（Copy/Clone）次数上限
    pub dup_limit: usize,
    /// 最大步数
    pub max_steps: usize,
    /// 最小步数（目标条件）
    pub min_steps: usize,
    /// 搜索策略
    pub strategy: SearchStrategy,
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            dup_limit: 2,
            max_steps: 20,
            min_steps: 3,
            strategy: SearchStrategy::Bfs,
        }
    }
}

/// 搜索策略
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchStrategy {
    Bfs,
    Dfs,
}

// ==================== 仿真结果 ====================

/// 仿真结果
#[derive(Clone, Debug)]
pub struct SimResult {
    /// 是否找到 witness
    pub found: bool,
    /// Trace（firing 序列）
    pub trace: Vec<TraceFiring>,
    /// 探索的状态数
    pub states_explored: usize,
}

// ==================== 仿真器 ====================

/// PCPN 仿真器（简化版）
pub struct Simulator<'a> {
    pcpn: &'a Pcpn,
    config: SimConfig,
}

impl<'a> Simulator<'a> {
    pub fn new(pcpn: &'a Pcpn, config: SimConfig) -> Self {
        Simulator { pcpn, config }
    }

    /// 运行仿真
    pub fn run(&self) -> SimResult {
        match self.config.strategy {
            SearchStrategy::Bfs => self.search_bfs(),
            SearchStrategy::Dfs => self.search_dfs(),
        }
    }

    /// BFS 搜索
    fn search_bfs(&self) -> SimResult {
        let initial = self.initial_state();
        let mut queue: VecDeque<(SimState, Vec<TraceFiring>)> = VecDeque::new();
        let mut visited: HashSet<String> = HashSet::new();

        queue.push_back((initial.clone(), Vec::new()));
        visited.insert(initial.hash_key());

        let mut states_explored = 0;

        while let Some((state, trace)) = queue.pop_front() {
            states_explored += 1;

            // 检查步数限制
            if trace.len() >= self.config.max_steps {
                continue;
            }

            // 检查目标
            if self.check_goal(&state, &trace) {
                return SimResult {
                    found: true,
                    trace,
                    states_explored,
                };
            }

            // 生成所有可发生的 transitions
            for trans in &self.pcpn.transitions {
                if self.enabled(trans, &state) {
                    if let Some((next_state, firing)) = self.fire(trans, &state) {
                        // 检查 bounds
                        if !self.check_bounds(&next_state) {
                            continue;
                        }

                        let hash = next_state.hash_key();
                        if !visited.contains(&hash) {
                            visited.insert(hash);
                            let mut next_trace = trace.clone();
                            next_trace.push(firing);
                            queue.push_back((next_state, next_trace));
                        }
                    }
                }
            }
        }

        SimResult {
            found: false,
            trace: Vec::new(),
            states_explored,
        }
    }

    /// DFS 搜索
    fn search_dfs(&self) -> SimResult {
        let initial = self.initial_state();
        let mut stack: Vec<(SimState, Vec<TraceFiring>)> = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();

        stack.push((initial.clone(), Vec::new()));
        visited.insert(initial.hash_key());

        let mut states_explored = 0;

        while let Some((state, trace)) = stack.pop() {
            states_explored += 1;

            if trace.len() >= self.config.max_steps {
                continue;
            }

            if self.check_goal(&state, &trace) {
                return SimResult {
                    found: true,
                    trace,
                    states_explored,
                };
            }

            for trans in &self.pcpn.transitions {
                if self.enabled(trans, &state) {
                    if let Some((next_state, firing)) = self.fire(trans, &state) {
                        if !self.check_bounds(&next_state) {
                            continue;
                        }

                        let hash = next_state.hash_key();
                        if !visited.contains(&hash) {
                            visited.insert(hash);
                            let mut next_trace = trace.clone();
                            next_trace.push(firing);
                            stack.push((next_state, next_trace));
                        }
                    }
                }
            }
        }

        SimResult {
            found: false,
            trace: Vec::new(),
            states_explored,
        }
    }

    /// 创建初始状态
    ///
    /// Primitive 类型：每个 1 token
    fn initial_state(&self) -> SimState {
        let mut state = SimState::new();

        for place in &self.pcpn.places {
            if place.is_primitive && !place.is_ref {
                state.add(place.id, 1);
            }
        }

        state
    }

    // ==================== Firing 判定（简化版）====================

    /// 统一的 firing 判定
    /// enabled(t, M) := structural_enabled ∧ dup_limit_ok
    fn enabled(&self, trans: &Transition, state: &SimState) -> bool {
        self.structural_enabled(trans, state) && self.dup_limit_ok(trans, state)
    }

    /// (1) 结构可发生性：所有前置库所都有足够 token
    fn structural_enabled(&self, trans: &Transition, state: &SimState) -> bool {
        // 统计每个 place 需要消耗的 token 数
        let mut required: HashMap<PlaceId, usize> = HashMap::new();

        for arc in &trans.input_arcs {
            if arc.consumes {
                *required.entry(arc.place_id).or_insert(0) += 1;
            } else {
                // 非消耗弧也需要至少有 token（用于读取）
                required.entry(arc.place_id).or_insert(1);
            }
        }

        for (place_id, count) in required {
            if state.count(place_id) < count {
                return false;
            }
        }

        true
    }

    /// (2) Dup（Copy/Clone）限制检查
    fn dup_limit_ok(&self, trans: &Transition, state: &SimState) -> bool {
        match &trans.kind {
            TransitionKind::DupCopy { type_key } | TransitionKind::DupClone { type_key } => {
                state.get_dup_count(type_key) < self.config.dup_limit
            }
            TransitionKind::CreatePrimitive { type_key } => {
                // Primitive 创建也受限制
                state.get_dup_count(type_key) < self.config.dup_limit
            }
            _ => true,
        }
    }

    /// Fire transition，返回新状态和 firing 记录
    fn fire(&self, trans: &Transition, state: &SimState) -> Option<(SimState, TraceFiring)> {
        let mut new_state = state.clone();
        let mut consumes = Vec::new();
        let mut produces = Vec::new();

        // 消耗输入 token
        for arc in &trans.input_arcs {
            if arc.consumes {
                if !new_state.remove(arc.place_id, 1) {
                    return None;
                }
                let place = &self.pcpn.places[arc.place_id];
                consumes.push(place.type_key.clone());
            }
        }

        // 产生输出 token
        for arc in &trans.output_arcs {
            new_state.add(arc.place_id, 1);
            let place = &self.pcpn.places[arc.place_id];
            produces.push(place.type_key.clone());
        }

        // 处理 Dup 计数
        match &trans.kind {
            TransitionKind::DupCopy { type_key }
            | TransitionKind::DupClone { type_key }
            | TransitionKind::CreatePrimitive { type_key } => {
                new_state.inc_dup_count(type_key);
            }
            _ => {}
        }

        let firing = TraceFiring {
            name: trans.name.clone(),
            consumes,
            produces,
        };

        Some((new_state, firing))
    }

    /// 检查 bounds（budget）
    fn check_bounds(&self, state: &SimState) -> bool {
        for place in &self.pcpn.places {
            let count = state.count(place.id);
            if count > place.budget {
                return false;
            }
        }
        true
    }

    /// 检查目标条件
    fn check_goal(&self, _state: &SimState, trace: &[TraceFiring]) -> bool {
        // 达到最小步数且有非 primitive 类型的操作
        if trace.len() < self.config.min_steps {
            return false;
        }

        // 检查 trace 中是否有有意义的操作
        let has_meaningful = trace.iter().any(|f| {
            // 非 primitive 的产生
            f.produces.iter().any(|ty| !ty.is_primitive())
                // 或者非 primitive 的消耗
                || f.consumes.iter().any(|ty| !ty.is_primitive())
        });

        has_meaningful
    }

    /// 生成可达图
    pub fn generate_reachability_graph(&self, max_states: usize) -> ReachabilityGraph {
        let initial = self.initial_state();
        let mut states: Vec<SimState> = Vec::new();
        let mut state_ids: HashMap<String, usize> = HashMap::new();
        let mut edges: Vec<(usize, usize, String)> = Vec::new();
        let mut queue: VecDeque<SimState> = VecDeque::new();

        let initial_hash = initial.hash_key();
        state_ids.insert(initial_hash.clone(), 0);
        states.push(initial.clone());
        queue.push_back(initial);

        while let Some(state) = queue.pop_front() {
            if states.len() >= max_states {
                break;
            }

            let from_id = *state_ids.get(&state.hash_key()).unwrap();

            for trans in &self.pcpn.transitions {
                if self.enabled(trans, &state) {
                    if let Some((next_state, _)) = self.fire(trans, &state) {
                        if !self.check_bounds(&next_state) {
                            continue;
                        }

                        let next_hash = next_state.hash_key();
                        let to_id = if let Some(&id) = state_ids.get(&next_hash) {
                            id
                        } else {
                            let id = states.len();
                            state_ids.insert(next_hash, id);
                            states.push(next_state.clone());
                            queue.push_back(next_state);
                            id
                        };

                        edges.push((from_id, to_id, trans.name.clone()));
                    }
                }
            }
        }

        ReachabilityGraph { states, edges }
    }
}

// ==================== 可达图 ====================

/// 可达图结构
pub struct ReachabilityGraph {
    pub states: Vec<SimState>,
    pub edges: Vec<(usize, usize, String)>,
}

impl ReachabilityGraph {
    /// 输出 DOT 格式
    pub fn to_dot(&self, pcpn: &Pcpn) -> String {
        let mut dot = String::new();
        dot.push_str("digraph ReachabilityGraph {\n");
        dot.push_str("  rankdir=TB;\n");
        dot.push_str("  node [shape=box, style=filled, fillcolor=lightyellow];\n");
        dot.push_str("\n");

        for (i, state) in self.states.iter().enumerate() {
            let label = self.state_label(state, pcpn);
            let fillcolor = if i == 0 { "lightgreen" } else { "lightyellow" };
            dot.push_str(&format!(
                "  s{} [label=\"s{}\\n{}\", fillcolor={}];\n",
                i, i, label, fillcolor
            ));
        }

        dot.push_str("\n");

        for (from, to, label) in &self.edges {
            let short_label = if label.len() > 25 {
                format!("{}...", &label[..22])
            } else {
                label.clone()
            };
            dot.push_str(&format!(
                "  s{} -> s{} [label=\"{}\"];\n",
                from,
                to,
                short_label.replace('"', "\\\"")
            ));
        }

        dot.push_str("}\n");
        dot
    }

    fn state_label(&self, state: &SimState, pcpn: &Pcpn) -> String {
        let mut parts = Vec::new();

        let mut places: Vec<_> = state.marking.iter().filter(|(_, c)| **c > 0).collect();
        places.sort_by_key(|(p, _)| *p);

        for (place_id, count) in places.iter().take(5) {
            let place_name = pcpn
                .places
                .get(**place_id)
                .map(|p| {
                    let name = p.type_key.short_name();
                    if name.len() > 12 {
                        format!("{}...", &name[..9])
                    } else {
                        name
                    }
                })
                .unwrap_or_else(|| format!("p{}", place_id));
            parts.push(format!("{}:{}", place_name, count));
        }

        if parts.is_empty() {
            "∅".to_string()
        } else {
            parts.join("\\n")
        }
    }

    pub fn stats(&self) -> String {
        format!(
            "可达图: {} 个状态, {} 条边",
            self.states.len(),
            self.edges.len()
        )
    }
}

// ==================== 辅助函数 ====================

/// 打印 trace
pub fn print_trace(trace: &[TraceFiring]) {
    println!("=== Abstract Trace ({} steps) ===", trace.len());
    for (i, firing) in trace.iter().enumerate() {
        println!("  {}. {}", i + 1, firing);
    }
}

// ==================== 兼容旧接口 ====================

/// Token - 兼容旧接口
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Token {
    pub type_key: TypeKey,
    pub capability: crate::pcpn::Capability,
    pub lifetime: Option<u32>,
}

impl Token {
    pub fn owned(type_key: TypeKey) -> Self {
        Token {
            type_key,
            capability: crate::pcpn::Capability::Own,
            lifetime: None,
        }
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.type_key.short_name())
    }
}

/// 兼容旧的 Capability 引用
#[allow(unused_imports)]
pub use crate::pcpn::Capability;
