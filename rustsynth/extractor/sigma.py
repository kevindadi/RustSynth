"""Assemble Sigma(C) from a Rust crate.

Tries rustdoc JSON first; falls back to syn-based source parsing.
"""

from __future__ import annotations

import logging
from pathlib import Path

from rustsynth.core.types import GroundType, PRIMITIVES
from rustsynth.core.env import SigmaC, CallableItem, ParamInfo, PassingMode, ReturnMode
from rustsynth.extractor.cargo_meta import find_lib_src
from rustsynth.extractor.rustdoc_parser import generate_rustdoc_json, parse_rustdoc_json
from rustsynth.extractor.syn_fallback import parse_rust_source

logger = logging.getLogger(__name__)


def build_sigma(crate_path: Path) -> SigmaC:
    """Build Sigma(C) from a crate, trying rustdoc JSON then fallback."""

    json_path = generate_rustdoc_json(crate_path)
    if json_path is not None:
        logger.info("Using rustdoc JSON: %s", json_path)
        sigma = parse_rustdoc_json(json_path)
    else:
        logger.info("Falling back to source parsing")
        lib_src = find_lib_src(crate_path)
        if lib_src is None:
            raise FileNotFoundError(f"No lib.rs found in {crate_path}")
        source = lib_src.read_text()
        crate_name = crate_path.name.replace("-", "_")
        sigma = parse_rust_source(source, crate_name)

    _inject_primitive_providers(sigma)
    return sigma


_UNSIZED_TYPES = {"str"}


def _inject_primitive_providers(sigma: SigmaC) -> None:
    """Add 0-ary literal providers for primitive types used in the API."""
    used_prims = set()
    for c in sigma.callables:
        for p in c.params:
            if p.ty.is_primitive() and p.ty.kind == "primitive":
                used_prims.add(p.ty.name)
        if c.return_type and c.return_type.is_primitive() and c.return_type.kind == "primitive":
            used_prims.add(c.return_type.name)

    used_prims.add("i32")
    used_prims.add("bool")

    for prim_name in sorted(used_prims):
        if prim_name in _UNSIZED_TYPES:
            ty = GroundType.primitive(prim_name)
            provider_path = f"__literal_{prim_name}_ref"
            if not any(c.path == provider_path for c in sigma.callables):
                sigma.add_callable(CallableItem(
                    path=provider_path,
                    params=[],
                    return_type=ty,
                    return_mode=ReturnMode.BorrowShr,
                    is_constructor=True,
                    is_const=True,
                ))
            sigma.register_type(ty)
            continue
        ty = GroundType.primitive(prim_name)
        provider_path = f"__literal_{prim_name}"
        if not any(c.path == provider_path for c in sigma.callables):
            sigma.add_callable(CallableItem(
                path=provider_path,
                params=[],
                return_type=ty,
                return_mode=ReturnMode.Owned,
                is_constructor=True,
                is_const=True,
            ))
        sigma.register_type(ty)
