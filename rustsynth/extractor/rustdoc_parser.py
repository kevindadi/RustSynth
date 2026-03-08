"""Parse rustdoc JSON output to extract public API items.

Primary extraction path: generates rustdoc JSON via
  cargo +nightly rustdoc -Z unstable-options --output-format json
then parses the resulting JSON to extract public functions, structs,
impl blocks, trait bounds, associated types, etc.
"""

from __future__ import annotations

import json
import subprocess
import logging
from pathlib import Path
from typing import Optional

from rustsynth.core.types import GroundType, FieldInfo
from rustsynth.core.env import (
    CallableItem, ParamInfo, PassingMode, ReturnMode,
    StructDef, ImplFact, AssocFact, CopyCloneFact, SigmaC,
)

logger = logging.getLogger(__name__)


def generate_rustdoc_json(crate_path: Path) -> Optional[Path]:
    """Try to generate rustdoc JSON. Returns path to JSON file or None."""
    cmd = [
        "cargo", "+nightly", "rustdoc",
        "-Z", "unstable-options", "--output-format", "json",
    ]
    result = subprocess.run(
        cmd, cwd=str(crate_path), capture_output=True, text=True, timeout=120,
    )
    if result.returncode != 0:
        logger.warning("rustdoc JSON generation failed: %s", result.stderr[:500])
        return None

    target_dir = crate_path / "target" / "doc"
    for p in target_dir.glob("*.json"):
        if p.stem != "search-index":
            return p
    return None


def parse_rustdoc_json(json_path: Path) -> SigmaC:
    """Parse a rustdoc JSON file and extract SigmaC."""
    data = json.loads(json_path.read_text())
    sigma = SigmaC()

    root_id = data.get("root")
    index = data.get("index", {})
    paths = data.get("paths", {})

    root_item = index.get(root_id, {})
    sigma.crate_name = root_item.get("name", json_path.stem)

    for item_id, item in index.items():
        if item.get("visibility") != "public" and item_id != root_id:
            continue

        inner = item.get("inner", {})
        item_kind = _item_kind(inner)

        if item_kind == "function":
            _parse_function(item, inner, sigma)
        elif item_kind == "struct":
            _parse_struct(item, inner, index, sigma)
        elif item_kind == "impl":
            _parse_impl(item, inner, index, sigma)

    _collect_type_universe(sigma)
    return sigma


def _item_kind(inner: dict) -> str:
    if "function" in inner:
        return "function"
    if "struct" in inner:
        return "struct"
    if "impl" in inner:
        return "impl"
    if "module" in inner:
        return "module"
    return "other"


def _parse_type(ty: dict | str | None) -> Optional[GroundType]:
    """Convert a rustdoc type representation to GroundType."""
    if ty is None:
        return None

    if isinstance(ty, str):
        return GroundType.primitive(ty)

    if isinstance(ty, dict):
        if "primitive" in ty:
            return GroundType.primitive(ty["primitive"])

        if "resolved_path" in ty:
            rp = ty["resolved_path"]
            name = rp.get("name", "")
            args_raw = rp.get("args", {})
            args = []
            if isinstance(args_raw, dict) and "angle_bracketed" in args_raw:
                for arg in args_raw["angle_bracketed"].get("args", []):
                    if isinstance(arg, dict) and "type" in arg:
                        parsed = _parse_type(arg["type"])
                        if parsed:
                            args.append(parsed)
            return GroundType.path(name, tuple(args))

        if "borrowed_ref" in ty:
            br = ty["borrowed_ref"]
            inner_ty = _parse_type(br.get("type"))
            is_mut = br.get("is_mutable", False)
            return inner_ty

        if "tuple" in ty:
            elems = []
            for e in ty["tuple"]:
                parsed = _parse_type(e)
                if parsed:
                    elems.append(parsed)
            return GroundType.tuple_(tuple(elems))

        if "generic" in ty:
            return GroundType.path(ty["generic"])

        if "qualified_path" in ty:
            qp = ty["qualified_path"]
            return GroundType.path(qp.get("name", "AssocType"))

    return None


def _parse_passing_mode(ty_dict: dict | None) -> tuple[PassingMode, bool]:
    """Determine passing mode from the type dict. Returns (mode, is_self)."""
    if ty_dict is None:
        return PassingMode.Move, False

    if isinstance(ty_dict, dict) and "borrowed_ref" in ty_dict:
        br = ty_dict["borrowed_ref"]
        is_mut = br.get("is_mutable", False)
        return (PassingMode.BorrowMut if is_mut else PassingMode.BorrowShr), False

    return PassingMode.Move, False


def _parse_function(item: dict, inner: dict, sigma: SigmaC) -> None:
    fn_data = inner.get("function", {})
    sig = fn_data.get("sig", fn_data.get("decl", {}))
    name = item.get("name", "")

    params = []
    inputs = sig.get("inputs", [])
    for param_entry in inputs:
        if isinstance(param_entry, list) and len(param_entry) >= 2:
            pname, pty = param_entry[0], param_entry[1]
        elif isinstance(param_entry, dict):
            pname = param_entry.get("name", "")
            pty = param_entry.get("type")
        else:
            continue

        is_self = pname in ("self", "&self", "&mut self")
        passing, _ = _parse_passing_mode(pty)

        if is_self and isinstance(pty, dict) and "borrowed_ref" in pty:
            br = pty["borrowed_ref"]
            passing = PassingMode.BorrowMut if br.get("is_mutable", False) else PassingMode.BorrowShr

        ground_ty = _parse_type(pty)
        if ground_ty is None:
            ground_ty = GroundType.unit()

        params.append(ParamInfo(
            name=pname, ty=ground_ty, passing=passing, is_self=is_self,
        ))

    ret_ty_raw = sig.get("output")
    ret_ty = _parse_type(ret_ty_raw)
    ret_mode = ReturnMode.Owned
    if isinstance(ret_ty_raw, dict) and "borrowed_ref" in ret_ty_raw:
        br = ret_ty_raw["borrowed_ref"]
        ret_mode = ReturnMode.BorrowMut if br.get("is_mutable", False) else ReturnMode.BorrowShr

    generics = fn_data.get("generics", {})
    type_vars = []
    bounds_list = []
    for gp in generics.get("params", []):
        if gp.get("kind", {}).get("type") is not None:
            tv_name = gp.get("name", "")
            type_vars.append(tv_name)
            gp_bounds = gp.get("kind", {}).get("type", {}).get("bounds", [])
            trait_names = []
            for b in gp_bounds:
                if isinstance(b, dict) and "trait_bound" in b:
                    tb = b["trait_bound"]
                    trait_path = tb.get("trait", {})
                    trait_names.append(trait_path.get("name", ""))
            if trait_names:
                bounds_list.append((tv_name, trait_names))

    callable_item = CallableItem(
        path=name,
        params=params,
        return_type=ret_ty,
        return_mode=ret_mode,
        type_vars=type_vars,
        bounds=bounds_list,
        is_constructor=(not any(p.is_self for p in params) and ret_ty is not None),
        is_const=fn_data.get("has_body", True) and "const" in item.get("attrs", []),
    )
    sigma.add_callable(callable_item)


def _parse_struct(item: dict, inner: dict, index: dict, sigma: SigmaC) -> None:
    struct_data = inner.get("struct", {})
    name = item.get("name", "")

    fields = []
    for field_id in struct_data.get("fields", []):
        field_item = index.get(str(field_id), {})
        if field_item.get("visibility") != "public":
            continue
        field_inner = field_item.get("inner", {})
        if "struct_field" in field_inner:
            fty = _parse_type(field_inner["struct_field"])
            if fty:
                fields.append(FieldInfo(
                    name=field_item.get("name", ""),
                    ty=fty,
                    is_public=True,
                ))

    generics = struct_data.get("generics", {})
    type_vars = [
        gp.get("name", "")
        for gp in generics.get("params", [])
        if gp.get("kind", {}).get("type") is not None
    ]

    sigma.add_struct(StructDef(
        name=name,
        full_path=name,
        fields=fields,
        type_vars=type_vars,
    ))


def _parse_impl(item: dict, inner: dict, index: dict, sigma: SigmaC) -> None:
    impl_data = inner.get("impl", {})
    trait_info = impl_data.get("trait")
    for_ty = _parse_type(impl_data.get("for"))

    if trait_info and for_ty:
        trait_name = trait_info.get("name", "")
        sigma.add_impl(ImplFact(ty=for_ty, trait_name=trait_name))

        if trait_name in ("Copy",):
            sigma.copy_clone_facts.append(CopyCloneFact(
                ty=for_ty, is_copy=True, is_clone=True,
            ))
        elif trait_name in ("Clone",):
            sigma.copy_clone_facts.append(CopyCloneFact(
                ty=for_ty, is_copy=False, is_clone=True,
            ))

    impl_type_name = ""
    if for_ty and for_ty.kind == "path":
        impl_type_name = for_ty.name

    for member_id in impl_data.get("items", []):
        member = index.get(str(member_id), {})
        if member.get("visibility") != "public":
            continue
        member_inner = member.get("inner", {})
        if "function" in member_inner:
            method_name = member.get("name", "")
            full_path = f"{impl_type_name}::{method_name}" if impl_type_name else method_name
            member_copy = dict(member)
            member_copy["name"] = full_path
            _parse_function(member_copy, member_inner, sigma)


def _collect_type_universe(sigma: SigmaC) -> None:
    """Collect all ground types mentioned in callables into type_universe."""
    for c in sigma.callables:
        for p in c.params:
            sigma.register_type(p.ty)
        if c.return_type:
            sigma.register_type(c.return_type)
    for s in sigma.structs:
        sigma.register_type(GroundType.path(s.name))
        for f in s.fields:
            sigma.register_type(f.ty)
