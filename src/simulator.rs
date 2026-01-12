//! PCPN Bounded Simulator - 有界仿真器
//!
//! 工程化实现的状态空间探索器：
//! - 状态 = (marking: HashMap<PlaceId, Vec<Token>>, stack: Vec<Frame>, next_vid, next_region)
//! - Bounds: place token 上限, 栈深度上限, 最大步数
//! - BFS/DFS 搜索找到 witness trace
//!
//! ## Token 结构
//! ```ignore
//! Token {
//!     vid: u32,              // 变量 ID
//!     bind_mut: bool,        // 是否 let mut
//!     region: Option<u32>,   // None: owned, Some(L): 引用的 region
//! }
//! ```
//!
//! ## 栈帧结构
//! ```ignore
//! enum Frame {
//!     Freeze { owner: VarId },                           // 第一次共享借用
//!     Shr { owner: VarId, r: VarId, region: RegionId },  // 共享借用
//!     Mut { owner: VarId, r: VarId, region: RegionId },  // 可变借用
//! }
//! ```

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use crate::pcpn::{Arc, Pcpn, PlaceId, Transition, TransitionId, TransitionKind};
use crate::type_model::TypeKey;

/// 变量 ID (用于着色 token)
pub type VarId = u32;

/// Region ID (用于生命周期追踪)
pub type RegionId = u32;

/// Token (着色 token)
/// 
/// 约定：
/// - RefShr/RefMut token 必须有 region
/// - owned token 的 bind_mut 决定能否进行 &mut 借用
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Token {
    /// 变量 ID (着色)
    pub vid: VarId,
    /// 是否是 let mut 绑定
    pub bind_mut: bool,
    /// 如果是引用，关联的 region；None 表示 owned
    pub region: Option<RegionId>,
}

impl Token {
    /// 创建 owned token
    pub fn owned(vid: VarId, bind_mut: bool) -> Self {
        Token {
            vid,
            bind_mut,
            region: None,
        }
    }

    /// 创建 reference token
    pub fn reference(vid: VarId, region: RegionId) -> Self {
        Token {
            vid,
            bind_mut: false, // 引用本身不需要 mut
            region: Some(region),
        }
    }

    /// 是否是 owned token
    pub fn is_owned(&self) -> bool {
        self.region.is_none()
    }

    /// 是否是 reference token
    pub fn is_ref(&self) -> bool {
        self.region.is_some()
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut_marker = if self.bind_mut { "mut " } else { "" };
        if let Some(region) = self.region {
            write!(f, "{}v{}@r{}", mut_marker, self.vid, region)
        } else {
            write!(f, "{}v{}", mut_marker, self.vid)
        }
    }
}

/// 栈帧 (用于借用追踪)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Frame {
    /// 冻结帧：标记 owner 进入冻结状态（第一次共享借用）
    Freeze {
        owner: VarId,
        base_type: TypeKey,
    },
    /// 共享借用帧
    Shr {
        owner: VarId,
        r: VarId,
        region: RegionId,
        base_type: TypeKey,
    },
    /// 可变借用帧
    Mut {
        owner: VarId,
        r: VarId,
        region: RegionId,
        base_type: TypeKey,
    },
}

impl Frame {
    pub fn owner(&self) -> VarId {
        match self {
            Frame::Freeze { owner, .. } => *owner,
            Frame::Shr { owner, .. } => *owner,
            Frame::Mut { owner, .. } => *owner,
        }
    }

    pub fn reference(&self) -> Option<VarId> {
        match self {
            Frame::Freeze { .. } => None,
            Frame::Shr { r, .. } => Some(*r),
            Frame::Mut { r, .. } => Some(*r),
        }
    }
}

impl fmt::Display for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Frame::Freeze { owner, base_type } => {
                write!(f, "Freeze(v{}, {})", owner, base_type.short_name())
            }
            Frame::Shr { owner, r, region, base_type } => {
                write!(f, "Shr(v{} -> v{}@r{}, {})", owner, r, region, base_type.short_name())
            }
            Frame::Mut { owner, r, region, base_type } => {
                write!(f, "Mut(v{} -> v{}@r{}, {})", owner, r, region, base_type.short_name())
            }
        }
    }
}

/// Marking (place -> multiset of tokens)
pub type Marking = HashMap<PlaceId, Vec<Token>>;

/// 仿真状态
#[derive(Clone, Debug)]
pub struct SimState {
    /// 当前 marking
    pub marking: Marking,
    /// 借用栈 (Pushdown)
    pub stack: Vec<Frame>,
    /// 下一个可用的 VarId
    pub next_vid: VarId,
    /// 下一个可用的 RegionId
    pub next_region: RegionId,
}

impl SimState {
    /// 创建空状态
    pub fn new() -> Self {
        SimState {
            marking: HashMap::new(),
            stack: Vec::new(),
            next_vid: 0,
            next_region: 0,
        }
    }

    /// 分配新的 VarId
    pub fn alloc_vid(&mut self) -> VarId {
        let vid = self.next_vid;
        self.next_vid += 1;
        vid
    }

    /// 分配新的 RegionId
    pub fn alloc_region(&mut self) -> RegionId {
        let region = self.next_region;
        self.next_region += 1;
        region
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

    /// 检查 owner 是否有活跃的借用
    pub fn has_active_borrow(&self, owner_vid: VarId) -> bool {
        self.stack.iter().any(|f| f.owner() == owner_vid)
    }

    /// 检查 owner 是否有活跃的可变借用
    pub fn has_active_mut_borrow(&self, owner_vid: VarId) -> bool {
        self.stack.iter().any(|f| matches!(f, Frame::Mut { owner, .. } if *owner == owner_vid))
    }

    /// 检查 owner 是否有多个共享借用
    pub fn has_multiple_shr_borrows(&self, owner_vid: VarId) -> bool {
        let count = self.stack.iter().filter(|f| {
            matches!(f, Frame::Shr { owner, .. } if *owner == owner_vid)
        }).count();
        count > 1
    }

    /// 获取 owner 的 Freeze 帧
    pub fn get_freeze_frame(&self, owner_vid: VarId) -> Option<&Frame> {
        self.stack.iter().find(|f| {
            matches!(f, Frame::Freeze { owner, .. } if *owner == owner_vid)
        })
    }

    /// 获取状态的规范化 hash key (用于去重)
    pub fn hash_key(&self) -> String {
        let mut parts = Vec::new();

        // Marking 部分
        let mut places: Vec<_> = self.marking.iter().collect();
        places.sort_by_key(|(p, _)| *p);
        for (place, tokens) in places {
            if !tokens.is_empty() {
                let mut token_strs: Vec<_> = tokens.iter().map(|t| {
                    format!("{}:{}:{:?}", t.vid, t.bind_mut, t.region)
                }).collect();
                token_strs.sort();
                parts.push(format!("p{}:{}", place, token_strs.join(",")));
            }
        }

        // Stack 部分
        for frame in &self.stack {
            parts.push(format!("S:{:?}", frame));
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
    /// Transition 类型
    pub kind: FiringKind,
    /// 输入绑定: (place_id, token)
    pub input_bindings: Vec<(PlaceId, Token)>,
    /// 输出绑定: (place_id, token)
    pub output_bindings: Vec<(PlaceId, Token)>,
}

/// Firing 类型（用于代码生成）
#[derive(Clone, Debug)]
pub enum FiringKind {
    /// API 调用
    ApiCall { fn_path: String },
    /// 创建常量
    CreateConst { type_key: TypeKey },
    /// 首次共享借用
    BorrowShrFirst { base_type: TypeKey },
    /// 后续共享借用
    BorrowShrNext { base_type: TypeKey },
    /// 结束共享借用（保持冻结）
    EndShrKeepFrz { base_type: TypeKey },
    /// 结束共享借用（解冻）
    EndShrUnfreeze { base_type: TypeKey },
    /// 可变借用
    BorrowMut { base_type: TypeKey },
    /// 结束可变借用
    EndMut { base_type: TypeKey },
    /// 创建 mut 绑定（Move）
    MakeMutByMove { type_key: TypeKey },
    /// 创建 mut 绑定（Copy）
    MakeMutByCopy { type_key: TypeKey },
    /// Drop
    Drop { type_key: TypeKey },
    /// Copy 使用
    CopyUse { type_key: TypeKey },
}

impl fmt::Display for Firing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        if !self.input_bindings.is_empty() {
            let inputs: Vec<_> = self.input_bindings.iter().map(|(_, t)| format!("{}", t)).collect();
            write!(f, " [{}]", inputs.join(", "))?;
        }
        if !self.output_bindings.is_empty() {
            let outputs: Vec<_> = self.output_bindings.iter().map(|(_, t)| format!("{}", t)).collect();
            write!(f, " → [{}]", outputs.join(", "))?;
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
                for firing in self.compute_enabled_firings(&state, trans) {
                    if let Some((next_state, updated_firing)) = self.fire(&state, &firing) {
                        // 检查 bounds
                        if !self.check_bounds(&next_state) {
                            continue;
                        }

                        let hash = next_state.hash_key();
                        if !visited.contains(&hash) {
                            visited.insert(hash);
                            let mut next_trace = trace.clone();
                            next_trace.push(updated_firing);
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
                for firing in self.compute_enabled_firings(&state, trans) {
                    if let Some((next_state, updated_firing)) = self.fire(&state, &firing) {
                        // 检查 bounds
                        if !self.check_bounds(&next_state) {
                            continue;
                        }

                        let hash = next_state.hash_key();
                        if !visited.contains(&hash) {
                            visited.insert(hash);
                            let mut next_trace = trace.clone();
                            next_trace.push(updated_firing);
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

    /// 计算 transition 的所有 enabled firings
    fn compute_enabled_firings(&self, state: &SimState, trans: &Transition) -> Vec<Firing> {
        let mut firings = Vec::new();

        match &trans.kind {
            // Primitive 创建：无输入要求，始终 enabled
            TransitionKind::CreatePrimitive { type_key } => {
                firings.push(Firing {
                    transition_id: trans.id,
                    name: trans.name.clone(),
                    kind: FiringKind::CreateConst { type_key: type_key.clone() },
                    input_bindings: Vec::new(),
                    output_bindings: Vec::new(),
                });
            }

            // API 调用
            TransitionKind::ApiCall { .. } => {
                if let Some(bindings) = self.find_input_bindings(state, &trans.input_arcs) {
                    firings.push(Firing {
                        transition_id: trans.id,
                        name: trans.name.clone(),
                        kind: FiringKind::ApiCall { fn_path: trans.name.clone() },
                        input_bindings: bindings,
                        output_bindings: Vec::new(),
                    });
                }
            }

            // BorrowShrFirst: Own(T) → Frz(T) + Own(RefShr(T))
            TransitionKind::BorrowShrFirst { base_type } => {
                for arc in &trans.input_arcs {
                    for token in state.get_tokens(arc.place_id) {
                        // 检查是否可以借用（没有活跃的可变借用）
                        if token.is_owned() && !state.has_active_borrow(token.vid) {
                            firings.push(Firing {
                                transition_id: trans.id,
                                name: trans.name.clone(),
                                kind: FiringKind::BorrowShrFirst { base_type: base_type.clone() },
                                input_bindings: vec![(arc.place_id, token.clone())],
                                output_bindings: Vec::new(),
                            });
                        }
                    }
                }
            }

            // BorrowShrNext: Frz(T) → Frz(T) + Own(RefShr(T))
            TransitionKind::BorrowShrNext { base_type } => {
                for arc in &trans.input_arcs {
                    for token in state.get_tokens(arc.place_id) {
                        // 已经冻结，可以继续借用
                        if token.is_owned() {
                            firings.push(Firing {
                                transition_id: trans.id,
                                name: trans.name.clone(),
                                kind: FiringKind::BorrowShrNext { base_type: base_type.clone() },
                                input_bindings: vec![(arc.place_id, token.clone())],
                                output_bindings: Vec::new(),
                            });
                        }
                    }
                }
            }

            // EndShrKeepFrz: 还有其他共享借用
            TransitionKind::EndShrKeepFrz { base_type } => {
                // 需要同时有 Frz token 和 RefShr token
                if trans.input_arcs.len() >= 2 {
                    let frz_tokens = state.get_tokens(trans.input_arcs[0].place_id);
                    let ref_tokens = state.get_tokens(trans.input_arcs[1].place_id);

                    for frz_token in frz_tokens {
                        // 检查是否有多个共享借用
                        if state.has_multiple_shr_borrows(frz_token.vid) {
                            for ref_token in ref_tokens {
                                if ref_token.is_ref() {
                                    firings.push(Firing {
                                        transition_id: trans.id,
                                        name: trans.name.clone(),
                                        kind: FiringKind::EndShrKeepFrz { base_type: base_type.clone() },
                                        input_bindings: vec![
                                            (trans.input_arcs[0].place_id, frz_token.clone()),
                                            (trans.input_arcs[1].place_id, ref_token.clone()),
                                        ],
                                        output_bindings: Vec::new(),
                                    });
                                }
                            }
                        }
                    }
                }
            }

            // EndShrUnfreeze: 最后一个共享借用
            TransitionKind::EndShrUnfreeze { base_type } => {
                if trans.input_arcs.len() >= 2 {
                    let frz_tokens = state.get_tokens(trans.input_arcs[0].place_id);
                    let ref_tokens = state.get_tokens(trans.input_arcs[1].place_id);

                    for frz_token in frz_tokens {
                        // 检查是否是最后一个共享借用
                        if !state.has_multiple_shr_borrows(frz_token.vid) {
                            for ref_token in ref_tokens {
                                if ref_token.is_ref() {
                                    // 确保是这个 owner 的借用
                                    let matches = state.stack.iter().any(|f| {
                                        matches!(f, Frame::Shr { owner, r, .. } 
                                            if *owner == frz_token.vid && *r == ref_token.vid)
                                    });
                                    if matches {
                                        firings.push(Firing {
                                            transition_id: trans.id,
                                            name: trans.name.clone(),
                                            kind: FiringKind::EndShrUnfreeze { base_type: base_type.clone() },
                                            input_bindings: vec![
                                                (trans.input_arcs[0].place_id, frz_token.clone()),
                                                (trans.input_arcs[1].place_id, ref_token.clone()),
                                            ],
                                            output_bindings: Vec::new(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // BorrowMut: 需要 bind_mut = true
            TransitionKind::BorrowMut { base_type } => {
                for arc in &trans.input_arcs {
                    for token in state.get_tokens(arc.place_id) {
                        // 需要 bind_mut 且没有活跃借用
                        if token.is_owned() && token.bind_mut && !state.has_active_borrow(token.vid) {
                            firings.push(Firing {
                                transition_id: trans.id,
                                name: trans.name.clone(),
                                kind: FiringKind::BorrowMut { base_type: base_type.clone() },
                                input_bindings: vec![(arc.place_id, token.clone())],
                                output_bindings: Vec::new(),
                            });
                        }
                    }
                }
            }

            // EndMut: Blk(T) + Own(RefMut(T)) → Own(T)
            TransitionKind::EndMut { base_type } => {
                if trans.input_arcs.len() >= 2 {
                    let blk_tokens = state.get_tokens(trans.input_arcs[0].place_id);
                    let ref_tokens = state.get_tokens(trans.input_arcs[1].place_id);

                    for blk_token in blk_tokens {
                        for ref_token in ref_tokens {
                            if ref_token.is_ref() {
                                // 确保栈顶匹配
                                let matches = state.stack.last().map_or(false, |f| {
                                    matches!(f, Frame::Mut { owner, r, .. } 
                                        if *owner == blk_token.vid && *r == ref_token.vid)
                                });
                                if matches {
                                    firings.push(Firing {
                                        transition_id: trans.id,
                                        name: trans.name.clone(),
                                        kind: FiringKind::EndMut { base_type: base_type.clone() },
                                        input_bindings: vec![
                                            (trans.input_arcs[0].place_id, blk_token.clone()),
                                            (trans.input_arcs[1].place_id, ref_token.clone()),
                                        ],
                                        output_bindings: Vec::new(),
                                    });
                                }
                            }
                        }
                    }
                }
            }

            // MakeMutByMove: 把非 mut 变成 mut
            TransitionKind::MakeMutByMove { type_key } => {
                for arc in &trans.input_arcs {
                    for token in state.get_tokens(arc.place_id) {
                        if token.is_owned() && !token.bind_mut && !state.has_active_borrow(token.vid) {
                            firings.push(Firing {
                                transition_id: trans.id,
                                name: trans.name.clone(),
                                kind: FiringKind::MakeMutByMove { type_key: type_key.clone() },
                                input_bindings: vec![(arc.place_id, token.clone())],
                                output_bindings: Vec::new(),
                            });
                        }
                    }
                }
            }

            // MakeMutByCopy: Copy 类型创建 mut 副本
            TransitionKind::MakeMutByCopy { type_key } => {
                for arc in &trans.input_arcs {
                    for token in state.get_tokens(arc.place_id) {
                        if token.is_owned() {
                            firings.push(Firing {
                                transition_id: trans.id,
                                name: trans.name.clone(),
                                kind: FiringKind::MakeMutByCopy { type_key: type_key.clone() },
                                input_bindings: vec![(arc.place_id, token.clone())],
                                output_bindings: Vec::new(),
                            });
                        }
                    }
                }
            }

            // Drop: 需要没有活跃借用
            TransitionKind::Drop { type_key } => {
                for arc in &trans.input_arcs {
                    for token in state.get_tokens(arc.place_id) {
                        if token.is_owned() && !state.has_active_borrow(token.vid) {
                            firings.push(Firing {
                                transition_id: trans.id,
                                name: trans.name.clone(),
                                kind: FiringKind::Drop { type_key: type_key.clone() },
                                input_bindings: vec![(arc.place_id, token.clone())],
                                output_bindings: Vec::new(),
                            });
                        }
                    }
                }
            }

            // CopyUse: 复制 token
            TransitionKind::CopyUse { type_key } => {
                for arc in &trans.input_arcs {
                    for token in state.get_tokens(arc.place_id) {
                        if token.is_owned() {
                            firings.push(Firing {
                                transition_id: trans.id,
                                name: trans.name.clone(),
                                kind: FiringKind::CopyUse { type_key: type_key.clone() },
                                input_bindings: vec![(arc.place_id, token.clone())],
                                output_bindings: Vec::new(),
                            });
                        }
                    }
                }
            }

            _ => {}
        }

        firings
    }

    /// 查找输入 binding
    fn find_input_bindings(&self, state: &SimState, arcs: &[Arc]) -> Option<Vec<(PlaceId, Token)>> {
        let mut bindings = Vec::new();
        let mut used_vids = HashSet::new();

        for arc in arcs {
            let tokens = state.get_tokens(arc.place_id);
            let found = tokens.iter().find(|t| !used_vids.contains(&t.vid));
            if let Some(token) = found {
                bindings.push((arc.place_id, token.clone()));
                if arc.consumes {
                    used_vids.insert(token.vid);
                }
            } else {
                return None;
            }
        }

        Some(bindings)
    }

    /// 执行 firing，返回 (新状态, 更新后的 firing)
    fn fire(&self, state: &SimState, firing: &Firing) -> Option<(SimState, Firing)> {
        let mut new_state = state.clone();
        let mut updated_firing = firing.clone();
        let trans = &self.pcpn.transitions[firing.transition_id];

        match &firing.kind {
            FiringKind::CreateConst { .. } => {
                let vid = new_state.alloc_vid();
                for arc in &trans.output_arcs {
                    let token = Token::owned(vid, false);
                    new_state.add_token(arc.place_id, token.clone());
                    updated_firing.output_bindings.push((arc.place_id, token));
                }
            }

            FiringKind::ApiCall { .. } => {
                // 消耗输入 tokens
                for (i, (place_id, token)) in firing.input_bindings.iter().enumerate() {
                    if let Some(arc) = trans.input_arcs.get(i) {
                        if arc.consumes {
                            new_state.remove_token(*place_id, token.vid);
                        }
                    }
                }
                // 生成输出 tokens
                // 直接产生 bind_mut=true 的 Token，避免后续需要 MakeMutByMove
                for arc in &trans.output_arcs {
                    let vid = new_state.alloc_vid();
                    // 检查输出是否是引用类型
                    let place = &self.pcpn.places[arc.place_id];
                    let token = if place.is_ref {
                        let region = new_state.alloc_region();
                        Token::reference(vid, region)
                    } else {
                        // 直接产生 mut 绑定，这样可以直接进行 &mut 借用
                        Token::owned(vid, true)
                    };
                    new_state.add_token(arc.place_id, token.clone());
                    updated_firing.output_bindings.push((arc.place_id, token));
                }
            }

            FiringKind::BorrowShrFirst { base_type } => {
                let (_, owner_token) = &firing.input_bindings[0];
                let owner_vid = owner_token.vid;
                let owner_bind_mut = owner_token.bind_mut;

                // 移除 Own token
                new_state.remove_token(trans.input_arcs[0].place_id, owner_vid);

                // 添加 Frz token
                let frz_token = Token::owned(owner_vid, owner_bind_mut);
                new_state.add_token(trans.output_arcs[0].place_id, frz_token.clone());
                updated_firing.output_bindings.push((trans.output_arcs[0].place_id, frz_token));

                // 创建 RefShr token
                let ref_vid = new_state.alloc_vid();
                let region = new_state.alloc_region();
                let ref_token = Token::reference(ref_vid, region);
                new_state.add_token(trans.output_arcs[1].place_id, ref_token.clone());
                updated_firing.output_bindings.push((trans.output_arcs[1].place_id, ref_token));

                // Push stack frames
                new_state.stack.push(Frame::Freeze {
                    owner: owner_vid,
                    base_type: base_type.clone(),
                });
                new_state.stack.push(Frame::Shr {
                    owner: owner_vid,
                    r: ref_vid,
                    region,
                    base_type: base_type.clone(),
                });
            }

            FiringKind::BorrowShrNext { base_type } => {
                let (_, frz_token) = &firing.input_bindings[0];
                let owner_vid = frz_token.vid;

                // Frz token 保持不变（读取弧）
                
                // 创建新的 RefShr token
                let ref_vid = new_state.alloc_vid();
                let region = new_state.alloc_region();
                let ref_token = Token::reference(ref_vid, region);
                new_state.add_token(trans.output_arcs[0].place_id, ref_token.clone());
                updated_firing.output_bindings.push((trans.output_arcs[0].place_id, ref_token));

                // Push Shr frame
                new_state.stack.push(Frame::Shr {
                    owner: owner_vid,
                    r: ref_vid,
                    region,
                    base_type: base_type.clone(),
                });
            }

            FiringKind::EndShrKeepFrz { .. } => {
                // 移除 RefShr token
                let (_, ref_token) = &firing.input_bindings[1];
                new_state.remove_token(trans.input_arcs[1].place_id, ref_token.vid);

                // 从栈中移除对应的 Shr frame
                if let Some(pos) = new_state.stack.iter().position(|f| {
                    matches!(f, Frame::Shr { r, .. } if *r == ref_token.vid)
                }) {
                    new_state.stack.remove(pos);
                }
            }

            FiringKind::EndShrUnfreeze { .. } => {
                let (_, frz_token) = &firing.input_bindings[0];
                let (_, ref_token) = &firing.input_bindings[1];
                let owner_vid = frz_token.vid;
                let owner_bind_mut = frz_token.bind_mut;

                // 移除 Frz token 和 RefShr token
                new_state.remove_token(trans.input_arcs[0].place_id, owner_vid);
                new_state.remove_token(trans.input_arcs[1].place_id, ref_token.vid);

                // 恢复 Own token
                let own_token = Token::owned(owner_vid, owner_bind_mut);
                new_state.add_token(trans.output_arcs[0].place_id, own_token.clone());
                updated_firing.output_bindings.push((trans.output_arcs[0].place_id, own_token));

                // 移除 Shr frame 和 Freeze frame
                new_state.stack.retain(|f| {
                    !matches!(f, Frame::Shr { r, .. } if *r == ref_token.vid) &&
                    !matches!(f, Frame::Freeze { owner, .. } if *owner == owner_vid)
                });
            }

            FiringKind::BorrowMut { base_type } => {
                let (_, owner_token) = &firing.input_bindings[0];
                let owner_vid = owner_token.vid;
                let owner_bind_mut = owner_token.bind_mut;

                // 移除 Own token
                new_state.remove_token(trans.input_arcs[0].place_id, owner_vid);

                // 添加 Blk token
                let blk_token = Token::owned(owner_vid, owner_bind_mut);
                new_state.add_token(trans.output_arcs[0].place_id, blk_token.clone());
                updated_firing.output_bindings.push((trans.output_arcs[0].place_id, blk_token));

                // 创建 RefMut token
                let ref_vid = new_state.alloc_vid();
                let region = new_state.alloc_region();
                let ref_token = Token::reference(ref_vid, region);
                new_state.add_token(trans.output_arcs[1].place_id, ref_token.clone());
                updated_firing.output_bindings.push((trans.output_arcs[1].place_id, ref_token));

                // Push Mut frame
                new_state.stack.push(Frame::Mut {
                    owner: owner_vid,
                    r: ref_vid,
                    region,
                    base_type: base_type.clone(),
                });
            }

            FiringKind::EndMut { .. } => {
                let (_, blk_token) = &firing.input_bindings[0];
                let (_, ref_token) = &firing.input_bindings[1];
                let owner_vid = blk_token.vid;
                let owner_bind_mut = blk_token.bind_mut;

                // 移除 Blk token 和 RefMut token
                new_state.remove_token(trans.input_arcs[0].place_id, owner_vid);
                new_state.remove_token(trans.input_arcs[1].place_id, ref_token.vid);

                // 恢复 Own token
                let own_token = Token::owned(owner_vid, owner_bind_mut);
                new_state.add_token(trans.output_arcs[0].place_id, own_token.clone());
                updated_firing.output_bindings.push((trans.output_arcs[0].place_id, own_token));

                // Pop Mut frame
                new_state.stack.pop();
            }

            FiringKind::MakeMutByMove { .. } => {
                let (place_id, token) = &firing.input_bindings[0];
                
                // 移除旧 token
                new_state.remove_token(*place_id, token.vid);
                
                // 添加新的 mut token（使用同一个 vid）
                let mut_token = Token::owned(token.vid, true);
                new_state.add_token(trans.output_arcs[0].place_id, mut_token.clone());
                updated_firing.output_bindings.push((trans.output_arcs[0].place_id, mut_token));
            }

            FiringKind::MakeMutByCopy { .. } => {
                // 原 token 保持不变，创建新的 mut token
                let new_vid = new_state.alloc_vid();
                let mut_token = Token::owned(new_vid, true);
                new_state.add_token(trans.output_arcs[0].place_id, mut_token.clone());
                updated_firing.output_bindings.push((trans.output_arcs[0].place_id, mut_token));
            }

            FiringKind::Drop { .. } => {
                let (place_id, token) = &firing.input_bindings[0];
                new_state.remove_token(*place_id, token.vid);
            }

            FiringKind::CopyUse { .. } => {
                // 原 token 保持不变，创建副本
                let new_vid = new_state.alloc_vid();
                let bind_mut = firing.input_bindings[0].1.bind_mut;
                let copy_token = Token::owned(new_vid, bind_mut);
                new_state.add_token(trans.output_arcs[0].place_id, copy_token.clone());
                updated_firing.output_bindings.push((trans.output_arcs[0].place_id, copy_token));
            }
        }

        Some((new_state, updated_firing))
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

        // 默认目标：栈为空（无未结束的借用），且有 owned token
        if !state.stack.is_empty() {
            return false;
        }

        // 检查是否有 owned token（非引用）
        let has_owned = state.marking.values().any(|tokens| {
            tokens.iter().any(|t| t.is_owned())
        });

        has_owned
    }
}

/// 打印 trace
pub fn print_trace(trace: &[Firing]) {
    println!("=== Firing Sequence ({} steps) ===", trace.len());
    for (i, firing) in trace.iter().enumerate() {
        println!("  {}. {}", i + 1, firing);
    }
}

/// 打印最终状态
pub fn print_final_state(state: &SimState, pcpn: &Pcpn) {
    println!("\n=== Final State ===");
    
    // Marking
    let mut places: Vec<_> = state.marking.iter()
        .filter(|(_, tokens)| !tokens.is_empty())
        .collect();
    places.sort_by_key(|(p, _)| *p);
    
    for (place_id, tokens) in places {
        let place_name = pcpn.places.get(*place_id)
            .map(|p| p.display_name())
            .unwrap_or_else(|| format!("p{}", place_id));
        let token_strs: Vec<_> = tokens.iter().map(|t| format!("{}", t)).collect();
        println!("  {}: [{}]", place_name, token_strs.join(", "));
    }
    
    // Stack
    if !state.stack.is_empty() {
        println!("\n  Stack (bottom → top):");
        for (i, frame) in state.stack.iter().enumerate() {
            println!("    {}: {}", i, frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_display() {
        let t1 = Token::owned(0, false);
        assert_eq!(format!("{}", t1), "v0");

        let t2 = Token::owned(1, true);
        assert_eq!(format!("{}", t2), "mut v1");

        let t3 = Token::reference(2, 0);
        assert_eq!(format!("{}", t3), "v2@r0");
    }

    #[test]
    fn test_state_hash() {
        let mut s1 = SimState::new();
        s1.add_token(0, Token::owned(0, false));
        s1.add_token(1, Token::owned(1, true));

        let mut s2 = SimState::new();
        s2.add_token(1, Token::owned(1, true));
        s2.add_token(0, Token::owned(0, false));

        // 不同顺序添加应该得到相同的 hash
        assert_eq!(s1.hash_key(), s2.hash_key());
    }
}
