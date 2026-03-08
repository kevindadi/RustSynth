"""
PCPN (Pushdown Colored Petri Net) — 9-Place Model.

Paper mapping:
  PCPN = (P, T, W, Col, Gamma, M0, Guard, Out, Act)

  P     = set of places (9 per ground type: {T,&T,&mut T} x {own,frz,blk})
  T     = set of transitions (API calls + structural rules)
  W     = arc weight/inscription functions
  Col   = color sets (Token)
  Gamma = stack alphabet (StackFrame kinds)
  M0    = initial marking
  Guard = enabling guard predicates
  Out   = output arc functions
  Act   = stack actions on transitions
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum, auto
from typing import Optional

from rustsynth.core.types import (
    GroundType, TypeForm, Capability, PlaceKey, Place, Token,
    Marking, BorrowStack, StackFrame, StackFrameKind, VarId,
)
from rustsynth.core.env import (
    SigmaC, CallableItem, PassingMode, ReturnMode,
)


# ---------------------------------------------------------------------------
# Transition kinds
# ---------------------------------------------------------------------------

class TransitionKind(Enum):
    ApiCall = "api_call"
    CreatePrimitive = "create_primitive"
    ConstProducer = "const_producer"
    BorrowShrFirst = "borrow_shr_first"
    BorrowShrNext = "borrow_shr_next"
    BorrowMut = "borrow_mut"
    EndBorrowShrKeepFrz = "end_borrow_shr_keep_frz"
    EndBorrowShrUnfreeze = "end_borrow_shr_unfreeze"
    EndBorrowMut = "end_borrow_mut"
    Drop = "drop"
    CopyUse = "copy_use"
    DupClone = "dup_clone"
    Move = "move"
    ProjMove = "proj_move"
    ProjRef = "proj_ref"
    Reborrow = "reborrow"
    DerefCopy = "deref_copy"


# ---------------------------------------------------------------------------
# Guard kinds
# ---------------------------------------------------------------------------

class GuardKind(Enum):
    NoFrzNoBlk = "no_frz_no_blk"
    NoBlk = "no_blk"
    NoFrzNoOtherBlk = "no_frz_no_other_blk"
    NotBlocked = "not_blocked"
    StackTopMatches = "stack_top_matches"
    PlaceCountRange = "place_count_range"
    StackDepthMax = "stack_depth_max"
    And = "and"


@dataclass
class Guard:
    kind: GuardKind
    base_type: Optional[GroundType] = None
    form: Optional[TypeForm] = None
    cap: Optional[Capability] = None
    min_count: int = 0
    max_count: int = 999
    max_depth: int = 999
    sub_guards: list[Guard] = field(default_factory=list)
    owner_vid: Optional[VarId] = None


# ---------------------------------------------------------------------------
# Arc
# ---------------------------------------------------------------------------

@dataclass
class Arc:
    place_id: int
    consumes: bool = False
    param_name: Optional[str] = None
    param_index: Optional[int] = None
    is_return: bool = False
    is_self: bool = False


# ---------------------------------------------------------------------------
# Stack action
# ---------------------------------------------------------------------------

class StackActionKind(Enum):
    Push = "push"
    Pop = "pop"
    Nop = "nop"


@dataclass
class StackAction:
    kind: StackActionKind = StackActionKind.Nop
    frame_kind: Optional[StackFrameKind] = None
    base_type: Optional[GroundType] = None


# ---------------------------------------------------------------------------
# Transition
# ---------------------------------------------------------------------------

@dataclass
class Transition:
    id: int
    name: str
    kind: TransitionKind
    input_arcs: list[Arc] = field(default_factory=list)
    output_arcs: list[Arc] = field(default_factory=list)
    guards: list[Guard] = field(default_factory=list)
    stack_action: StackAction = field(default_factory=StackAction)
    base_type: Optional[GroundType] = None
    fn_path: Optional[str] = None
    field_name: Optional[str] = None

    def label(self) -> str:
        if self.fn_path:
            return self.fn_path
        return self.name


# ---------------------------------------------------------------------------
# PCPN
# ---------------------------------------------------------------------------

@dataclass
class PCPN:
    places: list[Place] = field(default_factory=list)
    transitions: list[Transition] = field(default_factory=list)
    place_index: dict[PlaceKey, int] = field(default_factory=dict)
    type_universe: list[GroundType] = field(default_factory=list)
    sigma: Optional[SigmaC] = None

    # convenience counters
    _next_tid: int = 0

    def get_place(self, base_type: GroundType, form: TypeForm, cap: Capability) -> Optional[int]:
        key = PlaceKey(base_type, form, cap)
        return self.place_index.get(key)

    def get_or_create_place(self, base_type: GroundType, form: TypeForm, cap: Capability, budget: int = 3) -> int:
        key = PlaceKey(base_type, form, cap)
        if key in self.place_index:
            return self.place_index[key]
        pid = len(self.places)
        self.places.append(Place(id=pid, base_type=base_type, form=form, cap=cap, budget=budget))
        self.place_index[key] = pid
        return pid

    def add_transition(self, t: Transition) -> None:
        t.id = self._next_tid
        self._next_tid += 1
        self.transitions.append(t)

    def create_9_places(self, base_type: GroundType, budget: int = 3) -> None:
        for form in TypeForm:
            for cap in Capability:
                self.get_or_create_place(base_type, form, cap, budget)

    def register_type(self, ty: GroundType) -> None:
        if ty not in self.type_universe:
            self.type_universe.append(ty)

    @staticmethod
    def from_sigma(
        sigma: SigmaC,
        token_per_place: int = 3,
        check_obligations: bool = True,
    ) -> PCPN:
        """Build a PCPN from a Sigma(C) environment."""
        from rustsynth.core.structural_rules import add_structural_transitions
        from rustsynth.core.api_transitions import add_api_transitions

        pcpn = PCPN(sigma=sigma)

        for ty in sigma.type_universe:
            pcpn.register_type(ty)
            pcpn.create_9_places(ty, budget=token_per_place)

        add_api_transitions(pcpn, sigma, check_obligations=check_obligations)
        for ty in list(pcpn.type_universe):
            add_structural_transitions(pcpn, ty, sigma)

        return pcpn

    def to_dict(self) -> dict:
        return {
            "num_places": len(self.places),
            "num_transitions": len(self.transitions),
            "type_universe": [t.full_name() for t in self.type_universe],
            "transitions": [
                {"id": t.id, "name": t.name, "kind": t.kind.value}
                for t in self.transitions
            ],
        }
