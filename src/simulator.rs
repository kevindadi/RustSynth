//! PCPN 仿真器 - 支持 9-Place 模型和 Canonicalization

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;

use crate::config::{ParsedGoal, TaskConfig};
use crate::pcpn::{Guard, GuardKind, Pcpn, Transition, TransitionKind};
use crate::types::{
    BorrowStack, CanonFrame, CanonFrameKind, CanonToken, Capability, Marking, PlaceId, RegionLabel,
    StackFrame, Token, TypeForm, VarId,
};

#[derive(Clone, Debug)]
pub struct TraceFiring {
    pub name: String,
    pub kind: TransitionKind,
    pub consumed: Vec<(PlaceId, Token)>,
    pub produced: Vec<(PlaceId, Token)>,
}

impl fmt::Display for TraceFiring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        if !self.consumed.is_empty() {
            let c: Vec<_> = self
                .consumed
                .iter()
                .map(|(_, t)| format!("v{}", t.vid))
                .collect();
            write!(f, " [-{}]", c.join(","))?;
        }
        if !self.produced.is_empty() {
            let p: Vec<_> = self
                .produced
                .iter()
                .map(|(_, t)| format!("v{}", t.vid))
                .collect();
            write!(f, " [+{}]", p.join(","))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct SimState {
    pub marking: Marking,
    pub stack: BorrowStack,
    pub next_vid: VarId,
    pub next_region: RegionLabel,
}

impl SimState {
    pub fn new() -> Self {
        SimState {
            marking: Marking::new(),
            stack: BorrowStack::new(),
            next_vid: 0,
            next_region: 0,
        }
    }

    pub fn fresh_vid(&mut self) -> VarId {
        let vid = self.next_vid;
        self.next_vid += 1;
        vid
    }

    pub fn fresh_region(&mut self) -> RegionLabel {
        let r = self.next_region;
        self.next_region += 1;
        r
    }

    pub fn canonicalize(&self) -> CanonState {
        let mut vid_map: HashMap<VarId, VarId> = HashMap::new();
        let mut region_map: HashMap<RegionLabel, RegionLabel> = HashMap::new();
        let mut next_vid: VarId = 0;
        let mut next_region: RegionLabel = 0;

        let mut canon_marking: BTreeMap<PlaceId, Vec<CanonToken>> = BTreeMap::new();
        let mut place_ids: Vec<_> = self.marking.tokens.keys().copied().collect();
        place_ids.sort();

        for pid in place_ids {
            if let Some(tokens) = self.marking.tokens.get(&pid) {
                let mut canon_tokens: Vec<CanonToken> = Vec::new();
                for token in tokens {
                    let canon_vid = *vid_map.entry(token.vid).or_insert_with(|| {
                        let v = next_vid;
                        next_vid += 1;
                        v
                    });
                    let canon_borrowed_from = token.borrowed_from.map(|bv| {
                        *vid_map.entry(bv).or_insert_with(|| {
                            let v = next_vid;
                            next_vid += 1;
                            v
                        })
                    });
                    let canon_regions: smallvec::SmallVec<[RegionLabel; 2]> = token
                        .regions
                        .iter()
                        .map(|&r| {
                            *region_map.entry(r).or_insert_with(|| {
                                let lr = next_region;
                                next_region += 1;
                                lr
                            })
                        })
                        .collect();

                    canon_tokens.push(CanonToken {
                        vid: canon_vid,
                        ty: token.ty.clone(),
                        form: token.form.clone(),
                        regions: canon_regions,
                        borrowed_from: canon_borrowed_from,
                    });
                }
                canon_tokens.sort_by_key(|t| t.vid);
                canon_marking.insert(pid, canon_tokens);
            }
        }

        let canon_stack: Vec<CanonFrame> = self
            .stack
            .frames
            .iter()
            .map(|f| {
                let owner = *vid_map.entry(f.owner_vid()).or_insert_with(|| {
                    let v = next_vid;
                    next_vid += 1;
                    v
                });
                let ref_v = f.ref_vid().map(|rv| {
                    *vid_map.entry(rv).or_insert_with(|| {
                        let v = next_vid;
                        next_vid += 1;
                        v
                    })
                });
                let reg = f.region().map(|r| {
                    *region_map.entry(r).or_insert_with(|| {
                        let lr = next_region;
                        next_region += 1;
                        lr
                    })
                });
                // 使用重映射后的值构造 CanonFrame,而非原始值
                let kind = match f {
                    StackFrame::Freeze { .. } => CanonFrameKind::Freeze,
                    StackFrame::Shr { .. } => CanonFrameKind::Shr,
                    StackFrame::Mut { .. } => CanonFrameKind::Mut,
                };
                CanonFrame {
                    kind,
                    owner_vid: owner,
                    ref_vid: ref_v,
                    region: reg,
                }
            })
            .collect();

        CanonState {
            marking: canon_marking,
            stack: canon_stack,
        }
    }
}

impl Default for SimState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CanonState {
    pub marking: BTreeMap<PlaceId, Vec<CanonToken>>,
    pub stack: Vec<CanonFrame>,
}

impl CanonState {
    pub fn hash_key(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for (pid, tokens) in &self.marking {
            if !tokens.is_empty() {
                let token_strs: Vec<String> =
                    tokens.iter().map(|t| format!("v{}", t.vid)).collect();
                parts.push(format!("p{}:[{}]", pid, token_strs.join(",")));
            }
        }
        let stack_str: Vec<String> = self.stack.iter().map(|f| format!("{:?}", f.kind)).collect();
        format!("M:{};S:{}", parts.join("|"), stack_str.join("|"))
    }
}

#[derive(Clone, Debug)]
pub struct SimConfig {
    pub max_steps: usize,
    pub stack_depth: usize,
    pub place_bounds: HashMap<PlaceId, usize>,
    pub default_bound: usize,
    pub goal: Option<ParsedGoal>,
    pub allow_transitions: Vec<String>,
    pub deny_transitions: Vec<String>,
    /// Search strategy: "bfs" (default), "dfs", or "iddfs" (iterative deepening)
    pub strategy: String,
    /// Maximum number of distinct witness traces to collect (for multi-trace mode)
    pub max_traces: usize,
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            max_steps: 100,
            stack_depth: 8,
            place_bounds: HashMap::new(),
            default_bound: 2,
            goal: None,
            allow_transitions: Vec::new(),
            deny_transitions: Vec::new(),
            strategy: "bfs".to_string(),
            max_traces: 1,
        }
    }
}

impl SimConfig {
    pub fn from_task_config(task: &TaskConfig, pcpn: &Pcpn) -> Self {
        let mut place_bounds = HashMap::new();
        for (key_str, &bound) in &task.search.place_bounds {
            for place in &pcpn.places {
                if place.key().display_name() == *key_str {
                    place_bounds.insert(place.id, bound);
                }
            }
        }

        let goal = ParsedGoal::parse(&task.goal).ok();

        SimConfig {
            max_steps: task.search.max_steps,
            stack_depth: task.search.stack_depth,
            place_bounds,
            default_bound: task.search.default_place_bound,
            goal,
            allow_transitions: task.filter.allow.clone(),
            deny_transitions: task.filter.deny.clone(),
            strategy: task.search.strategy.clone(),
            max_traces: task.search.max_traces,
        }
    }

    pub fn get_bound(&self, place_id: PlaceId) -> usize {
        *self
            .place_bounds
            .get(&place_id)
            .unwrap_or(&self.default_bound)
    }

    pub fn is_transition_allowed(&self, name: &str) -> bool {
        for deny in &self.deny_transitions {
            if name.contains(deny) || deny.contains(name) {
                return false;
            }
        }

        if self.allow_transitions.is_empty() {
            return true;
        }

        for allow in &self.allow_transitions {
            if name.contains(allow) || allow.contains(name) {
                return true;
            }
        }

        false
    }
}

#[derive(Clone, Debug)]
pub struct SimResult {
    pub found: bool,
    pub trace: Vec<TraceFiring>,
    pub states_explored: usize,
    pub final_state: Option<SimState>,
    /// Additional witness traces collected in multi-trace mode
    pub extra_traces: Vec<Vec<TraceFiring>>,
}

pub struct Simulator<'a> {
    pcpn: &'a Pcpn,
    config: SimConfig,
}

impl<'a> Simulator<'a> {
    pub fn new(pcpn: &'a Pcpn, config: SimConfig) -> Self {
        Simulator { pcpn, config }
    }

    pub fn run(&self) -> SimResult {
        match self.config.strategy.as_str() {
            "dfs" => self.search_dfs(),
            "iddfs" => self.search_iddfs(),
            _ => self.search_bfs(),
        }
    }

    fn search_bfs(&self) -> SimResult {
        let initial = self.initial_state();
        let mut queue: VecDeque<(SimState, Vec<TraceFiring>)> = VecDeque::new();
        let mut visited: HashSet<String> = HashSet::new();

        let canon = initial.canonicalize();
        visited.insert(canon.hash_key());
        queue.push_back((initial, Vec::new()));

        let mut states_explored = 0;
        let mut collected_traces: Vec<Vec<TraceFiring>> = Vec::new();
        let mut first_state: Option<SimState> = None;

        while let Some((state, trace)) = queue.pop_front() {
            states_explored += 1;

            if trace.len() >= self.config.max_steps {
                continue;
            }

            if self.check_goal(&state) {
                if collected_traces.is_empty() {
                    first_state = Some(state.clone());
                }
                collected_traces.push(trace.clone());
                if collected_traces.len() >= self.config.max_traces {
                    let first = collected_traces.remove(0);
                    return SimResult {
                        found: true,
                        trace: first,
                        states_explored,
                        final_state: first_state,
                        extra_traces: collected_traces,
                    };
                }
                continue;
            }

            for trans in &self.pcpn.transitions {
                if !self.config.is_transition_allowed(&trans.name) {
                    continue;
                }
                if let Some((consume_bindings, read_bindings)) = self.enabled(trans, &state) {
                    if let Some((next_state, firing)) =
                        self.fire(trans, &state, &consume_bindings, &read_bindings)
                    {
                        if !self.check_bounds(&next_state) {
                            continue;
                        }
                        if next_state.stack.len() > self.config.stack_depth {
                            continue;
                        }

                        let canon = next_state.canonicalize();
                        let hash = canon.hash_key();
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

        if !collected_traces.is_empty() {
            let first = collected_traces.remove(0);
            return SimResult {
                found: true,
                trace: first,
                states_explored,
                final_state: first_state,
                extra_traces: collected_traces,
            };
        }

        SimResult {
            found: false,
            trace: Vec::new(),
            states_explored,
            final_state: None,
            extra_traces: Vec::new(),
        }
    }

    fn search_dfs(&self) -> SimResult {
        let initial = self.initial_state();
        let mut stack: Vec<(SimState, Vec<TraceFiring>)> = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();

        let canon = initial.canonicalize();
        visited.insert(canon.hash_key());
        stack.push((initial, Vec::new()));

        let mut states_explored = 0;
        let mut collected_traces: Vec<Vec<TraceFiring>> = Vec::new();
        let mut first_state: Option<SimState> = None;

        while let Some((state, trace)) = stack.pop() {
            states_explored += 1;

            if trace.len() >= self.config.max_steps {
                continue;
            }

            if self.check_goal(&state) {
                if collected_traces.is_empty() {
                    first_state = Some(state.clone());
                }
                collected_traces.push(trace.clone());
                if collected_traces.len() >= self.config.max_traces {
                    let first = collected_traces.remove(0);
                    return SimResult {
                        found: true,
                        trace: first,
                        states_explored,
                        final_state: first_state,
                        extra_traces: collected_traces,
                    };
                }
                continue;
            }

            for trans in &self.pcpn.transitions {
                if !self.config.is_transition_allowed(&trans.name) {
                    continue;
                }
                if let Some((consume_bindings, read_bindings)) = self.enabled(trans, &state) {
                    if let Some((next_state, firing)) =
                        self.fire(trans, &state, &consume_bindings, &read_bindings)
                    {
                        if !self.check_bounds(&next_state) {
                            continue;
                        }
                        if next_state.stack.len() > self.config.stack_depth {
                            continue;
                        }

                        let canon = next_state.canonicalize();
                        let hash = canon.hash_key();
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

        if !collected_traces.is_empty() {
            let first = collected_traces.remove(0);
            return SimResult {
                found: true,
                trace: first,
                states_explored,
                final_state: first_state,
                extra_traces: collected_traces,
            };
        }

        SimResult {
            found: false,
            trace: Vec::new(),
            states_explored,
            final_state: None,
            extra_traces: Vec::new(),
        }
    }

    fn search_iddfs(&self) -> SimResult {
        let mut total_explored = 0;

        for depth_limit in 1..=self.config.max_steps {
            let initial = self.initial_state();
            let mut stack: Vec<(SimState, Vec<TraceFiring>)> = Vec::new();
            let mut visited: HashSet<String> = HashSet::new();

            let canon = initial.canonicalize();
            visited.insert(canon.hash_key());
            stack.push((initial, Vec::new()));

            let mut found_at_depth = false;
            let mut collected_traces: Vec<Vec<TraceFiring>> = Vec::new();
            let mut first_state: Option<SimState> = None;

            while let Some((state, trace)) = stack.pop() {
                total_explored += 1;

                if trace.len() >= depth_limit {
                    continue;
                }

                if self.check_goal(&state) {
                    if collected_traces.is_empty() {
                        first_state = Some(state.clone());
                    }
                    collected_traces.push(trace.clone());
                    found_at_depth = true;
                    if collected_traces.len() >= self.config.max_traces {
                        break;
                    }
                    continue;
                }

                for trans in &self.pcpn.transitions {
                    if !self.config.is_transition_allowed(&trans.name) {
                        continue;
                    }
                    if let Some((consume_bindings, read_bindings)) = self.enabled(trans, &state) {
                        if let Some((next_state, firing)) =
                            self.fire(trans, &state, &consume_bindings, &read_bindings)
                        {
                            if !self.check_bounds(&next_state) {
                                continue;
                            }
                            if next_state.stack.len() > self.config.stack_depth {
                                continue;
                            }

                            let canon = next_state.canonicalize();
                            let hash = canon.hash_key();
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

            if found_at_depth {
                let first = collected_traces.remove(0);
                return SimResult {
                    found: true,
                    trace: first,
                    states_explored: total_explored,
                    final_state: first_state,
                    extra_traces: collected_traces,
                };
            }
        }

        SimResult {
            found: false,
            trace: Vec::new(),
            states_explored: total_explored,
            final_state: None,
            extra_traces: Vec::new(),
        }
    }

    fn initial_state(&self) -> SimState {
        SimState::new()
    }

    /// 使能检查的结果:(消耗绑定, 读取绑定)
    /// 消耗绑定 = 被 consume 的 token
    /// 读取绑定 = 被 read(非 consume)的 token,用于确定借用来源
    pub fn enabled(
        &self,
        trans: &Transition,
        state: &SimState,
    ) -> Option<(Vec<(PlaceId, Token)>, Vec<(PlaceId, Token)>)> {
        let mut consume_bindings = Vec::new();
        let mut read_bindings = Vec::new();
        let mut used_vids: HashSet<VarId> = HashSet::new();

        for arc in &trans.input_arcs {
            if arc.consumes {
                if let Some(tokens) = state.marking.get(arc.place_id) {
                    let available: Vec<_> = tokens
                        .iter()
                        .filter(|t| !used_vids.contains(&t.vid))
                        .collect();
                    if available.is_empty() {
                        return None;
                    }
                    let token = available[0].clone();
                    used_vids.insert(token.vid);
                    consume_bindings.push((arc.place_id, token));
                } else {
                    return None;
                }
            } else {
                // 非消耗弧:检查存在性,并记录读取绑定
                if let Some(tokens) = state.marking.get(arc.place_id) {
                    let available: Vec<_> = tokens
                        .iter()
                        .filter(|t| !used_vids.contains(&t.vid))
                        .collect();
                    if available.is_empty() {
                        return None;
                    }
                    // 记录读取绑定(不消耗,但提供上下文信息)
                    read_bindings.push((arc.place_id, available[0].clone()));
                } else {
                    return None;
                }
            }
        }

        // Guard 检查时使用所有绑定
        let all_bindings: Vec<_> = consume_bindings
            .iter()
            .chain(read_bindings.iter())
            .cloned()
            .collect();
        for guard in &trans.guards {
            if !self.check_guard(guard, state, &all_bindings) {
                return None;
            }
        }

        Some((consume_bindings, read_bindings))
    }

    fn check_guard(&self, guard: &Guard, state: &SimState, bindings: &[(PlaceId, Token)]) -> bool {
        let base = &guard.base_type;

        match guard.kind {
            GuardKind::NoFrzNoBlk => {
                let frz_place = self.pcpn.get_place(base, &TypeForm::Value, Capability::Frz);
                let blk_place = self.pcpn.get_place(base, &TypeForm::Value, Capability::Blk);
                let frz_count = frz_place.map(|p| state.marking.count(p)).unwrap_or(0);
                let blk_count = blk_place.map(|p| state.marking.count(p)).unwrap_or(0);
                frz_count == 0 && blk_count == 0
            }
            GuardKind::NoBlk => {
                let blk_place = self.pcpn.get_place(base, &TypeForm::Value, Capability::Blk);
                let blk_count = blk_place.map(|p| state.marking.count(p)).unwrap_or(0);
                blk_count == 0
            }
            GuardKind::NoFrzNoOtherBlk => {
                let frz_place = self.pcpn.get_place(base, &TypeForm::Value, Capability::Frz);
                let blk_place = self.pcpn.get_place(base, &TypeForm::Value, Capability::Blk);
                let frz_count = frz_place.map(|p| state.marking.count(p)).unwrap_or(0);
                let blk_count = blk_place.map(|p| state.marking.count(p)).unwrap_or(0);
                frz_count == 0 && blk_count <= 1
            }
            GuardKind::NotBlocked => {
                if let Some((_, token)) = bindings.first() {
                    !state.stack.is_blocked(token.vid)
                } else {
                    true
                }
            }
            GuardKind::StackTopMatches => {
                if let Some((_, token)) = bindings.iter().find(|(_, t)| t.is_ref()) {
                    if let Some(top) = state.stack.top() {
                        top.ref_vid() == Some(token.vid)
                    } else {
                        false
                    }
                } else {
                    true
                }
            }
            GuardKind::PlaceCountRange {
                ref form,
                cap,
                min,
                max,
            } => {
                let place = self.pcpn.get_place(base, form, cap);
                let count = place.map(|p| state.marking.count(p)).unwrap_or(0);
                count >= min && count <= max
            }
            GuardKind::StackDepthMax { max_depth } => state.stack.len() <= max_depth,
            GuardKind::And(ref sub_guards) => sub_guards.iter().all(|sub_kind| {
                let sub_guard = Guard {
                    kind: sub_kind.clone(),
                    base_type: base.clone(),
                };
                self.check_guard(&sub_guard, state, bindings)
            }),
        }
    }

    pub fn fire(
        &self,
        trans: &Transition,
        state: &SimState,
        consume_bindings: &[(PlaceId, Token)],
        read_bindings: &[(PlaceId, Token)],
    ) -> Option<(SimState, TraceFiring)> {
        let mut new_state = state.clone();
        let mut consumed: Vec<(PlaceId, Token)> = Vec::new();
        let mut produced: Vec<(PlaceId, Token)> = Vec::new();

        for (place_id, token) in consume_bindings {
            new_state.marking.remove_by_vid(*place_id, token.vid)?;
            consumed.push((*place_id, token.clone()));
        }

        match &trans.kind {
            TransitionKind::CreatePrimitive { ty } | TransitionKind::ConstProducer { ty, .. } => {
                let vid = new_state.fresh_vid();
                let token = Token::new_owned(vid, ty.clone());
                if let Some(out_arc) = trans.output_arcs.first() {
                    new_state.marking.add(out_arc.place_id, token.clone());
                    produced.push((out_arc.place_id, token));
                }
            }

            TransitionKind::BorrowShrFirst { base_type } => {
                if let Some((_, owner_token)) = consumed.first() {
                    let region = new_state.fresh_region();
                    let ref_vid = new_state.fresh_vid();
                    let frz_token = Token::new_owned(owner_token.vid, base_type.clone());
                    let ref_token =
                        Token::new_ref_shr(ref_vid, base_type.clone(), region, owner_token.vid);

                    new_state.stack.push(StackFrame::Freeze {
                        owner_vid: owner_token.vid,
                    });
                    new_state.stack.push(StackFrame::Shr {
                        owner_vid: owner_token.vid,
                        ref_vid,
                        region,
                    });

                    if trans.output_arcs.len() >= 2 {
                        let frz_place = trans.output_arcs[0].place_id;
                        let ref_place = trans.output_arcs[1].place_id;
                        new_state.marking.add(frz_place, frz_token.clone());
                        new_state.marking.add(ref_place, ref_token.clone());
                        produced.push((frz_place, frz_token));
                        produced.push((ref_place, ref_token));
                    }
                }
            }

            TransitionKind::BorrowShrNext { base_type } => {
                if let Some(frz_tokens) = state.marking.get(trans.input_arcs[0].place_id) {
                    if let Some(frz_token) = frz_tokens.first() {
                        let region = new_state.fresh_region();
                        let ref_vid = new_state.fresh_vid();
                        let ref_token =
                            Token::new_ref_shr(ref_vid, base_type.clone(), region, frz_token.vid);

                        new_state.stack.push(StackFrame::Shr {
                            owner_vid: frz_token.vid,
                            ref_vid,
                            region,
                        });

                        if let Some(out_arc) = trans.output_arcs.first() {
                            new_state.marking.add(out_arc.place_id, ref_token.clone());
                            produced.push((out_arc.place_id, ref_token));
                        }
                    }
                }
            }

            TransitionKind::EndBorrowShrKeepFrz { .. } => {
                if consumed.iter().any(|(_, t)| t.is_ref()) {
                    new_state.stack.pop();
                }
            }

            TransitionKind::EndBorrowShrUnfreeze { base_type } => {
                if consumed.iter().any(|(_, t)| t.is_ref()) {
                    new_state.stack.pop();
                    new_state.stack.pop();

                    if let Some((_, frz_token)) = consumed.iter().find(|(_, t)| !t.is_ref()) {
                        let restored = Token::new_owned(frz_token.vid, base_type.clone());
                        if let Some(out_arc) = trans.output_arcs.first() {
                            new_state.marking.add(out_arc.place_id, restored.clone());
                            produced.push((out_arc.place_id, restored));
                        }
                    }
                }
            }

            TransitionKind::BorrowMut { base_type } => {
                if let Some((_, owner_token)) = consumed.first() {
                    let region = new_state.fresh_region();
                    let ref_vid = new_state.fresh_vid();
                    let blk_token = Token::new_owned(owner_token.vid, base_type.clone());
                    let ref_token =
                        Token::new_ref_mut(ref_vid, base_type.clone(), region, owner_token.vid);

                    new_state.stack.push(StackFrame::Mut {
                        owner_vid: owner_token.vid,
                        ref_vid,
                        region,
                    });

                    if trans.output_arcs.len() >= 2 {
                        let blk_place = trans.output_arcs[0].place_id;
                        let ref_place = trans.output_arcs[1].place_id;
                        new_state.marking.add(blk_place, blk_token.clone());
                        new_state.marking.add(ref_place, ref_token.clone());
                        produced.push((blk_place, blk_token));
                        produced.push((ref_place, ref_token));
                    }
                }
            }

            TransitionKind::EndBorrowMut { base_type } => {
                new_state.stack.pop();
                if let Some((_, blk_token)) = consumed.iter().find(|(_, t)| !t.is_ref()) {
                    let restored = Token::new_owned(blk_token.vid, base_type.clone());
                    if let Some(out_arc) = trans.output_arcs.first() {
                        new_state.marking.add(out_arc.place_id, restored.clone());
                        produced.push((out_arc.place_id, restored));
                    }
                }
            }

            TransitionKind::Drop { .. } => {}

            TransitionKind::CopyUse { ty } => {
                let vid = new_state.fresh_vid();
                let new_token = Token::new_owned(vid, ty.clone());
                if let Some(out_arc) = trans.output_arcs.first() {
                    new_state.marking.add(out_arc.place_id, new_token.clone());
                    produced.push((out_arc.place_id, new_token));
                }
            }

            TransitionKind::ApiCall { .. } => {
                for arc in &trans.output_arcs {
                    if arc
                        .annotation
                        .as_ref()
                        .map(|a| matches!(a, crate::pcpn::ArcAnnotation::ReturnArc))
                        .unwrap_or(false)
                    {
                        // Copy-return: 返回被消耗 token 的副本(Copy 类型)
                        if let Some((_, orig)) = consumed.first() {
                            let copy_token = Token::new_owned(orig.vid, orig.ty.clone());
                            new_state.marking.add(arc.place_id, copy_token.clone());
                            produced.push((arc.place_id, copy_token));
                        }
                    } else if arc
                        .annotation
                        .as_ref()
                        .map(|a| matches!(a, crate::pcpn::ArcAnnotation::Return))
                        .unwrap_or(false)
                    {
                        let place = &self.pcpn.places[arc.place_id];
                        let vid = new_state.fresh_vid();
                        let token = match place.form {
                            TypeForm::Value => Token::new_owned(vid, place.base_type.clone()),
                            TypeForm::RefShr => {
                                let region = new_state.fresh_region();
                                // 使用生命周期绑定确定借用来源(包含 read_bindings)
                                let owner = self.resolve_borrow_owner(
                                    trans,
                                    &consumed,
                                    read_bindings,
                                    true,
                                );
                                // 仅在 owner 尚未被 freeze 时添加 Freeze 帧
                                if !new_state.stack.has_freeze_for_owner(owner) {
                                    new_state
                                        .stack
                                        .push(StackFrame::Freeze { owner_vid: owner });
                                }
                                new_state.stack.push(StackFrame::Shr {
                                    owner_vid: owner,
                                    ref_vid: vid,
                                    region,
                                });
                                Token::new_ref_shr(vid, place.base_type.clone(), region, owner)
                            }
                            TypeForm::RefMut => {
                                let region = new_state.fresh_region();
                                // 使用生命周期绑定确定借用来源(包含 read_bindings)
                                let owner = self.resolve_borrow_owner(
                                    trans,
                                    &consumed,
                                    read_bindings,
                                    false,
                                );
                                new_state.stack.push(StackFrame::Mut {
                                    owner_vid: owner,
                                    ref_vid: vid,
                                    region,
                                });
                                Token::new_ref_mut(vid, place.base_type.clone(), region, owner)
                            }
                        };
                        new_state.marking.add(arc.place_id, token.clone());
                        produced.push((arc.place_id, token));
                    }
                }
            }
        }

        let firing = TraceFiring {
            name: trans.name.clone(),
            kind: trans.kind.clone(),
            consumed,
            produced,
        };

        Some((new_state, firing))
    }

    /// 根据生命周期绑定信息确定 API 调用返回引用的借用来源
    /// all_input_tokens 包含消耗和读取的 token(按 input_arcs 顺序排列)
    fn resolve_borrow_owner(
        &self,
        trans: &Transition,
        consumed: &[(PlaceId, Token)],
        read_bindings: &[(PlaceId, Token)],
        is_shared: bool,
    ) -> VarId {
        // 构建完整的 input token 列表(消耗 + 读取,按 arc 顺序)
        let mut all_inputs: Vec<Option<&Token>> = Vec::new();
        let mut consume_idx = 0;
        let mut read_idx = 0;
        for arc in &trans.input_arcs {
            if arc.consumes {
                if consume_idx < consumed.len() {
                    all_inputs.push(Some(&consumed[consume_idx].1));
                    consume_idx += 1;
                } else {
                    all_inputs.push(None);
                }
            } else {
                if read_idx < read_bindings.len() {
                    all_inputs.push(Some(&read_bindings[read_idx].1));
                    read_idx += 1;
                } else {
                    all_inputs.push(None);
                }
            }
        }

        // 优先使用 transition 上的 lifetime_bindings
        for lb in &trans.lifetime_bindings {
            if lb.is_shared == is_shared || trans.lifetime_bindings.len() == 1 {
                if let Some(Some(token)) = all_inputs.get(lb.source_arc_index) {
                    // 如果读取的是引用 token,追溯到其 borrowed_from(原始 owner)
                    if let Some(owner_vid) = token.borrowed_from {
                        return owner_vid;
                    }
                    return token.vid;
                }
            }
        }

        // 回退:优先使用读取绑定中的 token(通常是 &self 的来源)
        if let Some((_, token)) = read_bindings.first() {
            if let Some(owner_vid) = token.borrowed_from {
                return owner_vid;
            }
            return token.vid;
        }
        // 最后回退到消耗的第一个 token
        consumed.first().map(|(_, t)| t.vid).unwrap_or(0)
    }

    fn check_bounds(&self, state: &SimState) -> bool {
        for place in &self.pcpn.places {
            let count = state.marking.count(place.id);
            let bound = self.config.get_bound(place.id).max(place.budget);
            if count > bound {
                return false;
            }
        }
        true
    }

    fn check_goal(&self, state: &SimState) -> bool {
        if let Some(ref goal) = self.config.goal {
            let place_id = self.pcpn.get_place(&goal.base_type, &goal.form, goal.cap);
            if let Some(pid) = place_id {
                return state.marking.count(pid) >= goal.count;
            }
            false
        } else {
            let non_empty = state
                .marking
                .tokens
                .iter()
                .filter(|(_, ts)| !ts.is_empty())
                .count();
            non_empty > 0 && state.stack.is_empty()
        }
    }

    pub fn generate_reachability_graph(&self, max_states: usize) -> ReachabilityGraph {
        let initial = self.initial_state();
        let mut states: Vec<SimState> = Vec::new();
        let mut state_ids: HashMap<String, usize> = HashMap::new();
        let mut edges: Vec<(usize, usize, String)> = Vec::new();
        let mut queue: VecDeque<SimState> = VecDeque::new();

        let canon = initial.canonicalize();
        let hash = canon.hash_key();
        state_ids.insert(hash, 0);
        states.push(initial.clone());
        queue.push_back(initial);

        while let Some(state) = queue.pop_front() {
            if states.len() >= max_states {
                break;
            }

            let from_id = *state_ids.get(&state.canonicalize().hash_key()).unwrap();

            for trans in &self.pcpn.transitions {
                if !self.config.is_transition_allowed(&trans.name) {
                    continue;
                }
                if let Some((consume_bindings, read_bindings)) = self.enabled(trans, &state) {
                    if let Some((next_state, _)) =
                        self.fire(trans, &state, &consume_bindings, &read_bindings)
                    {
                        if !self.check_bounds(&next_state) {
                            continue;
                        }
                        if next_state.stack.len() > self.config.stack_depth {
                            continue;
                        }

                        let canon = next_state.canonicalize();
                        let hash = canon.hash_key();
                        let to_id = if let Some(&id) = state_ids.get(&hash) {
                            id
                        } else {
                            let id = states.len();
                            state_ids.insert(hash, id);
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

pub struct ReachabilityGraph {
    pub states: Vec<SimState>,
    pub edges: Vec<(usize, usize, String)>,
}

impl ReachabilityGraph {
    pub fn to_dot(&self, pcpn: &Pcpn) -> String {
        let mut dot = String::new();
        dot.push_str("digraph Reachability {\n");
        dot.push_str("  rankdir=TB;\n");
        dot.push_str("  node [shape=box, style=filled, fillcolor=lightyellow];\n\n");

        for (i, state) in self.states.iter().enumerate() {
            let label = self.state_label(state, pcpn);
            let color = if i == 0 { "lightgreen" } else { "lightyellow" };
            dot.push_str(&format!(
                "  s{} [label=\"s{}\\n{}\", fillcolor={}];\n",
                i, i, label, color
            ));
        }
        dot.push_str("\n");

        for (from, to, label) in &self.edges {
            let short_label = if label.len() > 20 {
                format!("{}...", &label[..17])
            } else {
                label.clone()
            };
            dot.push_str(&format!(
                "  s{} -> s{} [label=\"{}\"];\n",
                from, to, short_label
            ));
        }

        dot.push_str("}\n");
        dot
    }

    fn state_label(&self, state: &SimState, pcpn: &Pcpn) -> String {
        let mut parts = Vec::new();
        let mut pids: Vec<_> = state.marking.tokens.keys().copied().collect();
        pids.sort();

        for pid in pids.iter().take(6) {
            if let Some(tokens) = state.marking.tokens.get(pid) {
                if !tokens.is_empty() {
                    let place = &pcpn.places[*pid];
                    let name = place.key().display_name();
                    parts.push(format!("{}:{}", name, tokens.len()));
                }
            }
        }

        if parts.is_empty() {
            "empty".to_string()
        } else {
            parts.join("\\n")
        }
    }

    pub fn stats(&self) -> String {
        format!("States: {}, Edges: {}", self.states.len(), self.edges.len())
    }
}

pub fn print_trace(trace: &[TraceFiring]) {
    println!("=== Trace ({} steps) ===", trace.len());
    for (i, f) in trace.iter().enumerate() {
        println!("  {}. {}", i + 1, f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TyGround;

    #[test]
    fn test_canon_state_hash() {
        let mut state = SimState::new();
        let t1 = Token::new_owned(0, TyGround::path("Counter"));
        let t2 = Token::new_owned(1, TyGround::path("Counter"));
        state.marking.add(0, t1);
        state.marking.add(0, t2);

        let canon = state.canonicalize();
        let hash = canon.hash_key();
        assert!(!hash.is_empty());
    }

    #[test]
    fn test_borrow_stack_operations() {
        let mut stack = BorrowStack::new();
        stack.push(StackFrame::Freeze { owner_vid: 0 });
        stack.push(StackFrame::Shr {
            owner_vid: 0,
            ref_vid: 1,
            region: 0,
        });

        assert!(stack.is_blocked(0));
        assert_eq!(stack.len(), 2);

        stack.pop();
        assert_eq!(stack.len(), 1);
    }

    // ===================================================================
    //  Counter PCPN 可达性搜索测试
    // ===================================================================

    use crate::apigraph::build_counter_api_graph;
    use crate::config::{GoalConfig, ParsedGoal};
    use crate::pcpn::{Pcpn, TransitionKind};

    /// 辅助：构建 Counter PCPN
    fn counter_pcpn() -> Pcpn {
        let graph = build_counter_api_graph();
        Pcpn::from_api_graph(&graph)
    }

    /// 辅助：从 trace 中提取 API 调用名称（过滤掉结构转换）
    fn api_call_names(trace: &[TraceFiring]) -> Vec<String> {
        trace.iter()
            .filter(|f| matches!(f.kind,
                TransitionKind::ApiCall { .. } | TransitionKind::ConstProducer { .. }))
            .map(|f| f.name.clone())
            .collect()
    }

    #[test]
    fn test_counter_reach_own_counter() {
        // Goal: own Counter — 最短路径应为 Counter::new() (1 步)
        let pcpn = counter_pcpn();
        let goal = ParsedGoal::parse(&GoalConfig {
            want: "own Counter".to_string(),
            count: 1,
        }).unwrap();

        let config = SimConfig {
            max_steps: 50,
            stack_depth: 4,
            goal: Some(goal),
            ..Default::default()
        };

        let sim = Simulator::new(&pcpn, config);
        let result = sim.run();

        assert!(result.found, "Should find own Counter");
        // 最短 trace 应该只有 Counter::new()
        let api_names = api_call_names(&result.trace);
        assert_eq!(api_names.len(), 1, "Shortest path is 1 API call");
        assert_eq!(api_names[0], "Counter::new");
    }

    #[test]
    fn test_counter_reach_own_i32() {
        // Goal: own i32 — 最短路径是 const_i32 (1 步，但它是 CreatePrimitive 不是 API)
        let pcpn = counter_pcpn();
        let goal = ParsedGoal::parse(&GoalConfig {
            want: "own i32".to_string(),
            count: 1,
        }).unwrap();

        let config = SimConfig {
            max_steps: 50,
            stack_depth: 4,
            goal: Some(goal),
            ..Default::default()
        };

        let sim = Simulator::new(&pcpn, config);
        let result = sim.run();

        assert!(result.found, "Should find own i32");
        assert!(!result.trace.is_empty(), "Trace should not be empty");
        // trace 第一步可能是 const_i32 (CreatePrimitive) 或 Counter::new
        // 只需验证找到了合法路径
    }

    #[test]
    fn test_counter_reach_own_i32_via_counter_only() {
        // 只允许 Counter 相关的 transition (禁止 const_i32)
        // Goal: own i32 → 必须通过 Counter::new() → Counter::into_value()
        let pcpn = counter_pcpn();
        let goal = ParsedGoal::parse(&GoalConfig {
            want: "own i32".to_string(),
            count: 1,
        }).unwrap();

        let config = SimConfig {
            max_steps: 50,
            stack_depth: 4,
            goal: Some(goal),
            deny_transitions: vec!["const_i32".to_string(), "copy_use".to_string()],
            ..Default::default()
        };

        let sim = Simulator::new(&pcpn, config);
        let result = sim.run();

        assert!(result.found, "Should find own i32 via Counter path");
        let api_names = api_call_names(&result.trace);
        // 应包含 Counter::new 和 Counter::into_value
        assert!(api_names.contains(&"Counter::new".to_string()),
            "Trace should include Counter::new, got {:?}", api_names);
        assert!(api_names.contains(&"Counter::into_value".to_string()),
            "Trace should include Counter::into_value, got {:?}", api_names);
    }

    #[test]
    fn test_simulator_borrow_shr_cycle() {
        // 验证 new → borrow_shr_first → end_shr_unfreeze 恢复原始 token
        let pcpn = counter_pcpn();
        let counter_ty = TyGround::path("Counter");

        // 手动执行: 初始化 → new → borrow_shr → end_shr_unfreeze
        let mut state = SimState::new();
        let vid = state.fresh_vid();
        let own_place = pcpn.get_place(&counter_ty, &TypeForm::Value, Capability::Own).unwrap();
        state.marking.add(own_place, Token::new_owned(vid, counter_ty.clone()));

        // 验证初始状态
        assert_eq!(state.marking.count(own_place), 1);
        assert!(state.stack.is_empty());

        // 手动执行 borrow_shr_first
        let bsf = pcpn.transitions.iter()
            .find(|t| t.name == "borrow_shr_first(Counter)").unwrap();

        let config = SimConfig::default();
        let sim = Simulator::new(&pcpn, config);

        if let Some((consume, read)) = sim.enabled(bsf, &state) {
            let (state2, _firing) = sim.fire(bsf, &state, &consume, &read).unwrap();

            let frz_place = pcpn.get_place(&counter_ty, &TypeForm::Value, Capability::Frz).unwrap();
            let shr_place = pcpn.get_place(&counter_ty, &TypeForm::RefShr, Capability::Own).unwrap();

            // 应该: own_Counter 空, frz_Counter 有 1 token, own_&Counter 有 1 token
            assert_eq!(state2.marking.count(own_place), 0, "own consumed");
            assert_eq!(state2.marking.count(frz_place), 1, "frz created");
            assert_eq!(state2.marking.count(shr_place), 1, "ref created");
            assert_eq!(state2.stack.len(), 2, "Freeze + Shr frames");

            // 手动执行 end_shr_unfreeze
            let esu = pcpn.transitions.iter()
                .find(|t| t.name == "end_shr_unfreeze(Counter)").unwrap();

            if let Some((consume2, read2)) = sim.enabled(esu, &state2) {
                let (state3, _firing2) = sim.fire(esu, &state2, &consume2, &read2).unwrap();

                // 应该恢复: own_Counter 有 1 token, frz/shr 都空, stack 空
                assert_eq!(state3.marking.count(own_place), 1, "own restored");
                assert_eq!(state3.marking.count(frz_place), 0, "frz consumed");
                assert_eq!(state3.marking.count(shr_place), 0, "ref consumed");
                assert!(state3.stack.is_empty(), "stack empty after unfreeze");
            } else {
                panic!("end_shr_unfreeze should be enabled");
            }
        } else {
            panic!("borrow_shr_first should be enabled");
        }
    }

    #[test]
    fn test_simulator_borrow_mut_cycle() {
        // 验证 new → borrow_mut → end_mut 恢复原始 token
        let pcpn = counter_pcpn();
        let counter_ty = TyGround::path("Counter");

        let mut state = SimState::new();
        let vid = state.fresh_vid();
        let own_place = pcpn.get_place(&counter_ty, &TypeForm::Value, Capability::Own).unwrap();
        state.marking.add(own_place, Token::new_owned(vid, counter_ty.clone()));

        let bm = pcpn.transitions.iter()
            .find(|t| t.name == "borrow_mut(Counter)").unwrap();

        let config = SimConfig::default();
        let sim = Simulator::new(&pcpn, config);

        if let Some((consume, read)) = sim.enabled(bm, &state) {
            let (state2, _) = sim.fire(bm, &state, &consume, &read).unwrap();

            let blk_place = pcpn.get_place(&counter_ty, &TypeForm::Value, Capability::Blk).unwrap();
            let mut_place = pcpn.get_place(&counter_ty, &TypeForm::RefMut, Capability::Own).unwrap();

            assert_eq!(state2.marking.count(own_place), 0, "own consumed");
            assert_eq!(state2.marking.count(blk_place), 1, "blk created");
            assert_eq!(state2.marking.count(mut_place), 1, "mut ref created");
            assert_eq!(state2.stack.len(), 1, "Mut frame pushed");

            // end_mut
            let em = pcpn.transitions.iter()
                .find(|t| t.name == "end_mut(Counter)").unwrap();

            if let Some((consume2, read2)) = sim.enabled(em, &state2) {
                let (state3, _) = sim.fire(em, &state2, &consume2, &read2).unwrap();

                assert_eq!(state3.marking.count(own_place), 1, "own restored");
                assert_eq!(state3.marking.count(blk_place), 0, "blk consumed");
                assert_eq!(state3.marking.count(mut_place), 0, "mut ref consumed");
                assert!(state3.stack.is_empty(), "stack empty after end_mut");
            } else {
                panic!("end_mut should be enabled");
            }
        } else {
            panic!("borrow_mut should be enabled");
        }
    }

    #[test]
    fn test_counter_reachability_graph() {
        // 生成 Counter PCPN 的可达图并验证基本属性
        let pcpn = counter_pcpn();
        let config = SimConfig {
            max_steps: 200,
            stack_depth: 3,
            default_bound: 2,
            ..Default::default()
        };

        let sim = Simulator::new(&pcpn, config);
        let rg = sim.generate_reachability_graph(100);

        // 应该至少有初始状态 + const_i32 / Counter::new 产生的状态
        assert!(rg.states.len() >= 2, "At least 2 states, got {}", rg.states.len());
        assert!(rg.edges.len() >= 2, "At least 2 edges, got {}", rg.edges.len());

        // 初始状态（index 0）应该有从 const_i32 和 Counter::new 出发的边
        let from_initial: Vec<_> = rg.edges.iter()
            .filter(|(from, _, _)| *from == 0)
            .collect();
        assert!(!from_initial.is_empty(), "Initial state should have outgoing edges");

        // 验证存在 Counter::new 边
        assert!(from_initial.iter().any(|(_, _, name)| name == "Counter::new"),
            "Should have Counter::new edge from initial state");
    }

    #[test]
    fn test_canonicalization_identity() {
        // 两个语义相同但 vid 不同的 state 应该产生相同的 canonical hash
        let counter_ty = TyGround::path("Counter");

        let mut state1 = SimState::new();
        state1.marking.add(0, Token::new_owned(5, counter_ty.clone()));
        state1.marking.add(0, Token::new_owned(10, counter_ty.clone()));

        let mut state2 = SimState::new();
        state2.marking.add(0, Token::new_owned(100, counter_ty.clone()));
        state2.marking.add(0, Token::new_owned(200, counter_ty.clone()));

        let hash1 = state1.canonicalize().hash_key();
        let hash2 = state2.canonicalize().hash_key();
        assert_eq!(hash1, hash2, "Same structure should have same canonical hash");
    }

    #[test]
    fn test_empty_goal_matches_any_nonempty() {
        // 无 goal 时，任何有 token 且 stack 为空的状态都算目标
        let pcpn = counter_pcpn();
        let config = SimConfig {
            max_steps: 10,
            stack_depth: 4,
            goal: None,
            ..Default::default()
        };

        let sim = Simulator::new(&pcpn, config);
        let result = sim.run();

        assert!(result.found, "With no explicit goal, any non-empty state with empty stack is a goal");
        assert!(!result.trace.is_empty());
    }
}
