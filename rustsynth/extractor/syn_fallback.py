"""Conservative fallback extractor using regex-based parsing.

When rustdoc JSON is unavailable, this module parses Rust source files
to extract public function signatures, struct definitions, impl blocks,
and trait bounds.  This is intentionally conservative — it may miss
complex signatures but will not produce false positives.
"""

from __future__ import annotations

import re
import logging
from pathlib import Path
from typing import Optional

from rustsynth.core.types import GroundType, FieldInfo
from rustsynth.core.env import (
    CallableItem, ParamInfo, PassingMode, ReturnMode,
    StructDef, ImplFact, CopyCloneFact, SigmaC,
)

logger = logging.getLogger(__name__)

_FN_RE = re.compile(
    r"pub\s+(?:const\s+)?fn\s+(\w+)"
    r"\s*(?:<([^>]*)>)?"
    r"\s*\(([^)]*)\)"
    r"(?:\s*->\s*(.+?))?"
    r"\s*(?:where\s+(.+?))?\s*\{",
    re.DOTALL,
)

_STRUCT_RE = re.compile(
    r"pub\s+struct\s+(\w+)\s*(?:<([^>]*)>)?\s*\{([^}]*)\}",
    re.DOTALL,
)

_IMPL_RE = re.compile(
    r"impl\s*(?:<([^>]*)>)?\s*(?:(\w+)\s+for\s+)?(\w+)(?:<[^>]*>)?\s*\{",
    re.DOTALL,
)

_DERIVE_RE = re.compile(
    r"#\[derive\(([^)]+)\)\]",
)


def parse_rust_source(source: str, crate_name: str = "") -> SigmaC:
    """Parse Rust source code and extract a conservative SigmaC."""
    sigma = SigmaC(crate_name=crate_name)

    derives_by_struct: dict[str, list[str]] = {}
    lines = source.split("\n")
    for i, line in enumerate(lines):
        dm = _DERIVE_RE.search(line)
        if dm:
            traits = [t.strip() for t in dm.group(1).split(",")]
            for j in range(i + 1, min(i + 5, len(lines))):
                sm = re.match(r"\s*pub\s+struct\s+(\w+)", lines[j])
                if sm:
                    derives_by_struct[sm.group(1)] = traits
                    break

    current_impl_type = None
    current_impl_trait = None

    for m in _IMPL_RE.finditer(source):
        trait_name = m.group(2)
        type_name = m.group(3)

        if trait_name:
            ty = GroundType.path(type_name)
            sigma.add_impl(ImplFact(ty=ty, trait_name=trait_name))
            if trait_name == "Copy":
                sigma.copy_clone_facts.append(CopyCloneFact(ty=ty, is_copy=True, is_clone=True))
            elif trait_name == "Clone":
                sigma.copy_clone_facts.append(CopyCloneFact(ty=ty, is_copy=False, is_clone=True))

    for struct_name, traits in derives_by_struct.items():
        ty = GroundType.path(struct_name)
        if "Copy" in traits:
            sigma.add_impl(ImplFact(ty=ty, trait_name="Copy"))
            sigma.copy_clone_facts.append(CopyCloneFact(ty=ty, is_copy=True, is_clone=True))
        if "Clone" in traits:
            sigma.add_impl(ImplFact(ty=ty, trait_name="Clone"))
            if not any(cf.ty == ty for cf in sigma.copy_clone_facts):
                sigma.copy_clone_facts.append(CopyCloneFact(ty=ty, is_copy=False, is_clone=True))

    for m in _STRUCT_RE.finditer(source):
        name = m.group(1)
        type_params = _parse_type_params(m.group(2))
        body = m.group(3)
        fields = _parse_struct_fields(body)
        is_copy = name in derives_by_struct and "Copy" in derives_by_struct[name]
        is_clone = name in derives_by_struct and "Clone" in derives_by_struct.get(name, [])
        sigma.add_struct(StructDef(
            name=name, full_path=name, fields=fields,
            type_vars=type_params, is_copy=is_copy, is_clone=is_clone,
        ))

    impl_blocks = _find_impl_blocks(source)

    for fn_match in _FN_RE.finditer(source):
        fn_name = fn_match.group(1)
        generics_str = fn_match.group(2)
        params_str = fn_match.group(3)
        ret_str = fn_match.group(4)

        offset = fn_match.start()
        impl_type = _find_enclosing_impl(offset, impl_blocks)

        full_path = f"{impl_type}::{fn_name}" if impl_type else fn_name

        type_vars = _parse_type_params(generics_str)
        params = _parse_params(params_str, impl_type)
        ret_ty, ret_mode = _parse_return_type(ret_str)

        bounds = _parse_where_bounds(fn_match.group(5), generics_str)

        if ret_ty and ret_ty.kind == "path" and ret_ty.name == "Self" and impl_type:
            ret_ty = GroundType.path(impl_type)
        for p in params:
            if p.ty.kind == "path" and p.ty.name == "Self" and impl_type:
                p.ty = GroundType.path(impl_type)

        is_ctor = not any(p.is_self for p in params) and ret_ty is not None
        is_const = "const" in source[max(0, fn_match.start() - 20):fn_match.start() + 10]

        sigma.add_callable(CallableItem(
            path=full_path,
            params=params,
            return_type=ret_ty,
            return_mode=ret_mode,
            type_vars=type_vars,
            bounds=bounds,
            is_constructor=is_ctor,
            is_const=is_const,
        ))

    _collect_types(sigma)
    return sigma


def _parse_type_params(s: Optional[str]) -> list[str]:
    if not s:
        return []
    parts = s.split(",")
    result = []
    for p in parts:
        p = p.strip()
        name = p.split(":")[0].strip()
        if name and name[0].isupper() and len(name) <= 3:
            result.append(name)
    return result


def _parse_struct_fields(body: str) -> list[FieldInfo]:
    fields = []
    for line in body.split(","):
        line = line.strip()
        m = re.match(r"pub\s+(\w+)\s*:\s*(.+)", line)
        if m:
            fname = m.group(1)
            fty_str = m.group(2).strip()
            fty = _str_to_ground_type(fty_str)
            fields.append(FieldInfo(name=fname, ty=fty, is_public=True))
    return fields


def _parse_params(params_str: str, impl_type: Optional[str]) -> list[ParamInfo]:
    params = []
    if not params_str.strip():
        return params

    for part in _split_params(params_str):
        part = part.strip()
        if not part:
            continue

        if part in ("self",):
            ty = GroundType.path(impl_type) if impl_type else GroundType.unit()
            params.append(ParamInfo(name="self", ty=ty, passing=PassingMode.Move, is_self=True))
        elif part.startswith("&mut self"):
            ty = GroundType.path(impl_type) if impl_type else GroundType.unit()
            params.append(ParamInfo(name="self", ty=ty, passing=PassingMode.BorrowMut, is_self=True))
        elif part.startswith("&self"):
            ty = GroundType.path(impl_type) if impl_type else GroundType.unit()
            params.append(ParamInfo(name="self", ty=ty, passing=PassingMode.BorrowShr, is_self=True))
        else:
            m = re.match(r"(\w+)\s*:\s*(.+)", part)
            if m:
                pname = m.group(1)
                ty_str = m.group(2).strip()
                passing = PassingMode.Move
                if ty_str.startswith("&mut "):
                    passing = PassingMode.BorrowMut
                    ty_str = ty_str[5:]
                elif ty_str.startswith("&"):
                    passing = PassingMode.BorrowShr
                    ty_str = ty_str[1:].strip()
                ty = _str_to_ground_type(ty_str)
                params.append(ParamInfo(name=pname, ty=ty, passing=passing, is_self=False))
    return params


def _split_params(s: str) -> list[str]:
    """Split parameter string respecting angle brackets."""
    depth = 0
    parts = []
    current = []
    for ch in s:
        if ch == '<':
            depth += 1
            current.append(ch)
        elif ch == '>':
            depth -= 1
            current.append(ch)
        elif ch == ',' and depth == 0:
            parts.append("".join(current))
            current = []
        else:
            current.append(ch)
    if current:
        parts.append("".join(current))
    return parts


def _parse_return_type(ret_str: Optional[str]) -> tuple[Optional[GroundType], ReturnMode]:
    if not ret_str:
        return None, ReturnMode.Owned
    ret_str = ret_str.strip()
    if ret_str.startswith("&mut "):
        return _str_to_ground_type(ret_str[5:].strip()), ReturnMode.BorrowMut
    if ret_str.startswith("&"):
        return _str_to_ground_type(ret_str[1:].strip()), ReturnMode.BorrowShr
    return _str_to_ground_type(ret_str), ReturnMode.Owned


def _str_to_ground_type(s: str) -> GroundType:
    s = s.strip().rstrip(",").strip()
    if s in ("()", ""):
        return GroundType.unit()
    if s in ("bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize",
             "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64"):
        return GroundType.primitive(s)
    if s == "String":
        return GroundType.path("String")
    if s.startswith("(") and s.endswith(")"):
        inner = s[1:-1]
        elems = [_str_to_ground_type(e.strip()) for e in _split_params(inner) if e.strip()]
        return GroundType.tuple_(tuple(elems))

    m = re.match(r"(\w+)\s*<(.+)>$", s)
    if m:
        name = m.group(1)
        args_str = m.group(2)
        args = [_str_to_ground_type(a.strip()) for a in _split_params(args_str) if a.strip()]
        return GroundType.path(name, tuple(args))

    if s == "Self":
        return GroundType.path("Self")

    return GroundType.path(s)


def _parse_where_bounds(where_str: Optional[str], generics_str: Optional[str]) -> list[tuple[str, list[str]]]:
    bounds: list[tuple[str, list[str]]] = []
    if generics_str:
        for part in generics_str.split(","):
            part = part.strip()
            if ":" in part:
                tv, bstr = part.split(":", 1)
                tv = tv.strip()
                traits = [b.strip() for b in bstr.split("+") if b.strip()]
                if traits:
                    bounds.append((tv, traits))
    if where_str:
        for clause in where_str.split(","):
            clause = clause.strip()
            if ":" in clause:
                tv, bstr = clause.split(":", 1)
                tv = tv.strip()
                traits = [b.strip() for b in bstr.split("+") if b.strip()]
                if traits:
                    bounds.append((tv, traits))
    return bounds


def _find_impl_blocks(source: str) -> list[tuple[int, int, str]]:
    """Find impl blocks and their byte ranges. Returns (start, end, type_name)."""
    blocks = []
    for m in _IMPL_RE.finditer(source):
        type_name = m.group(3)
        start = m.start()
        depth = 0
        end = m.end()
        for i in range(m.end() - 1, len(source)):
            if source[i] == '{':
                depth += 1
            elif source[i] == '}':
                depth -= 1
                if depth == 0:
                    end = i + 1
                    break
        blocks.append((start, end, type_name))
    return blocks


def _find_enclosing_impl(offset: int, blocks: list[tuple[int, int, str]]) -> Optional[str]:
    for start, end, type_name in blocks:
        if start <= offset < end:
            return type_name
    return None


def _collect_types(sigma: SigmaC) -> None:
    for c in sigma.callables:
        for p in c.params:
            sigma.register_type(p.ty)
        if c.return_type:
            sigma.register_type(c.return_type)
    for s in sigma.structs:
        sigma.register_type(GroundType.path(s.name))
        for f in s.fields:
            sigma.register_type(f.ty)
