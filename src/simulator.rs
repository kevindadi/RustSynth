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
    /// Marking: Place -> Token 实例列表
    pub marking: HashMap<PlaceId, Vec<crate::pcpn::Token>>,
    /// 下一个可用的 Token ID
    pub next_token_id: crate::pcpn::TokenId,
    /// Dup（Copy/Clone）使用计数: TypeKey -> 次数
    pub dup_count: HashMap<TypeKey, usize>,
    /// 生命周期栈
    pub lifetime_stack: crate::pcpn::LifetimeStack,
}

impl SimState {
    pub fn new() -> Self {
        SimState {
            marking: HashMap::new(),
            next_token_id: 0,
            dup_count: HashMap::new(),
            lifetime_stack: crate::pcpn::LifetimeStack::new(),
        }
    }

    /// 获取 place 的 token 数量
    pub fn count(&self, place: PlaceId) -> usize {
        self.marking
            .get(&place)
            .map(|tokens| tokens.len())
            .unwrap_or(0)
    }

    /// 添加 token 实例
    pub fn add_token(&mut self, token: crate::pcpn::Token, place: PlaceId) {
        self.marking
            .entry(place)
            .or_insert_with(Vec::new)
            .push(token);
    }

    /// 移除一个 token 实例（移除第一个）
    pub fn remove_token(&mut self, place: PlaceId) -> Option<crate::pcpn::Token> {
        if let Some(tokens) = self.marking.get_mut(&place) {
            if !tokens.is_empty() {
                return Some(tokens.remove(0));
            }
        }
        None
    }

    /// 兼容旧接口：添加指定数量的简单 token
    pub fn add(&mut self, place: PlaceId, count: usize) {
        for _ in 0..count {
            let token = crate::pcpn::Token::new_owned(
                self.next_token_id,
                crate::type_model::TypeKey::Primitive("i32".to_string()),
            );
            self.next_token_id += 1;
            self.add_token(token, place);
        }
    }

    /// 兼容旧接口：移除指定数量的 token
    pub fn remove(&mut self, place: PlaceId, count: usize) -> bool {
        for _ in 0..count {
            if self.remove_token(place).is_none() {
                return false;
            }
        }
        true
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
            .filter(|(_, tokens)| !tokens.is_empty())
            .map(|(p, tokens)| format!("p{}:{}", p, tokens.len()))
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
        let state = SimState::new();

        // 初始标识为空，所有 token（包括基本类型）通过变迁生成
        // CreatePrimitive 变迁持续使能，上限由 budget 控制（3 个）

        state
    }

    // ==================== Firing 判定（简化版）====================

    /// 统一的 firing 判定
    /// enabled(t, M) := structural_enabled ∧ dup_limit_ok ∧ guard_check
    fn enabled(&self, trans: &Transition, state: &SimState) -> bool {
        if !self.structural_enabled(trans, state) {
            return false;
        }
        if !self.dup_limit_ok(trans, state) {
            return false;
        }

        // 预览输入 tokens 用于 Guard 检查
        let input_tokens = self.peek_input_tokens(trans, state).unwrap_or_default();
        self.guard_check(trans, state, &input_tokens)
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
            TransitionKind::CreatePrimitive { type_key: _ } => {
                // CreatePrimitive 受 budget 限制（检查目标 place 的当前 token 数）
                if let Some(output_arc) = trans.output_arcs.first() {
                    let place_id = output_arc.place_id;
                    if let Some(place) = self.pcpn.places.get(place_id) {
                        return state.count(place_id) < place.budget;
                    }
                }
                false
            }
            _ => true,
        }
    }

    /// 预览输入 tokens（不实际移除）
    fn peek_input_tokens(&self, trans: &Transition, state: &SimState) -> Option<Vec<Token>> {
        let mut tokens = Vec::new();

        for arc in &trans.input_arcs {
            if arc.consumes {
                // 需要消耗的弧：预览第一个 token
                if let Some(place_tokens) = state.marking.get(&arc.place_id) {
                    if let Some(token) = place_tokens.first() {
                        tokens.push(token.clone());
                    } else {
                        return None; // 没有可用 token
                    }
                } else {
                    return None;
                }
            }
        }

        Some(tokens)
    }

    /// (3) Guard 检查：强制 Rust 借用规则
    fn guard_check(&self, trans: &Transition, state: &SimState, input_tokens: &[Token]) -> bool {
        use crate::pcpn::{Capability, GuardKind};

        for guard in &trans.guards {
            let type_key = &guard.type_key;

            match guard.kind {
                GuardKind::RequireOwn => {
                    // 传递所有权时，该类型的 shr 和 mut place 都不能有 token
                    // （不能有任何借用存在）
                    if let Some(&shr_place) = self
                        .pcpn
                        .type_cap_to_place
                        .get(&(type_key.clone(), Capability::Shr))
                    {
                        if state.count(shr_place) > 0 {
                            return false; // 有共享引用，无法传递所有权
                        }
                    }
                    if let Some(&mut_place) = self
                        .pcpn
                        .type_cap_to_place
                        .get(&(type_key.clone(), Capability::Mut))
                    {
                        if state.count(mut_place) > 0 {
                            return false; // 有可变借用，无法传递所有权
                        }
                    }
                }
                GuardKind::RequireShr => {
                    // 持有共享引用时，不能有可变借用存在
                    if let Some(&mut_place) = self
                        .pcpn
                        .type_cap_to_place
                        .get(&(type_key.clone(), Capability::Mut))
                    {
                        if state.count(mut_place) > 0 {
                            return false; // 有可变借用，无法创建共享引用
                        }
                    }
                }
                GuardKind::RequireMut => {
                    // 持有可变借用时，不能有共享引用或其他可变借用
                    if let Some(&shr_place) = self
                        .pcpn
                        .type_cap_to_place
                        .get(&(type_key.clone(), Capability::Shr))
                    {
                        if state.count(shr_place) > 0 {
                            return false; // 有共享引用，无法创建可变借用
                        }
                    }
                    // 注意：mut place 本身只能有 1 个 token（独占），这由 budget 控制
                }
                GuardKind::RequireNotBorrowed => {
                    // 检查具体 token 是否被生命周期栈阻塞
                    // 找到要操作的 token（第一个消耗型输入）
                    if let Some(token) = input_tokens.first() {
                        // 检查这个 token 是否被借用
                        if state.lifetime_stack.is_blocked(token.id) {
                            return false; // 被借用，不能 drop
                        }
                    } else {
                        // 没有输入 token，回退到类型级别检查
                        if let Some(&shr_place) = self
                            .pcpn
                            .type_cap_to_place
                            .get(&(type_key.clone(), Capability::Shr))
                        {
                            if state.count(shr_place) > 0 {
                                return false; // 有共享借用，不能 drop
                            }
                        }
                        if let Some(&mut_place) = self
                            .pcpn
                            .type_cap_to_place
                            .get(&(type_key.clone(), Capability::Mut))
                        {
                            if state.count(mut_place) > 0 {
                                return false; // 有可变借用，不能 drop
                            }
                        }
                    }
                }
            }
        }

        true // 所有 Guard 检查通过
    }

    /// Fire transition，返回新状态和 firing 记录
    fn fire(&self, trans: &Transition, state: &SimState) -> Option<(SimState, TraceFiring)> {
        use crate::pcpn::{Token, TransitionKind};

        let mut new_state = state.clone();
        let mut consumes = Vec::new();
        let mut produces = Vec::new();

        // 根据变迁类型进行特殊处理
        match &trans.kind {
            TransitionKind::BorrowMut { base_type, .. } => {
                // 可变借用：从 own place 取 token，在 mut place 生成借用 token
                if let Some(input_arc) = trans.input_arcs.first() {
                    if let Some(source_token) = new_state.remove_token(input_arc.place_id) {
                        consumes.push(source_token.type_key.clone());

                        // 创建可变借用 token
                        let borrow_token = Token::borrow_mut(
                            new_state.next_token_id,
                            base_type.clone(),
                            source_token.id,
                            None, // TODO: 从函数签名提取生命周期
                        );
                        new_state.next_token_id += 1;

                        if let Some(output_arc) = trans.output_arcs.first() {
                            new_state.add_token(borrow_token.clone(), output_arc.place_id);
                            produces.push(borrow_token.type_key);
                        }
                    } else {
                        return None;
                    }
                }
            }

            TransitionKind::BorrowShr { base_type, .. } => {
                // 共享借用：从 own place 取 token，在 shr place 生成借用 token
                if let Some(input_arc) = trans.input_arcs.first() {
                    if let Some(source_token) = new_state.remove_token(input_arc.place_id) {
                        consumes.push(source_token.type_key.clone());

                        // 创建共享借用 token
                        let borrow_token = Token::borrow_shr(
                            new_state.next_token_id,
                            base_type.clone(),
                            source_token.id,
                            None, // TODO: 从函数签名提取生命周期
                        );
                        new_state.next_token_id += 1;

                        if let Some(output_arc) = trans.output_arcs.first() {
                            new_state.add_token(borrow_token.clone(), output_arc.place_id);
                            produces.push(borrow_token.type_key);
                        }
                    } else {
                        return None;
                    }
                }
            }

            TransitionKind::EndBorrowMut { .. } | TransitionKind::EndBorrowShr { .. } => {
                // 结束借用：从借用 place 取 token，恢复原 token 到 own place
                if let Some(input_arc) = trans.input_arcs.first() {
                    if let Some(borrow_token) = new_state.remove_token(input_arc.place_id) {
                        consumes.push(borrow_token.type_key.clone());

                        // 弹栈：移除包含此 borrow_token 的生命周期帧
                        let _unblocked_tokens =
                            new_state.lifetime_stack.remove_borrow(borrow_token.id);

                        // 恢复原 token（简化：创建新的 owned token）
                        if let Some(output_arc) = trans.output_arcs.first() {
                            let restored_token = Token::new_owned(
                                new_state.next_token_id,
                                borrow_token.type_key.clone(),
                            );
                            new_state.next_token_id += 1;
                            new_state.add_token(restored_token.clone(), output_arc.place_id);
                            produces.push(restored_token.type_key);
                        }
                    } else {
                        return None;
                    }
                }
            }

            TransitionKind::DerefRef { .. } => {
                // 解引用：降低 ref_level
                if let Some(input_arc) = trans.input_arcs.first() {
                    if let Some(ref_token) = new_state.remove_token(input_arc.place_id) {
                        consumes.push(ref_token.type_key.clone());

                        // 解引用：ref_level - 1
                        if let Some(deref_token) = ref_token.deref(new_state.next_token_id) {
                            new_state.next_token_id += 1;
                            if let Some(output_arc) = trans.output_arcs.first() {
                                new_state.add_token(deref_token.clone(), output_arc.place_id);
                                produces.push(deref_token.type_key);
                            }
                        } else {
                            return None; // 无法解引用（ref_level 已经是 0）
                        }
                    } else {
                        return None;
                    }
                }
            }

            TransitionKind::CreatePrimitive { type_key } => {
                // 创建 primitive token
                let token = Token::new_owned(new_state.next_token_id, type_key.clone());
                new_state.next_token_id += 1;

                if let Some(output_arc) = trans.output_arcs.first() {
                    new_state.add_token(token.clone(), output_arc.place_id);
                    produces.push(token.type_key);
                }
                new_state.inc_dup_count(type_key);
            }

            TransitionKind::ApiCall { fn_id } => {
                // API 调用：特殊处理生命周期绑定
                let mut consumed_tokens = Vec::new();
                let mut produced_tokens = Vec::new();

                // 处理输入
                for arc in &trans.input_arcs {
                    if arc.consumes {
                        if let Some(token) = new_state.remove_token(arc.place_id) {
                            consumes.push(token.type_key.clone());
                            consumed_tokens.push(token);
                        } else {
                            return None;
                        }
                    }
                }

                // 处理输出
                for arc in &trans.output_arcs {
                    let place = &self.pcpn.places[arc.place_id];

                    // 创建新 token
                    let token = Token::new_owned(new_state.next_token_id, place.type_key.clone());
                    new_state.next_token_id += 1;

                    new_state.add_token(token.clone(), arc.place_id);
                    produces.push(token.type_key.clone());
                    produced_tokens.push(token);
                }

                // 生命周期绑定：检查返回值是否是引用
                // 简化版本：如果返回值在 shr 或 mut place，则认为是返回引用
                if !produced_tokens.is_empty() && !consumed_tokens.is_empty() {
                    let return_token = &produced_tokens[0];

                    // 检查返回值的 place 是否是 shr 或 mut
                    if let Some(output_arc) = trans.output_arcs.first() {
                        let return_place = &self.pcpn.places[output_arc.place_id];

                        if return_place.capability == Capability::Shr
                            || return_place.capability == Capability::Mut
                        {
                            // 这是返回引用！需要绑定到 self 参数（第一个 consumed token）
                            let source_token = &consumed_tokens[0];

                            // 生成生命周期标识（使用函数名和 token ID）
                            let lifetime = format!("'fn{}_{}", fn_id, source_token.id);

                            // 压栈：先创建帧，然后记录借用关系
                            new_state.lifetime_stack.push_frame(lifetime.clone());
                            new_state.lifetime_stack.add_borrow(
                                &lifetime,
                                return_token.id,
                                source_token.id,
                            );
                        }
                    }
                }
            }

            _ => {
                // 其他变迁：使用旧逻辑（兼容性）
                for arc in &trans.input_arcs {
                    if arc.consumes {
                        if !new_state.remove(arc.place_id, 1) {
                            return None;
                        }
                        let place = &self.pcpn.places[arc.place_id];
                        consumes.push(place.type_key.clone());
                    }
                }

                for arc in &trans.output_arcs {
                    new_state.add(arc.place_id, 1);
                    let place = &self.pcpn.places[arc.place_id];
                    produces.push(place.type_key.clone());
                }
            }
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
        use crate::pcpn::Capability;
        let mut parts = Vec::new();

        let mut places: Vec<_> = state
            .marking
            .iter()
            .filter(|(_, tokens)| !tokens.is_empty())
            .collect();
        places.sort_by_key(|(p, _)| *p);

        for (place_id, tokens) in places.iter().take(8) {
            // 增加显示数量到 8
            let count = tokens.len();
            let place_info = pcpn
                .places
                .get(**place_id)
                .map(|p| {
                    let mut name = p.type_key.short_name();
                    if name.len() > 12 {
                        name = format!("{}...", &name[..9]);
                    }
                    // 添加 capability 标注
                    let cap = match p.capability {
                        Capability::Own => "[own]",
                        Capability::Shr => "[shr]",
                        Capability::Mut => "[mut]",
                    };
                    format!("{}{}", name, cap)
                })
                .unwrap_or_else(|| format!("p{}", place_id));
            parts.push(format!("{}:{}", place_info, count));
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

// 使用 PCPN 模块中定义的 Token
pub use crate::pcpn::Token;

/// 兼容旧的 Capability 引用
#[allow(unused_imports)]
pub use crate::pcpn::Capability;
