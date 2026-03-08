"""
Structural transition rules for the PCPN.

Paper mapping — these are the non-API transitions that model Rust's
ownership, borrowing, and lifetime semantics:

  Move            — consume owned value
  CopyUse         — copy an owned Copy-type value
  DupClone        — clone a Clone-type value via &T
  DropOwn         — drop an owned value
  BorrowShrFirst  — create first shared borrow: T(own) -> &T(own) + T(frz)
  BorrowShrNext   — create additional shared borrow from frozen: T(frz) -> &T(own) + T(frz)
  BorrowMut       — create mutable borrow: T(own) -> &mut T(own) + T(blk)
  EndBorrowShrKeepFrz — end shared borrow, keep frozen: &T(own) -> [pop stack]
  EndBorrowShrUnfreeze — end last shared borrow, unfreeze: &T(own) -> T(own) [pop stack]
  EndBorrowMut    — end mutable borrow: &mut T(own) -> T(own) [pop stack]
  ProjMove        — move out a field from a struct
  ProjRef         — project a field reference from &struct / &mut struct
  Reborrow        — reborrow &mut T as &T or &mut T
  DerefCopy       — dereference &T where T: Copy to get owned T
"""

from __future__ import annotations

from rustsynth.core.types import (
    GroundType, TypeForm, Capability, StackFrameKind,
)
from rustsynth.core.env import SigmaC
from rustsynth.core.pcpn import (
    PCPN, Transition, TransitionKind, Arc, Guard, GuardKind,
    StackAction, StackActionKind,
)


def add_structural_transitions(pcpn: PCPN, ty: GroundType, sigma: SigmaC) -> None:
    """Add all structural transitions for a ground type."""
    _add_borrow_shr_first(pcpn, ty)
    _add_borrow_shr_next(pcpn, ty)
    _add_borrow_mut(pcpn, ty)
    _add_end_borrow_shr_keep_frz(pcpn, ty)
    _add_end_borrow_shr_unfreeze(pcpn, ty)
    _add_end_borrow_mut(pcpn, ty)
    _add_drop_val(pcpn, ty)
    _add_drop_shr(pcpn, ty)
    _add_drop_mut(pcpn, ty)

    if sigma.is_type_copy(ty):
        _add_copy_use(pcpn, ty)
        _add_deref_copy(pcpn, ty)

    if sigma.is_type_clone(ty) and not sigma.is_type_copy(ty):
        _add_dup_clone(pcpn, ty)

    for fi in sigma.visible_fields(ty):
        _add_proj_move(pcpn, ty, fi.name, fi.ty)
        _add_proj_ref_shr(pcpn, ty, fi.name, fi.ty)
        _add_proj_ref_mut(pcpn, ty, fi.name, fi.ty)

    _add_reborrow_shr_from_mut(pcpn, ty)


# ---------------------------------------------------------------------------
# Borrow rules
# ---------------------------------------------------------------------------

def _add_borrow_shr_first(pcpn: PCPN, ty: GroundType) -> None:
    """T(own) -> &T(own) + T(frz), push Freeze frame."""
    p_val_own = pcpn.get_place(ty, TypeForm.Value, Capability.Own)
    p_ref_own = pcpn.get_place(ty, TypeForm.RefShr, Capability.Own)
    p_val_frz = pcpn.get_place(ty, TypeForm.Value, Capability.Frz)
    if p_val_own is None or p_ref_own is None or p_val_frz is None:
        return

    pcpn.add_transition(Transition(
        id=0, name=f"BorrowShrFirst({ty.short_name()})",
        kind=TransitionKind.BorrowShrFirst,
        input_arcs=[Arc(place_id=p_val_own, consumes=True)],
        output_arcs=[Arc(place_id=p_ref_own), Arc(place_id=p_val_frz)],
        guards=[
            Guard(kind=GuardKind.NoFrzNoBlk, base_type=ty),
        ],
        stack_action=StackAction(
            kind=StackActionKind.Push,
            frame_kind=StackFrameKind.Freeze,
            base_type=ty,
        ),
        base_type=ty,
    ))


def _add_borrow_shr_next(pcpn: PCPN, ty: GroundType) -> None:
    """T(frz) -> &T(own) + T(frz), push Shr frame."""
    p_val_frz = pcpn.get_place(ty, TypeForm.Value, Capability.Frz)
    p_ref_own = pcpn.get_place(ty, TypeForm.RefShr, Capability.Own)
    if p_val_frz is None or p_ref_own is None:
        return

    pcpn.add_transition(Transition(
        id=0, name=f"BorrowShrNext({ty.short_name()})",
        kind=TransitionKind.BorrowShrNext,
        input_arcs=[Arc(place_id=p_val_frz, consumes=False)],
        output_arcs=[Arc(place_id=p_ref_own)],
        guards=[],
        stack_action=StackAction(
            kind=StackActionKind.Push,
            frame_kind=StackFrameKind.Shr,
            base_type=ty,
        ),
        base_type=ty,
    ))


def _add_borrow_mut(pcpn: PCPN, ty: GroundType) -> None:
    """T(own) -> &mut T(own) + T(blk), push Mut frame."""
    p_val_own = pcpn.get_place(ty, TypeForm.Value, Capability.Own)
    p_mut_own = pcpn.get_place(ty, TypeForm.RefMut, Capability.Own)
    p_val_blk = pcpn.get_place(ty, TypeForm.Value, Capability.Blk)
    if p_val_own is None or p_mut_own is None or p_val_blk is None:
        return

    pcpn.add_transition(Transition(
        id=0, name=f"BorrowMut({ty.short_name()})",
        kind=TransitionKind.BorrowMut,
        input_arcs=[Arc(place_id=p_val_own, consumes=True)],
        output_arcs=[Arc(place_id=p_mut_own), Arc(place_id=p_val_blk)],
        guards=[
            Guard(kind=GuardKind.NoFrzNoBlk, base_type=ty),
        ],
        stack_action=StackAction(
            kind=StackActionKind.Push,
            frame_kind=StackFrameKind.Mut,
            base_type=ty,
        ),
        base_type=ty,
    ))


def _add_end_borrow_shr_keep_frz(pcpn: PCPN, ty: GroundType) -> None:
    """End a shared borrow (not the last one): &T(own) consumed, keep T(frz), pop Shr."""
    p_ref_own = pcpn.get_place(ty, TypeForm.RefShr, Capability.Own)
    if p_ref_own is None:
        return

    pcpn.add_transition(Transition(
        id=0, name=f"EndBorrowShrKeepFrz({ty.short_name()})",
        kind=TransitionKind.EndBorrowShrKeepFrz,
        input_arcs=[Arc(place_id=p_ref_own, consumes=True)],
        output_arcs=[],
        guards=[
            Guard(kind=GuardKind.StackTopMatches, base_type=ty),
        ],
        stack_action=StackAction(
            kind=StackActionKind.Pop,
            frame_kind=StackFrameKind.Shr,
            base_type=ty,
        ),
        base_type=ty,
    ))


def _add_end_borrow_shr_unfreeze(pcpn: PCPN, ty: GroundType) -> None:
    """End the last shared borrow: &T(own) consumed + T(frz) consumed -> T(own), pop Freeze."""
    p_ref_own = pcpn.get_place(ty, TypeForm.RefShr, Capability.Own)
    p_val_frz = pcpn.get_place(ty, TypeForm.Value, Capability.Frz)
    p_val_own = pcpn.get_place(ty, TypeForm.Value, Capability.Own)
    if p_ref_own is None or p_val_frz is None or p_val_own is None:
        return

    pcpn.add_transition(Transition(
        id=0, name=f"EndBorrowShrUnfreeze({ty.short_name()})",
        kind=TransitionKind.EndBorrowShrUnfreeze,
        input_arcs=[
            Arc(place_id=p_ref_own, consumes=True),
            Arc(place_id=p_val_frz, consumes=True),
        ],
        output_arcs=[Arc(place_id=p_val_own)],
        guards=[
            Guard(kind=GuardKind.StackTopMatches, base_type=ty),
        ],
        stack_action=StackAction(
            kind=StackActionKind.Pop,
            frame_kind=StackFrameKind.Freeze,
            base_type=ty,
        ),
        base_type=ty,
    ))


def _add_end_borrow_mut(pcpn: PCPN, ty: GroundType) -> None:
    """End mutable borrow: &mut T(own) consumed + T(blk) consumed -> T(own), pop Mut."""
    p_mut_own = pcpn.get_place(ty, TypeForm.RefMut, Capability.Own)
    p_val_blk = pcpn.get_place(ty, TypeForm.Value, Capability.Blk)
    p_val_own = pcpn.get_place(ty, TypeForm.Value, Capability.Own)
    if p_mut_own is None or p_val_blk is None or p_val_own is None:
        return

    pcpn.add_transition(Transition(
        id=0, name=f"EndBorrowMut({ty.short_name()})",
        kind=TransitionKind.EndBorrowMut,
        input_arcs=[
            Arc(place_id=p_mut_own, consumes=True),
            Arc(place_id=p_val_blk, consumes=True),
        ],
        output_arcs=[Arc(place_id=p_val_own)],
        guards=[
            Guard(kind=GuardKind.StackTopMatches, base_type=ty),
        ],
        stack_action=StackAction(
            kind=StackActionKind.Pop,
            frame_kind=StackFrameKind.Mut,
            base_type=ty,
        ),
        base_type=ty,
    ))


# ---------------------------------------------------------------------------
# Drop rules
# ---------------------------------------------------------------------------

def _add_drop_val(pcpn: PCPN, ty: GroundType) -> None:
    """Drop an owned value: T(own) consumed."""
    p_val_own = pcpn.get_place(ty, TypeForm.Value, Capability.Own)
    if p_val_own is None:
        return
    pcpn.add_transition(Transition(
        id=0, name=f"DropOwn({ty.short_name()})",
        kind=TransitionKind.Drop,
        input_arcs=[Arc(place_id=p_val_own, consumes=True)],
        output_arcs=[],
        guards=[],
        base_type=ty,
    ))


def _add_drop_shr(pcpn: PCPN, ty: GroundType) -> None:
    """Drop a shared reference: &T(own) consumed."""
    p_ref_own = pcpn.get_place(ty, TypeForm.RefShr, Capability.Own)
    if p_ref_own is None:
        return
    pcpn.add_transition(Transition(
        id=0, name=f"DropShr({ty.short_name()})",
        kind=TransitionKind.Drop,
        input_arcs=[Arc(place_id=p_ref_own, consumes=True)],
        output_arcs=[],
        guards=[],
        base_type=ty,
    ))


def _add_drop_mut(pcpn: PCPN, ty: GroundType) -> None:
    """Drop a mutable reference: &mut T(own) consumed."""
    p_mut_own = pcpn.get_place(ty, TypeForm.RefMut, Capability.Own)
    if p_mut_own is None:
        return
    pcpn.add_transition(Transition(
        id=0, name=f"DropMut({ty.short_name()})",
        kind=TransitionKind.Drop,
        input_arcs=[Arc(place_id=p_mut_own, consumes=True)],
        output_arcs=[],
        guards=[],
        base_type=ty,
    ))


# ---------------------------------------------------------------------------
# Copy / Clone
# ---------------------------------------------------------------------------

def _add_copy_use(pcpn: PCPN, ty: GroundType) -> None:
    """Copy an owned value: T(own) read (not consumed) -> T(own) new copy."""
    p_val_own = pcpn.get_place(ty, TypeForm.Value, Capability.Own)
    if p_val_own is None:
        return
    pcpn.add_transition(Transition(
        id=0, name=f"CopyUse({ty.short_name()})",
        kind=TransitionKind.CopyUse,
        input_arcs=[Arc(place_id=p_val_own, consumes=False)],
        output_arcs=[Arc(place_id=p_val_own)],
        guards=[],
        base_type=ty,
    ))


def _add_deref_copy(pcpn: PCPN, ty: GroundType) -> None:
    """Dereference &T where T: Copy -> owned T."""
    p_ref_own = pcpn.get_place(ty, TypeForm.RefShr, Capability.Own)
    p_val_own = pcpn.get_place(ty, TypeForm.Value, Capability.Own)
    if p_ref_own is None or p_val_own is None:
        return
    pcpn.add_transition(Transition(
        id=0, name=f"DerefCopy({ty.short_name()})",
        kind=TransitionKind.DerefCopy,
        input_arcs=[Arc(place_id=p_ref_own, consumes=False)],
        output_arcs=[Arc(place_id=p_val_own)],
        guards=[],
        base_type=ty,
    ))


def _add_dup_clone(pcpn: PCPN, ty: GroundType) -> None:
    """Clone from &T: &T(own) read -> T(own) new clone."""
    p_ref_own = pcpn.get_place(ty, TypeForm.RefShr, Capability.Own)
    p_val_own = pcpn.get_place(ty, TypeForm.Value, Capability.Own)
    if p_ref_own is None or p_val_own is None:
        return
    pcpn.add_transition(Transition(
        id=0, name=f"DupClone({ty.short_name()})",
        kind=TransitionKind.DupClone,
        input_arcs=[Arc(place_id=p_ref_own, consumes=False)],
        output_arcs=[Arc(place_id=p_val_own)],
        guards=[],
        base_type=ty,
    ))


# ---------------------------------------------------------------------------
# Field projection
# ---------------------------------------------------------------------------

def _add_proj_move(pcpn: PCPN, owner_ty: GroundType, field_name: str, field_ty: GroundType) -> None:
    """Move-project a field: consume T(own) -> field_T(own)."""
    pcpn.register_type(field_ty)
    pcpn.create_9_places(field_ty)
    p_owner_own = pcpn.get_place(owner_ty, TypeForm.Value, Capability.Own)
    p_field_own = pcpn.get_place(field_ty, TypeForm.Value, Capability.Own)
    if p_owner_own is None or p_field_own is None:
        return
    pcpn.add_transition(Transition(
        id=0, name=f"ProjMove({owner_ty.short_name()}.{field_name})",
        kind=TransitionKind.ProjMove,
        input_arcs=[Arc(place_id=p_owner_own, consumes=True)],
        output_arcs=[Arc(place_id=p_field_own)],
        guards=[Guard(kind=GuardKind.NoFrzNoBlk, base_type=owner_ty)],
        base_type=owner_ty,
        field_name=field_name,
    ))


def _add_proj_ref_shr(pcpn: PCPN, owner_ty: GroundType, field_name: str, field_ty: GroundType) -> None:
    """Shared-ref project: &T(own) read -> &field_T(own)."""
    pcpn.register_type(field_ty)
    pcpn.create_9_places(field_ty)
    p_ref_own = pcpn.get_place(owner_ty, TypeForm.RefShr, Capability.Own)
    p_field_ref = pcpn.get_place(field_ty, TypeForm.RefShr, Capability.Own)
    if p_ref_own is None or p_field_ref is None:
        return
    pcpn.add_transition(Transition(
        id=0, name=f"ProjRefShr({owner_ty.short_name()}.{field_name})",
        kind=TransitionKind.ProjRef,
        input_arcs=[Arc(place_id=p_ref_own, consumes=False)],
        output_arcs=[Arc(place_id=p_field_ref)],
        guards=[],
        base_type=owner_ty,
        field_name=field_name,
    ))


def _add_proj_ref_mut(pcpn: PCPN, owner_ty: GroundType, field_name: str, field_ty: GroundType) -> None:
    """Mut-ref project: &mut T(own) read -> &mut field_T(own)."""
    pcpn.register_type(field_ty)
    pcpn.create_9_places(field_ty)
    p_mut_own = pcpn.get_place(owner_ty, TypeForm.RefMut, Capability.Own)
    p_field_mut = pcpn.get_place(field_ty, TypeForm.RefMut, Capability.Own)
    if p_mut_own is None or p_field_mut is None:
        return
    pcpn.add_transition(Transition(
        id=0, name=f"ProjRefMut({owner_ty.short_name()}.{field_name})",
        kind=TransitionKind.ProjRef,
        input_arcs=[Arc(place_id=p_mut_own, consumes=False)],
        output_arcs=[Arc(place_id=p_field_mut)],
        guards=[],
        base_type=owner_ty,
        field_name=field_name,
    ))


# ---------------------------------------------------------------------------
# Reborrow
# ---------------------------------------------------------------------------

def _add_reborrow_shr_from_mut(pcpn: PCPN, ty: GroundType) -> None:
    """Reborrow: &mut T -> &T  (creates shared ref from mutable ref)."""
    p_mut_own = pcpn.get_place(ty, TypeForm.RefMut, Capability.Own)
    p_ref_own = pcpn.get_place(ty, TypeForm.RefShr, Capability.Own)
    if p_mut_own is None or p_ref_own is None:
        return
    pcpn.add_transition(Transition(
        id=0, name=f"ReborrowShrFromMut({ty.short_name()})",
        kind=TransitionKind.Reborrow,
        input_arcs=[Arc(place_id=p_mut_own, consumes=False)],
        output_arcs=[Arc(place_id=p_ref_own)],
        guards=[],
        base_type=ty,
    ))
