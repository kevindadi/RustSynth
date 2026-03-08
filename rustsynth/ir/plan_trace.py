"""
PlanTrace — sequence of firing instances with labels, substitutions,
chosen inputs, fresh ids, and stack operations.

This is the raw output from the PCPN search.  It must be lowered to
SnippetIR before rendering to Rust source code.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional

from rustsynth.core.types import GroundType, TypeForm, VarId
from rustsynth.core.pcpn import TransitionKind
from rustsynth.core.search import TraceFiring


@dataclass
class FiringInstance:
    """One step in a plan trace."""
    step: int
    transition_name: str
    kind: TransitionKind
    fn_path: Optional[str] = None
    field_name: Optional[str] = None
    base_type: Optional[GroundType] = None
    consumed_vids: list[tuple[int, VarId, GroundType, TypeForm]] = field(default_factory=list)
    produced_vids: list[tuple[int, VarId, GroundType, TypeForm]] = field(default_factory=list)

    @property
    def is_api_call(self) -> bool:
        return self.kind in (TransitionKind.ApiCall, TransitionKind.ConstProducer)

    @property
    def is_structural(self) -> bool:
        return not self.is_api_call and self.kind != TransitionKind.CreatePrimitive

    @property
    def is_borrow(self) -> bool:
        return self.kind in (
            TransitionKind.BorrowShrFirst,
            TransitionKind.BorrowShrNext,
            TransitionKind.BorrowMut,
        )

    @property
    def is_end_borrow(self) -> bool:
        return self.kind in (
            TransitionKind.EndBorrowShrKeepFrz,
            TransitionKind.EndBorrowShrUnfreeze,
            TransitionKind.EndBorrowMut,
        )

    @property
    def is_drop(self) -> bool:
        return self.kind == TransitionKind.Drop


@dataclass
class PlanTrace:
    """A complete plan trace — a sequence of firing instances."""
    task_id: str = ""
    firings: list[FiringInstance] = field(default_factory=list)

    @staticmethod
    def from_witness(witness: list[TraceFiring], task_id: str = "") -> PlanTrace:
        plan = PlanTrace(task_id=task_id)
        for i, firing in enumerate(witness):
            consumed = [
                (pid, tok.vid, tok.ty, tok.form)
                for pid, tok in firing.consumed
            ]
            produced = [
                (pid, tok.vid, tok.ty, tok.form)
                for pid, tok in firing.produced
            ]
            plan.firings.append(FiringInstance(
                step=i,
                transition_name=firing.name,
                kind=firing.kind,
                fn_path=firing.fn_path,
                field_name=firing.field_name,
                base_type=firing.base_type,
                consumed_vids=consumed,
                produced_vids=produced,
            ))
        return plan

    def to_dict(self) -> dict:
        return {
            "task_id": self.task_id,
            "steps": [
                {
                    "step": f.step,
                    "name": f.transition_name,
                    "kind": f.kind.value,
                    "fn_path": f.fn_path,
                    "consumed": [(pid, vid) for pid, vid, _, _ in f.consumed_vids],
                    "produced": [(pid, vid) for pid, vid, _, _ in f.produced_vids],
                }
                for f in self.firings
            ],
        }
