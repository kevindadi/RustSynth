"""
Obligation checking — trait entailment, associated type entailment,
and outlives entailment over the borrow stack.

Paper mapping:
  - check_trait_bound     ≈  trait entailment  (T : Trait)
  - check_assoc_equality  ≈  assoc entailment  (<T as Trait>::Assoc = U)
  - check_outlives        ≈  outlives entailment over stack (simplified)

These are used as guards during transition enablement in the PCPN search.
"""

from __future__ import annotations

from rustsynth.core.types import (
    GroundType, BorrowStack, StackFrameKind, VarId,
)
from rustsynth.core.env import SigmaC


def check_trait_bound(sigma: SigmaC, ty: GroundType, trait_name: str) -> bool:
    """Check if type `ty` satisfies trait bound `trait_name` in Sigma(C)."""
    return sigma.implements_trait(ty, trait_name)


def check_assoc_equality(
    sigma: SigmaC,
    ty: GroundType,
    trait_name: str,
    assoc_name: str,
    expected: GroundType,
) -> bool:
    """Check <ty as trait_name>::assoc_name == expected."""
    resolved = sigma.resolve_assoc(ty, trait_name, assoc_name)
    if resolved is None:
        return False
    return resolved == expected


def check_outlives(stack: BorrowStack, borrower_vid: VarId, lender_vid: VarId) -> bool:
    """Check that the lender's borrow scope encloses the borrower's scope.

    In the pushdown stack model, this means the lender's frame must be
    deeper (earlier) in the stack than the borrower's frame.
    """
    lender_depth = -1
    borrower_depth = -1
    for i, frame in enumerate(stack.frames):
        if frame.owner_vid == lender_vid:
            lender_depth = i
        if frame.ref_vid == borrower_vid:
            borrower_depth = i

    if lender_depth < 0 or borrower_depth < 0:
        return True
    return lender_depth <= borrower_depth


def check_all_obligations(
    sigma: SigmaC,
    bounds: list[tuple[str, list[str]]],
    subst: dict[str, GroundType],
) -> bool:
    """Check all trait bounds are satisfied under a substitution."""
    for var_name, trait_names in bounds:
        ty = subst.get(var_name)
        if ty is None:
            continue
        for trait_name in trait_names:
            if not check_trait_bound(sigma, ty, trait_name):
                return False
    return True


class ObligationChecker:
    """Stateful obligation checker used during search."""

    def __init__(self, sigma: SigmaC, *, check_obligations: bool = True):
        self.sigma = sigma
        self.enabled = check_obligations

    def check_transition_obligations(
        self,
        bounds: list[tuple[str, list[str]]],
        subst: dict[str, GroundType],
        stack: BorrowStack,
    ) -> bool:
        if not self.enabled:
            return True
        return check_all_obligations(self.sigma, bounds, subst)
