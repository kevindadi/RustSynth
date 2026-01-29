//! PCPN 仿真器 - 支持 9-Place 模型和 Canonicalization

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;

use crate::config::{ParsedGoal, TaskConfig};
use crate::pcpn::{Guard, GuardKind, Pcpn, Transition, TransitionKind};
use crate::types::{
    BorrowStack, CanonFrame, CanonToken, Capability, Marking, PlaceId, RegionLabel, StackFrame,
    Token, TypeForm, VarId,
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
            let c: Vec<_> = self.consumed.iter().map(|(_, t)| format!("v{}", t.vid)).collect();
            write!(f, " [-{}]", c.join(","))?;
        }
        if !self.produced.is_empty() {
            let p: Vec<_> = self.produced.iter().map(|(_, t)| format!("v{}", t.vid)).collect();
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
                CanonFrame::from(f)
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
                let token_strs: Vec<String> = tokens.iter().map(|t| format!("v{}", t.vid)).collect();
                parts.push(format!("p{}:[{}]", pid, token_strs.join(",")));
            }
        }
        let stack_str: Vec<String> = self
            .stack
            .iter()
            .map(|f| format!("{:?}", f.kind))
            .collect();
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
        }
    }

    pub fn get_bound(&self, place_id: PlaceId) -> usize {
        *self.place_bounds.get(&place_id).unwrap_or(&self.default_bound)
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
        self.search_bfs()
    }

    fn search_bfs(&self) -> SimResult {
        let initial = self.initial_state();
        let mut queue: VecDeque<(SimState, Vec<TraceFiring>)> = VecDeque::new();
        let mut visited: HashSet<String> = HashSet::new();

        let canon = initial.canonicalize();
        visited.insert(canon.hash_key());
        queue.push_back((initial, Vec::new()));

        let mut states_explored = 0;

        while let Some((state, trace)) = queue.pop_front() {
            states_explored += 1;

            if trace.len() >= self.config.max_steps {
                continue;
            }

            if self.check_goal(&state) {
                return SimResult {
                    found: true,
                    trace,
                    states_explored,
                    final_state: Some(state),
                };
            }

            for trans in &self.pcpn.transitions {
                if !self.config.is_transition_allowed(&trans.name) {
                    continue;
                }
                if let Some(bindings) = self.enabled(trans, &state) {
                    if let Some((next_state, firing)) = self.fire(trans, &state, &bindings) {
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

        SimResult {
            found: false,
            trace: Vec::new(),
            states_explored,
            final_state: None,
        }
    }

    fn initial_state(&self) -> SimState {
        SimState::new()
    }

    fn enabled(&self, trans: &Transition, state: &SimState) -> Option<Vec<(PlaceId, Token)>> {
        let mut bindings = Vec::new();
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
                    bindings.push((arc.place_id, token));
                } else {
                    return None;
                }
            } else {
                if state.marking.count(arc.place_id) == 0 {
                    return None;
                }
            }
        }

        for guard in &trans.guards {
            if !self.check_guard(guard, state, &bindings) {
                return None;
            }
        }

        Some(bindings)
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
        }
    }

    fn fire(
        &self,
        trans: &Transition,
        state: &SimState,
        bindings: &[(PlaceId, Token)],
    ) -> Option<(SimState, TraceFiring)> {
        let mut new_state = state.clone();
        let mut consumed: Vec<(PlaceId, Token)> = Vec::new();
        let mut produced: Vec<(PlaceId, Token)> = Vec::new();

        for (place_id, token) in bindings {
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
                if let Some((_, ref_token)) = consumed.iter().find(|(_, t)| t.is_ref()) {
                    new_state.stack.pop();
                }
            }

            TransitionKind::EndBorrowShrUnfreeze { base_type } => {
                if let Some((_, ref_token)) = consumed.iter().find(|(_, t)| t.is_ref()) {
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

            TransitionKind::ApiCall { fn_path, .. } => {
                for arc in &trans.output_arcs {
                    if arc.annotation.as_ref().map(|a| matches!(a, crate::pcpn::ArcAnnotation::ReturnArc)).unwrap_or(false) {
                        if let Some((_, orig)) = consumed.first() {
                            let copy_token = Token::new_owned(orig.vid, orig.ty.clone());
                            new_state.marking.add(arc.place_id, copy_token.clone());
                            produced.push((arc.place_id, copy_token));
                        }
                    } else if arc.annotation.as_ref().map(|a| matches!(a, crate::pcpn::ArcAnnotation::Return)).unwrap_or(false) {
                        let place = &self.pcpn.places[arc.place_id];
                        let vid = new_state.fresh_vid();
                        let token = match place.form {
                            TypeForm::Value => Token::new_owned(vid, place.base_type.clone()),
                            TypeForm::RefShr => {
                                let region = new_state.fresh_region();
                                let owner = consumed.first().map(|(_, t)| t.vid).unwrap_or(0);
                                new_state.stack.push(StackFrame::Shr {
                                    owner_vid: owner,
                                    ref_vid: vid,
                                    region,
                                });
                                Token::new_ref_shr(vid, place.base_type.clone(), region, owner)
                            }
                            TypeForm::RefMut => {
                                let region = new_state.fresh_region();
                                let owner = consumed.first().map(|(_, t)| t.vid).unwrap_or(0);
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
                if let Some(bindings) = self.enabled(trans, &state) {
                    if let Some((next_state, _)) = self.fire(trans, &state, &bindings) {
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
}
