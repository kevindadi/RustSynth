"""
Near-miss negative trace generator via 1-step mutations.

Mutation strategies:
  1. delete_drop       — remove a necessary drop/end_borrow
  2. swap_borrow_kind  — change shared borrow to mutable or vice versa
  3. reuse_moved       — copy a firing that consumes a value already consumed
  4. bad_trait_inst     — replace a type argument with one that doesn't satisfy bounds
  5. scramble_discharge — reorder end_borrow to violate stack discipline
  6. replace_proj      — replace a field projection with wrong field
"""

from __future__ import annotations

import copy
import random
from typing import Optional

from rustsynth.core.pcpn import TransitionKind
from rustsynth.core.search import TraceFiring
from rustsynth.core.types import GroundType, TypeForm, Token


def generate_near_miss_traces(
    witness: list[TraceFiring],
    seed: int = 42,
    max_mutations: int = 6,
) -> list[tuple[str, list[TraceFiring]]]:
    """Generate near-miss negative traces from a positive witness.

    Returns list of (mutation_name, mutated_trace) pairs.
    """
    rng = random.Random(seed)
    mutations: list[tuple[str, list[TraceFiring]]] = []

    m = _delete_drop(witness)
    if m:
        mutations.append(("delete_drop", m))

    m = _swap_borrow_kind(witness)
    if m:
        mutations.append(("swap_borrow_kind", m))

    m = _reuse_moved(witness)
    if m:
        mutations.append(("reuse_moved", m))

    m = _scramble_discharge(witness)
    if m:
        mutations.append(("scramble_discharge", m))

    m = _delete_end_borrow(witness)
    if m:
        mutations.append(("delete_end_borrow", m))

    m = _duplicate_mut_borrow(witness)
    if m:
        mutations.append(("duplicate_mut_borrow", m))

    return mutations[:max_mutations]


def _delete_drop(trace: list[TraceFiring]) -> Optional[list[TraceFiring]]:
    """Remove the first drop/end_borrow firing."""
    for i, f in enumerate(trace):
        if f.kind in (TransitionKind.Drop, TransitionKind.EndBorrowShrKeepFrz,
                       TransitionKind.EndBorrowShrUnfreeze, TransitionKind.EndBorrowMut):
            mutated = trace[:i] + trace[i + 1:]
            return mutated
    return None


def _swap_borrow_kind(trace: list[TraceFiring]) -> Optional[list[TraceFiring]]:
    """Swap a shared borrow to mutable or vice versa."""
    mutated = copy.deepcopy(trace)
    for f in mutated:
        if f.kind == TransitionKind.BorrowShrFirst:
            f.kind = TransitionKind.BorrowMut
            f.name = f.name.replace("BorrowShrFirst", "BorrowMut")
            return mutated
        if f.kind == TransitionKind.BorrowMut:
            f.kind = TransitionKind.BorrowShrFirst
            f.name = f.name.replace("BorrowMut", "BorrowShrFirst")
            return mutated
    return None


def _reuse_moved(trace: list[TraceFiring]) -> Optional[list[TraceFiring]]:
    """Duplicate a move-consuming step after the original to create use-after-move."""
    for i, f in enumerate(trace):
        if not f.consumed:
            continue
        for pid, tok in f.consumed:
            if tok.form == TypeForm.Value:
                mutated = list(trace)
                dup = copy.deepcopy(f)
                mutated.insert(i + 1, dup)
                return mutated
    return None


def _scramble_discharge(trace: list[TraceFiring]) -> Optional[list[TraceFiring]]:
    """Swap order of two adjacent end-borrow operations."""
    end_indices = [
        i for i, f in enumerate(trace)
        if f.kind in (TransitionKind.EndBorrowShrKeepFrz,
                       TransitionKind.EndBorrowShrUnfreeze,
                       TransitionKind.EndBorrowMut)
    ]
    if len(end_indices) >= 2:
        mutated = list(trace)
        i, j = end_indices[0], end_indices[1]
        mutated[i], mutated[j] = mutated[j], mutated[i]
        return mutated
    return None


def _delete_end_borrow(trace: list[TraceFiring]) -> Optional[list[TraceFiring]]:
    """Remove an end-borrow without removing the corresponding borrow."""
    for i, f in enumerate(trace):
        if f.kind in (TransitionKind.EndBorrowShrUnfreeze, TransitionKind.EndBorrowMut):
            return trace[:i] + trace[i + 1:]
    return None


def _duplicate_mut_borrow(trace: list[TraceFiring]) -> Optional[list[TraceFiring]]:
    """Duplicate a mutable borrow to create conflicting borrows."""
    for i, f in enumerate(trace):
        if f.kind == TransitionKind.BorrowMut:
            mutated = list(trace)
            dup = copy.deepcopy(f)
            mutated.insert(i + 1, dup)
            return mutated
    return None
