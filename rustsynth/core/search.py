"""
Bounded reachability search over the PCPN state space.

Paper mapping:
  - bounded_reachability  ≈  BFS worklist saturation with bound enforcement
  - fire_transition       ≈  transition firing (token consumption/production + stack ops)
  - check_guards          ≈  guarded enabling predicate evaluation
  - close_stack           ≈  CloseStack — append EndBorrow/Drop to clear stack
  - witness extraction    ≈  back-trace from goal state to initial state

Configuration: Cfg = <M, S> where M = Marking, S = BorrowStack
"""

from __future__ import annotations

import logging
import time
from collections import deque
from dataclasses import dataclass, field
from typing import Optional

from rustsynth.core.types import (
    GroundType, TypeForm, Capability, Token, Marking, BorrowStack,
    StackFrame, StackFrameKind, VarId, RegionLabel, PlaceKey,
)
from rustsynth.core.pcpn import (
    PCPN, Transition, TransitionKind, Arc, Guard, GuardKind,
    StackAction, StackActionKind,
)
from rustsynth.core.canon import Canonicalizer, CanonState

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Search configuration
# ---------------------------------------------------------------------------

@dataclass
class SearchConfig:
    max_trace_len: int = 6
    stack_depth: int = 4
    token_per_place: int = 3
    use_stack: bool = True
    use_capability: bool = True
    use_obligations: bool = True
    type_only: bool = False
    max_witnesses: int = 5
    seed: int = 42


# ---------------------------------------------------------------------------
# Search state
# ---------------------------------------------------------------------------

@dataclass
class SimState:
    marking: Marking
    stack: BorrowStack
    next_vid: VarId = 0
    next_region: RegionLabel = 0

    def fresh_vid(self) -> VarId:
        v = self.next_vid
        self.next_vid += 1
        return v

    def fresh_region(self) -> RegionLabel:
        r = self.next_region
        self.next_region += 1
        return r

    def clone(self) -> SimState:
        return SimState(
            marking=self.marking.clone(),
            stack=self.stack.clone(),
            next_vid=self.next_vid,
            next_region=self.next_region,
        )


@dataclass
class TraceFiring:
    name: str
    kind: TransitionKind
    consumed: list[tuple[int, Token]]
    produced: list[tuple[int, Token]]
    fn_path: Optional[str] = None
    field_name: Optional[str] = None
    base_type: Optional[GroundType] = None

    def label(self) -> str:
        return self.fn_path or self.name


# ---------------------------------------------------------------------------
# Search result
# ---------------------------------------------------------------------------

@dataclass
class SearchResult:
    witnesses: list[list[TraceFiring]] = field(default_factory=list)
    states_explored: int = 0
    search_time_ms: float = 0.0
    goal_reached: bool = False


# ---------------------------------------------------------------------------
# Guard evaluation
# ---------------------------------------------------------------------------

def check_guards(guards: list[Guard], state: SimState, pcpn: PCPN, config: SearchConfig) -> bool:
    for guard in guards:
        if not _eval_guard(guard, state, pcpn, config):
            return False
    return True


def _eval_guard(guard: Guard, state: SimState, pcpn: PCPN, config: SearchConfig) -> bool:
    if guard.kind == GuardKind.NoFrzNoBlk:
        if not config.use_capability:
            return True
        bt = guard.base_type
        if bt is None:
            return True
        p_frz = pcpn.get_place(bt, TypeForm.Value, Capability.Frz)
        p_blk = pcpn.get_place(bt, TypeForm.Value, Capability.Blk)
        if p_frz is not None and state.marking.count(p_frz) > 0:
            return False
        if p_blk is not None and state.marking.count(p_blk) > 0:
            return False
        return True

    if guard.kind == GuardKind.NoBlk:
        if not config.use_capability:
            return True
        bt = guard.base_type
        if bt is None:
            return True
        p_blk = pcpn.get_place(bt, TypeForm.Value, Capability.Blk)
        if p_blk is not None and state.marking.count(p_blk) > 0:
            return False
        return True

    if guard.kind == GuardKind.StackTopMatches:
        if not config.use_stack:
            return True
        top = state.stack.peek()
        if top is None:
            return False
        return top.base_type == guard.base_type

    if guard.kind == GuardKind.PlaceCountRange:
        bt = guard.base_type
        form = guard.form
        cap = guard.cap
        if bt is None or form is None or cap is None:
            return True
        pid = pcpn.get_place(bt, form, cap)
        if pid is None:
            return True
        count = state.marking.count(pid)
        return guard.min_count <= count <= guard.max_count

    if guard.kind == GuardKind.StackDepthMax:
        if not config.use_stack:
            return True
        return state.stack.depth() <= guard.max_depth

    if guard.kind == GuardKind.And:
        return all(_eval_guard(sg, state, pcpn, config) for sg in guard.sub_guards)

    return True


# ---------------------------------------------------------------------------
# Transition firing
# ---------------------------------------------------------------------------

def try_fire(
    transition: Transition,
    state: SimState,
    pcpn: PCPN,
    config: SearchConfig,
) -> Optional[tuple[SimState, TraceFiring]]:
    """Attempt to fire a transition. Returns new state + firing record, or None."""

    if not check_guards(transition.guards, state, pcpn, config):
        return None

    if config.use_stack and state.stack.depth() >= config.stack_depth:
        if transition.stack_action.kind == StackActionKind.Push:
            return None

    consumed: list[tuple[int, Token, int]] = []  # (place_id, token, arc_index)
    used_vids_per_place: dict[int, set[int]] = {}
    for arc_idx, arc in enumerate(transition.input_arcs):
        tokens = state.marking.get_tokens(arc.place_id)
        if not tokens:
            return None
        used = used_vids_per_place.get(arc.place_id, set())
        selected = None
        for t in tokens:
            if t.vid not in used:
                selected = t
                break
        if selected is None:
            if not arc.consumes:
                selected = tokens[0]
            else:
                return None
        used.add(selected.vid)
        used_vids_per_place[arc.place_id] = used
        consumed.append((arc.place_id, selected, arc_idx))

    if config.token_per_place > 0:
        for arc in transition.output_arcs:
            if state.marking.count(arc.place_id) >= config.token_per_place:
                if not any(c[0] == arc.place_id for c in consumed):
                    return None

    new_state = state.clone()

    actual_consumed = []
    for place_id, token, arc_idx in consumed:
        arc = transition.input_arcs[arc_idx]
        if arc.consumes:
            removed = new_state.marking.remove(place_id, token.vid)
            if removed:
                actual_consumed.append((place_id, removed))
            else:
                actual_consumed.append((place_id, token))
        else:
            actual_consumed.append((place_id, token))

    produced: list[tuple[int, Token]] = []
    for arc in transition.output_arcs:
        place = pcpn.places[arc.place_id]
        new_vid = new_state.fresh_vid()
        new_token = Token(
            vid=new_vid,
            ty=place.base_type,
            form=place.form,
            regions=[],
        )

        if transition.kind in (TransitionKind.BorrowShrFirst, TransitionKind.BorrowShrNext):
            if place.form == TypeForm.RefShr:
                if actual_consumed:
                    new_token.borrowed_from = actual_consumed[0][1].vid
                new_token.regions = [new_state.fresh_region()]

        if transition.kind == TransitionKind.BorrowMut:
            if place.form == TypeForm.RefMut:
                if actual_consumed:
                    new_token.borrowed_from = actual_consumed[0][1].vid
                new_token.regions = [new_state.fresh_region()]

        if transition.kind in (TransitionKind.EndBorrowShrUnfreeze, TransitionKind.EndBorrowMut):
            if place.form == TypeForm.Value and place.cap == Capability.Own:
                for _, ct in actual_consumed:
                    if ct.borrowed_from is not None:
                        new_token.vid = ct.borrowed_from
                        break

        if place.cap == Capability.Frz and transition.kind == TransitionKind.BorrowShrFirst:
            if actual_consumed:
                new_token.vid = actual_consumed[0][1].vid

        if place.cap == Capability.Blk and transition.kind == TransitionKind.BorrowMut:
            if actual_consumed:
                new_token.vid = actual_consumed[0][1].vid

        new_state.marking.add(arc.place_id, new_token)
        produced.append((arc.place_id, new_token))

    if config.use_stack:
        sa = transition.stack_action
        if sa.kind == StackActionKind.Push:
            owner_vid = actual_consumed[0][1].vid if actual_consumed else 0
            ref_vid = None
            for pid, tok in produced:
                p = pcpn.places[pid]
                if p.form in (TypeForm.RefShr, TypeForm.RefMut) and p.cap == Capability.Own:
                    ref_vid = tok.vid
                    break
            region = produced[0][1].regions[0] if produced and produced[0][1].regions else None
            new_state.stack.push(StackFrame(
                kind=sa.frame_kind or StackFrameKind.Freeze,
                owner_vid=owner_vid,
                ref_vid=ref_vid,
                region=region,
                base_type=sa.base_type,
            ))
        elif sa.kind == StackActionKind.Pop:
            if new_state.stack.frames:
                top = new_state.stack.peek()
                if top and (not sa.base_type or top.base_type == sa.base_type):
                    if not sa.frame_kind or top.kind == sa.frame_kind:
                        new_state.stack.pop()

    firing = TraceFiring(
        name=transition.name,
        kind=transition.kind,
        consumed=actual_consumed,
        produced=produced,
        fn_path=transition.fn_path,
        field_name=transition.field_name,
        base_type=transition.base_type,
    )

    return new_state, firing


def _find_input_arc(transition: Transition, place_id: int) -> Optional[Arc]:
    for arc in transition.input_arcs:
        if arc.place_id == place_id:
            return arc
    return None


# ---------------------------------------------------------------------------
# Goal checking
# ---------------------------------------------------------------------------

def check_goal(state: SimState, pcpn: PCPN, goal: dict) -> bool:
    """Check if the goal condition is met."""
    if "form" in goal and "type" in goal:
        form_str = goal["form"]
        type_str = goal["type"]
        count = goal.get("count", 1)
    elif "want" in goal:
        want_str = goal["want"]
        parts = want_str.strip().split(None, 1)
        if len(parts) == 2:
            form_str, type_str = parts
            count = goal.get("count", 1)
        else:
            return False
    elif "goal_type" in goal:
        want = goal["goal_type"]
        form_str = want.get("form", "own")
        type_str = want.get("type", "")
        count = want.get("count", 1)
    else:
        return False

    form_map = {"own": TypeForm.Value, "ref": TypeForm.RefShr, "mut": TypeForm.RefMut}
    form = form_map.get(form_str, TypeForm.Value)

    target_ty = _parse_goal_type(type_str, pcpn)
    if target_ty is None:
        return False

    pid = pcpn.get_place(target_ty, form, Capability.Own)
    if pid is None:
        return False

    return state.marking.count(pid) >= count


def _parse_goal_type(type_str: str, pcpn: PCPN) -> Optional[GroundType]:
    for ty in pcpn.type_universe:
        if ty.short_name() == type_str or ty.full_name() == type_str or ty.name == type_str:
            return ty
    if type_str in ("i32", "u32", "i64", "u64", "bool", "usize", "f32", "f64"):
        return GroundType.primitive(type_str)
    return GroundType.path(type_str)


# ---------------------------------------------------------------------------
# Initial state from task
# ---------------------------------------------------------------------------

def build_initial_state(pcpn: PCPN, task: dict) -> SimState:
    state = SimState(marking=Marking(), stack=BorrowStack())
    initial = task.get("initial_resources", [])
    for res in initial:
        ty_str = res if isinstance(res, str) else res.get("type", "")
        ty = _parse_goal_type(ty_str, pcpn)
        if ty:
            pid = pcpn.get_place(ty, TypeForm.Value, Capability.Own)
            if pid is not None:
                vid = state.fresh_vid()
                state.marking.add(pid, Token(vid=vid, ty=ty, form=TypeForm.Value))
    return state


# ---------------------------------------------------------------------------
# CloseStack — append cleanup transitions
# ---------------------------------------------------------------------------

def close_stack(
    trace: list[TraceFiring],
    state: SimState,
    pcpn: PCPN,
    config: SearchConfig,
) -> tuple[list[TraceFiring], SimState]:
    """Append EndBorrow and Drop firings to clear the borrow stack."""
    extended = list(trace)
    current = state.clone()
    max_close_steps = 20

    for _ in range(max_close_steps):
        if current.stack.depth() == 0:
            break

        fired = False
        for t in pcpn.transitions:
            if t.kind not in (
                TransitionKind.EndBorrowShrKeepFrz,
                TransitionKind.EndBorrowShrUnfreeze,
                TransitionKind.EndBorrowMut,
                TransitionKind.Drop,
            ):
                continue
            result = try_fire(t, current, pcpn, config)
            if result is not None:
                current, firing = result
                extended.append(firing)
                fired = True
                break
        if not fired:
            break

    for _ in range(max_close_steps):
        has_tokens = False
        for pid, toks in current.marking.tokens.items():
            if toks:
                has_tokens = True
                break
        if not has_tokens:
            break

        fired = False
        for t in pcpn.transitions:
            if t.kind != TransitionKind.Drop:
                continue
            result = try_fire(t, current, pcpn, config)
            if result is not None:
                current, firing = result
                extended.append(firing)
                fired = True
                break
        if not fired:
            break

    return extended, current


# ---------------------------------------------------------------------------
# Bounded reachability — BFS worklist
# ---------------------------------------------------------------------------

def bounded_reachability(pcpn: PCPN, task: dict, search_cfg: dict | None = None) -> SearchResult:
    """BFS worklist search with bounded reachability."""
    cfg = SearchConfig()
    if search_cfg:
        cfg.max_trace_len = search_cfg.get("max_trace_len", cfg.max_trace_len)
        cfg.stack_depth = search_cfg.get("stack_depth", cfg.stack_depth)
        cfg.token_per_place = search_cfg.get("token_per_place", cfg.token_per_place)
        cfg.use_stack = search_cfg.get("use_stack", cfg.use_stack)
        cfg.use_capability = search_cfg.get("use_capability", cfg.use_capability)
        cfg.use_obligations = search_cfg.get("use_obligations", cfg.use_obligations)
        cfg.type_only = search_cfg.get("type_only", cfg.type_only)
        cfg.max_witnesses = search_cfg.get("max_witnesses", cfg.max_witnesses)

    if cfg.type_only:
        cfg.use_stack = False
        cfg.use_capability = False
        cfg.use_obligations = False

    initial = build_initial_state(pcpn, task)
    canon = Canonicalizer()

    start_time = time.time()
    result = SearchResult()

    worklist: deque[tuple[SimState, list[TraceFiring]]] = deque()
    worklist.append((initial, []))
    visited: set[str] = set()

    init_canon = canon.canonicalize(initial.marking, initial.stack)
    visited.add(init_canon.hash_key())

    goal = task.get("goal", task)

    while worklist:
        state, trace = worklist.popleft()
        result.states_explored += 1

        if len(trace) > cfg.max_trace_len:
            continue

        if check_goal(state, pcpn, goal):
            closed_trace, closed_state = close_stack(trace, state, pcpn, cfg)
            result.witnesses.append(closed_trace)
            result.goal_reached = True
            if len(result.witnesses) >= cfg.max_witnesses:
                break
            continue

        if len(trace) >= cfg.max_trace_len:
            continue

        for t in pcpn.transitions:
            fire_result = try_fire(t, state, pcpn, cfg)
            if fire_result is None:
                continue

            new_state, firing = fire_result
            canon_state = canon.canonicalize(new_state.marking, new_state.stack)
            key = canon_state.hash_key()

            if key not in visited:
                visited.add(key)
                new_trace = trace + [firing]
                worklist.append((new_state, new_trace))

    result.search_time_ms = (time.time() - start_time) * 1000
    return result
