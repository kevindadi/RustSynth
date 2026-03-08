"""
Convert CallableItem entries from Sigma(C) into PCPN transitions.

Each public function/method becomes one (or more, after monomorphization)
PCPN transition with input arcs from parameter places and output arcs
to return-type places.
"""

from __future__ import annotations

from rustsynth.core.types import GroundType, TypeForm, Capability
from rustsynth.core.env import SigmaC, CallableItem, PassingMode, ReturnMode
from rustsynth.core.pcpn import (
    PCPN, Transition, TransitionKind, Arc, Guard, GuardKind,
    StackAction, StackActionKind,
)
from rustsynth.core.types import StackFrameKind


def add_api_transitions(
    pcpn: PCPN,
    sigma: SigmaC,
    check_obligations: bool = True,
) -> None:
    """Add transitions for all callable items in Sigma(C)."""
    for callable_item in sigma.callables:
        if callable_item.path.startswith("__literal_"):
            _add_literal_provider(pcpn, callable_item)
        elif callable_item.type_vars:
            _add_monomorphized_transitions(
                pcpn, sigma, callable_item,
                check_obligations=check_obligations,
            )
        else:
            _add_single_api_transition(pcpn, sigma, callable_item)


def _add_literal_provider(pcpn: PCPN, item: CallableItem) -> None:
    """Add a 0-ary provider transition for a literal/primitive type."""
    if item.return_type is None:
        return
    pcpn.register_type(item.return_type)
    pcpn.create_9_places(item.return_type)

    is_ref_provider = item.path.endswith("_ref")
    if is_ref_provider:
        p_out = pcpn.get_place(item.return_type, TypeForm.RefShr, Capability.Own)
        form_name = f"&{item.return_type.short_name()}"
    else:
        p_out = pcpn.get_place(item.return_type, TypeForm.Value, Capability.Own)
        form_name = item.return_type.short_name()
    if p_out is None:
        return

    guard_form = TypeForm.RefShr if is_ref_provider else TypeForm.Value
    pcpn.add_transition(Transition(
        id=0, name=f"Literal({form_name})",
        kind=TransitionKind.CreatePrimitive,
        input_arcs=[],
        output_arcs=[Arc(place_id=p_out, is_return=True)],
        guards=[
            Guard(kind=GuardKind.PlaceCountRange, base_type=item.return_type,
                  form=guard_form, cap=Capability.Own, max_count=2),
        ],
        base_type=item.return_type,
        fn_path=item.path,
    ))


def _add_single_api_transition(pcpn: PCPN, sigma: SigmaC, item: CallableItem) -> None:
    """Add a single transition for a non-generic callable."""
    input_arcs = []
    output_arcs = []
    guards = []
    stack_action = StackAction()

    for i, param in enumerate(item.params):
        pcpn.register_type(param.ty)
        pcpn.create_9_places(param.ty)

        if param.passing == PassingMode.Move:
            pid = pcpn.get_place(param.ty, TypeForm.Value, Capability.Own)
            if pid is not None:
                input_arcs.append(Arc(
                    place_id=pid, consumes=True,
                    param_name=param.name, param_index=i,
                    is_self=param.is_self,
                ))
        elif param.passing == PassingMode.BorrowShr:
            pid = pcpn.get_place(param.ty, TypeForm.RefShr, Capability.Own)
            if pid is not None:
                input_arcs.append(Arc(
                    place_id=pid, consumes=False,
                    param_name=param.name, param_index=i,
                    is_self=param.is_self,
                ))
        elif param.passing == PassingMode.BorrowMut:
            pid = pcpn.get_place(param.ty, TypeForm.RefMut, Capability.Own)
            if pid is not None:
                input_arcs.append(Arc(
                    place_id=pid, consumes=False,
                    param_name=param.name, param_index=i,
                    is_self=param.is_self,
                ))
        elif param.passing == PassingMode.Copy:
            pid = pcpn.get_place(param.ty, TypeForm.Value, Capability.Own)
            if pid is not None:
                input_arcs.append(Arc(
                    place_id=pid, consumes=False,
                    param_name=param.name, param_index=i,
                    is_self=param.is_self,
                ))

    if item.return_type is not None:
        pcpn.register_type(item.return_type)
        pcpn.create_9_places(item.return_type)

        if item.return_mode == ReturnMode.Owned:
            pid = pcpn.get_place(item.return_type, TypeForm.Value, Capability.Own)
        elif item.return_mode == ReturnMode.BorrowShr:
            pid = pcpn.get_place(item.return_type, TypeForm.RefShr, Capability.Own)
        elif item.return_mode == ReturnMode.BorrowMut:
            pid = pcpn.get_place(item.return_type, TypeForm.RefMut, Capability.Own)
        else:
            pid = None

        if pid is not None:
            output_arcs.append(Arc(place_id=pid, is_return=True))

        if item.return_mode in (ReturnMode.BorrowShr, ReturnMode.BorrowMut):
            self_param = item.self_param
            if self_param:
                frame_kind = (
                    StackFrameKind.Shr if item.return_mode == ReturnMode.BorrowShr
                    else StackFrameKind.Mut
                )
                stack_action = StackAction(
                    kind=StackActionKind.Push,
                    frame_kind=frame_kind,
                    base_type=self_param.ty,
                )

    pcpn.add_transition(Transition(
        id=0,
        name=item.path,
        kind=TransitionKind.ApiCall if not item.is_const else TransitionKind.ConstProducer,
        input_arcs=input_arcs,
        output_arcs=output_arcs,
        guards=guards,
        stack_action=stack_action,
        base_type=item.return_type,
        fn_path=item.path,
    ))


def _add_monomorphized_transitions(
    pcpn: PCPN,
    sigma: SigmaC,
    item: CallableItem,
    check_obligations: bool = True,
) -> None:
    """Enumerate monomorphic instances of a generic callable over the type universe."""
    from rustsynth.core.unify import TypeUniverse, enumerate_instantiations

    type_vars_with_bounds = []
    for tv in item.type_vars:
        tv_bounds = []
        for var_name, bounds in item.bounds:
            if var_name == tv:
                tv_bounds.extend(bounds)
        type_vars_with_bounds.append((tv, tv_bounds))

    universe = TypeUniverse(types=list(pcpn.type_universe))
    instantiations = enumerate_instantiations(
        type_vars_with_bounds, universe,
        sigma=sigma, check_obligations=check_obligations,
    )

    for subst in instantiations:
        mono_item = _apply_subst_to_callable(item, subst)
        _add_single_api_transition(pcpn, sigma, mono_item)


def _apply_subst_to_callable(item: CallableItem, subst: dict[str, GroundType]) -> CallableItem:
    """Apply a type substitution to a callable item."""
    from rustsynth.core.env import ParamInfo

    new_params = []
    for p in item.params:
        new_ty = _apply_subst_to_type(p.ty, subst)
        new_params.append(ParamInfo(
            name=p.name, ty=new_ty, passing=p.passing, is_self=p.is_self,
        ))

    new_ret = _apply_subst_to_type(item.return_type, subst) if item.return_type else None

    suffix = ",".join(f"{v.short_name()}" for v in subst.values())
    mono_path = f"{item.path}<{suffix}>" if suffix else item.path

    return CallableItem(
        path=mono_path,
        params=new_params,
        return_type=new_ret,
        return_mode=item.return_mode,
        type_vars=[],
        bounds=[],
        lifetime_params=item.lifetime_params,
        is_constructor=item.is_constructor,
        is_const=item.is_const,
    )


def _apply_subst_to_type(ty: GroundType, subst: dict[str, GroundType]) -> GroundType:
    if ty.kind == "path" and ty.name in subst and not ty.args:
        return subst[ty.name]
    if ty.args:
        new_args = tuple(_apply_subst_to_type(a, subst) for a in ty.args)
        if ty.kind == "path":
            return GroundType.path(ty.name, new_args)
        if ty.kind == "tuple":
            return GroundType.tuple_(new_args)
    return ty
