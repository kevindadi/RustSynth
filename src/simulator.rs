//! PCPN Bounded Simulator - 有界仿真器
//!
//! 简化的状态空间探索器，用于验证 PCPN 的可达性：
//! - 状态 = (marking: multiset, stack: Vec<Frame>)
//! - Bounds: place token 上限, 栈深度上限, 最大步数
//! - BFS/DFS 搜索找到 witness trace

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use crate::pcpn::{Arc, Capability, Pcpn, PlaceId, Transition, TransitionId, TransitionKind};

/// 变量 ID (用于着色 token)
pub type VarId = u32;

/// Token (着色 token)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Token {
    /// 变量 ID (着色)
    pub vid: VarId,
    /// 如果是借用，指向 origin
    pub origin: Option<VarId>,
}

impl Token {
    pub fn new(vid: VarId) -> Self {
        Token { vid, origin: None }
    }

    pub fn borrowed(vid: VarId, origin: VarId) -> Self {
        Token {
            vid,
            origin: Some(origin),
        }
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(origin) = self.origin {
            write!(f, "v{}←v{}", self.vid, origin)
        } else {
            write!(f, "v{}", self.vid)
        }
    }
}

/// 栈帧 (用于借用追踪)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Frame {
    /// 借用类型
    pub cap: Capability,
    /// 被借用的 owner vid
    pub owner: VarId,
    /// 借用引用的 vid
    pub reference: VarId,
}

/// Marking (place -> multiset of tokens)
pub type Marking = HashMap<PlaceId, Vec<Token>>;

/// 仿真状态
#[derive(Clone, Debug)]
pub struct SimState {
    /// 当前 marking
    pub marking: Marking,
    /// 借用栈
    pub stack: Vec<Frame>,
    /// 下一个可用的 VarId
    pub next_vid: VarId,
}

impl SimState {
    /// 创建空状态
    pub fn new() -> Self {
        SimState {
            marking: HashMap::new(),
            stack: Vec::new(),
            next_vid: 0,
        }
    }

    /// 分配新的 VarId
    pub fn alloc_vid(&mut self) -> VarId {
        let vid = self.next_vid;
        self.next_vid += 1;
        vid
    }

    /// 添加 token 到 place
    pub fn add_token(&mut self, place: PlaceId, token: Token) {
        self.marking.entry(place).or_default().push(token);
    }

    /// 移除 token (by vid)
    pub fn remove_token(&mut self, place: PlaceId, vid: VarId) -> Option<Token> {
        if let Some(tokens) = self.marking.get_mut(&place) {
            if let Some(pos) = tokens.iter().position(|t| t.vid == vid) {
                return Some(tokens.remove(pos));
            }
        }
        None
    }

    /// 获取 place 的所有 tokens
    pub fn get_tokens(&self, place: PlaceId) -> &[Token] {
        self.marking.get(&place).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// 统计 place 的 token 数量
    pub fn token_count(&self, place: PlaceId) -> usize {
        self.marking.get(&place).map(|v| v.len()).unwrap_or(0)
    }

    /// 获取状态的规范化 hash key (用于去重)
    pub fn hash_key(&self) -> String {
        let mut parts = Vec::new();
        
        // Marking 部分
        let mut places: Vec<_> = self.marking.iter().collect();
        places.sort_by_key(|(p, _)| *p);
        for (place, tokens) in places {
            if !tokens.is_empty() {
                let mut vids: Vec<_> = tokens.iter().map(|t| t.vid).collect();
                vids.sort();
                parts.push(format!("p{}:{:?}", place, vids));
            }
        }
        
        // Stack 部分
        for frame in &self.stack {
            parts.push(format!("S({:?},v{},v{})", frame.cap, frame.owner, frame.reference));
        }
        
        parts.join("|")
    }
}

impl Default for SimState {
    fn default() -> Self {
        Self::new()
    }
}

/// Firing 记录 (用于 trace)
#[derive(Clone, Debug)]
pub struct Firing {
    /// 触发的 transition
    pub transition_id: TransitionId,
    /// Transition 名称
    pub name: String,
    /// 输入绑定: (place_id, vid)
    pub input_bindings: Vec<(PlaceId, VarId)>,
    /// 输出绑定: (place_id, vid)
    pub output_bindings: Vec<(PlaceId, VarId)>,
}

impl fmt::Display for Firing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        if !self.input_bindings.is_empty() {
            write!(f, " [")?;
            for (i, (_, vid)) in self.input_bindings.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "v{}", vid)?;
            }
            write!(f, "]")?;
        }
        if !self.output_bindings.is_empty() {
            write!(f, " → [")?;
            for (i, (_, vid)) in self.output_bindings.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "v{}", vid)?;
            }
            write!(f, "]")?;
        }
        Ok(())
    }
}

/// 仿真配置 (Bounds)
#[derive(Clone, Debug)]
pub struct SimConfig {
    /// 每个 place 的 token 上限
    pub max_tokens_per_place: usize,
    /// 栈深度上限
    pub max_stack_depth: usize,
    /// 最大步数
    pub max_steps: usize,
    /// 最小步数（目标条件）
    pub min_steps: usize,
    /// 搜索策略
    pub strategy: SearchStrategy,
    /// 目标 place (找到有 token 的状态)
    pub target_place: Option<PlaceId>,
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            max_tokens_per_place: 3,
            max_stack_depth: 5,
            max_steps: 100,
            min_steps: 3,
            strategy: SearchStrategy::Bfs,
            target_place: None,
        }
    }
}

/// 搜索策略
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchStrategy {
    Bfs,
    Dfs,
}

/// 仿真结果
#[derive(Debug)]
pub struct SimResult {
    /// 是否找到目标
    pub found: bool,
    /// 最终状态
    pub final_state: SimState,
    /// Firing 序列
    pub trace: Vec<Firing>,
    /// 探索的状态数
    pub states_explored: usize,
}

/// PCPN 仿真器
pub struct Simulator<'a> {
    /// PCPN 网络
    pcpn: &'a Pcpn,
    /// 配置
    config: SimConfig,
}

impl<'a> Simulator<'a> {
    /// 创建仿真器
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
        let initial = SimState::new();
        let mut queue: VecDeque<(SimState, Vec<Firing>)> = VecDeque::new();
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
                    final_state: state,
                    trace,
                    states_explored,
                };
            }

            // 生成所有 enabled transitions 并 fire
            for trans in &self.pcpn.transitions {
                for firing in self.compute_enabled_bindings(&state, trans) {
                    if let Some(next_state) = self.fire(&state, trans, &firing) {
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
            final_state: SimState::new(),
            trace: Vec::new(),
            states_explored,
        }
    }

    /// DFS 搜索
    fn search_dfs(&self) -> SimResult {
        let initial = SimState::new();
        let mut stack: Vec<(SimState, Vec<Firing>)> = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();

        stack.push((initial.clone(), Vec::new()));
        visited.insert(initial.hash_key());

        let mut states_explored = 0;

        while let Some((state, trace)) = stack.pop() {
            states_explored += 1;

            // 检查步数限制
            if trace.len() >= self.config.max_steps {
                continue;
            }

            // 检查目标
            if self.check_goal(&state, &trace) {
                return SimResult {
                    found: true,
                    final_state: state,
                    trace,
                    states_explored,
                };
            }

            // 生成所有 enabled transitions 并 fire
            for trans in &self.pcpn.transitions {
                for firing in self.compute_enabled_bindings(&state, trans) {
                    if let Some(next_state) = self.fire(&state, trans, &firing) {
                        // 检查 bounds
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
            final_state: SimState::new(),
            trace: Vec::new(),
            states_explored,
        }
    }

    /// 计算 transition 的所有 enabled bindings
    fn compute_enabled_bindings(&self, state: &SimState, trans: &Transition) -> Vec<Firing> {
        let mut firings = Vec::new();

        match &trans.kind {
            // Primitive 创建：无输入要求，始终 enabled
            TransitionKind::CreatePrimitive { .. } => {
                firings.push(Firing {
                    transition_id: trans.id,
                    name: trans.name.clone(),
                    input_bindings: Vec::new(),
                    output_bindings: Vec::new(), // 会在 fire 时填充
                });
            }

            // API 调用：需要检查所有输入 place 是否有足够的 token
            TransitionKind::ApiCall { .. } => {
                if let Some(bindings) = self.find_input_bindings(state, &trans.input_arcs) {
                    firings.push(Firing {
                        transition_id: trans.id,
                        name: trans.name.clone(),
                        input_bindings: bindings,
                        output_bindings: Vec::new(),
                    });
                }
            }

            // BorrowShr: 需要 own place 有 token，且 owner 可以 borrow_shr
            TransitionKind::BorrowShr { .. } => {
                for arc in &trans.input_arcs {
                    for token in state.get_tokens(arc.place_id) {
                        // 检查是否可以借用（简化：栈上没有冲突的 mut borrow）
                        if self.can_borrow_shr(state, token.vid) {
                            firings.push(Firing {
                                transition_id: trans.id,
                                name: trans.name.clone(),
                                input_bindings: vec![(arc.place_id, token.vid)],
                                output_bindings: Vec::new(),
                            });
                        }
                    }
                }
            }

            // BorrowMut: 需要 own place 有 token，且 owner 可以 borrow_mut
            TransitionKind::BorrowMut { .. } => {
                for arc in &trans.input_arcs {
                    for token in state.get_tokens(arc.place_id) {
                        if self.can_borrow_mut(state, token.vid) {
                            firings.push(Firing {
                                transition_id: trans.id,
                                name: trans.name.clone(),
                                input_bindings: vec![(arc.place_id, token.vid)],
                                output_bindings: Vec::new(),
                            });
                        }
                    }
                }
            }

            // EndBorrowShr: 需要 shr place 有 token，且栈顶匹配
            TransitionKind::EndBorrowShr { .. } => {
                for arc in &trans.input_arcs {
                    for token in state.get_tokens(arc.place_id) {
                        // 不要求 LIFO（简化）
                        firings.push(Firing {
                            transition_id: trans.id,
                            name: trans.name.clone(),
                            input_bindings: vec![(arc.place_id, token.vid)],
                            output_bindings: Vec::new(),
                        });
                    }
                }
            }

            // EndBorrowMut: 需要 mut place 有 token，且栈顶匹配
            TransitionKind::EndBorrowMut { .. } => {
                for arc in &trans.input_arcs {
                    for token in state.get_tokens(arc.place_id) {
                        // 检查栈顶是否匹配
                        if self.stack_top_matches(state, Capability::Mut, token.vid) {
                            firings.push(Firing {
                                transition_id: trans.id,
                                name: trans.name.clone(),
                                input_bindings: vec![(arc.place_id, token.vid)],
                                output_bindings: Vec::new(),
                            });
                        }
                    }
                }
            }

            // Drop: 需要 own place 有 token，且没有活跃借用
            TransitionKind::Drop { .. } => {
                for arc in &trans.input_arcs {
                    for token in state.get_tokens(arc.place_id) {
                        if self.can_drop(state, token.vid) {
                            firings.push(Firing {
                                transition_id: trans.id,
                                name: trans.name.clone(),
                                input_bindings: vec![(arc.place_id, token.vid)],
                                output_bindings: Vec::new(),
                            });
                        }
                    }
                }
            }
        }

        firings
    }

    /// 查找输入 binding（简化：每个 input arc 选一个 token）
    fn find_input_bindings(&self, state: &SimState, arcs: &[Arc]) -> Option<Vec<(PlaceId, VarId)>> {
        let mut bindings = Vec::new();
        let mut used_vids = HashSet::new();

        for arc in arcs {
            let tokens = state.get_tokens(arc.place_id);
            // 找一个还没用过的 token
            let found = tokens.iter().find(|t| !used_vids.contains(&t.vid));
            if let Some(token) = found {
                bindings.push((arc.place_id, token.vid));
                if arc.consumes {
                    used_vids.insert(token.vid);
                }
            } else {
                return None; // 无法满足
            }
        }

        Some(bindings)
    }

    /// 检查是否可以 borrow_shr
    fn can_borrow_shr(&self, state: &SimState, owner_vid: VarId) -> bool {
        // 没有该 owner 的 mut borrow 在栈上
        !state.stack.iter().any(|f| f.owner == owner_vid && f.cap == Capability::Mut)
    }

    /// 检查是否可以 borrow_mut
    fn can_borrow_mut(&self, state: &SimState, owner_vid: VarId) -> bool {
        // 没有该 owner 的任何 borrow 在栈上
        !state.stack.iter().any(|f| f.owner == owner_vid)
    }

    /// 检查栈顶是否匹配
    fn stack_top_matches(&self, state: &SimState, cap: Capability, ref_vid: VarId) -> bool {
        if let Some(top) = state.stack.last() {
            top.cap == cap && top.reference == ref_vid
        } else {
            false
        }
    }

    /// 检查是否可以 drop
    fn can_drop(&self, state: &SimState, owner_vid: VarId) -> bool {
        // 没有该 owner 的任何 borrow 在栈上
        !state.stack.iter().any(|f| f.owner == owner_vid)
    }

    /// 执行 firing
    fn fire(&self, state: &SimState, trans: &Transition, firing: &Firing) -> Option<SimState> {
        let mut new_state = state.clone();

        match &trans.kind {
            TransitionKind::CreatePrimitive { .. } => {
                // 创建新的 primitive token
                let vid = new_state.alloc_vid();
                for arc in &trans.output_arcs {
                    new_state.add_token(arc.place_id, Token::new(vid));
                }
            }

            TransitionKind::ApiCall { .. } => {
                // 消耗输入 tokens
                for (place_id, vid) in &firing.input_bindings {
                    // 找到对应的 arc 检查是否 consumes
                    if let Some(arc) = trans.input_arcs.iter().find(|a| a.place_id == *place_id) {
                        if arc.consumes {
                            new_state.remove_token(*place_id, *vid);
                        }
                    }
                }
                // 生成输出 tokens
                for arc in &trans.output_arcs {
                    let vid = new_state.alloc_vid();
                    new_state.add_token(arc.place_id, Token::new(vid));
                }
            }

            TransitionKind::BorrowShr { .. } => {
                // 输入 arc 不消耗（读取弧）
                // 生成 shr token
                let owner_vid = firing.input_bindings.first()?.1;
                let ref_vid = new_state.alloc_vid();
                for arc in &trans.output_arcs {
                    new_state.add_token(arc.place_id, Token::borrowed(ref_vid, owner_vid));
                }
                // Push to stack
                new_state.stack.push(Frame {
                    cap: Capability::Shr,
                    owner: owner_vid,
                    reference: ref_vid,
                });
            }

            TransitionKind::BorrowMut { .. } => {
                // 消耗 own token（转移到 mut place）
                let (place_id, owner_vid) = firing.input_bindings.first()?;
                new_state.remove_token(*place_id, *owner_vid);
                
                let ref_vid = new_state.alloc_vid();
                for arc in &trans.output_arcs {
                    new_state.add_token(arc.place_id, Token::borrowed(ref_vid, *owner_vid));
                }
                // Push to stack
                new_state.stack.push(Frame {
                    cap: Capability::Mut,
                    owner: *owner_vid,
                    reference: ref_vid,
                });
            }

            TransitionKind::EndBorrowShr { .. } => {
                // 移除 shr token
                let (place_id, ref_vid) = firing.input_bindings.first()?;
                let token = new_state.remove_token(*place_id, *ref_vid)?;
                
                // 从栈中移除对应的 frame
                if let Some(pos) = new_state.stack.iter().position(|f| f.reference == *ref_vid) {
                    new_state.stack.remove(pos);
                }
                drop(token);
            }

            TransitionKind::EndBorrowMut { .. } => {
                // 移除 mut token，恢复 own token
                let (place_id, ref_vid) = firing.input_bindings.first()?;
                let token = new_state.remove_token(*place_id, *ref_vid)?;
                
                // 从栈中移除对应的 frame，并恢复 owner
                if let Some(pos) = new_state.stack.iter().position(|f| f.reference == *ref_vid) {
                    let frame = new_state.stack.remove(pos);
                    // 恢复 owner 到 own place
                    for out_arc in &trans.output_arcs {
                        new_state.add_token(out_arc.place_id, Token::new(frame.owner));
                    }
                }
                drop(token);
            }

            TransitionKind::Drop { .. } => {
                // 移除 token
                let (place_id, vid) = firing.input_bindings.first()?;
                new_state.remove_token(*place_id, *vid);
            }
        }

        Some(new_state)
    }

    /// 检查是否满足 bounds
    fn check_bounds(&self, state: &SimState) -> bool {
        // 检查每个 place 的 token 数量
        for (_, tokens) in &state.marking {
            if tokens.len() > self.config.max_tokens_per_place {
                return false;
            }
        }

        // 检查栈深度
        if state.stack.len() > self.config.max_stack_depth {
            return false;
        }

        true
    }

    /// 检查是否达到目标
    fn check_goal(&self, state: &SimState, trace: &[Firing]) -> bool {
        // 至少要有 min_steps 步骤
        if trace.len() < self.config.min_steps {
            return false;
        }

        // 如果指定了 target_place，检查是否有 token
        if let Some(target) = self.config.target_place {
            return state.token_count(target) > 0;
        }

        // 默认目标：栈为空（无未结束的借用），且有 token
        if !state.stack.is_empty() {
            return false;
        }

        let total_tokens: usize = state.marking.values().map(|v| v.len()).sum();
        total_tokens > 0
    }
}

/// 打印 trace
pub fn print_trace(trace: &[Firing]) {
    println!("=== Firing Sequence ({} steps) ===", trace.len());
    for (i, firing) in trace.iter().enumerate() {
        println!("  {}. {}", i + 1, firing);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_display() {
        let t1 = Token::new(0);
        assert_eq!(format!("{}", t1), "v0");

        let t2 = Token::borrowed(1, 0);
        assert_eq!(format!("{}", t2), "v1←v0");
    }

    #[test]
    fn test_state_hash() {
        let mut s1 = SimState::new();
        s1.add_token(0, Token::new(0));
        s1.add_token(1, Token::new(1));

        let mut s2 = SimState::new();
        s2.add_token(1, Token::new(1));
        s2.add_token(0, Token::new(0));

        // 不同顺序添加应该得到相同的 hash
        assert_eq!(s1.hash_key(), s2.hash_key());
    }
}
