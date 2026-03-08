"""
SnippetIR — intermediate representation for generated Rust code snippets.

This is the structured IR that sits between PlanTrace and rendered Rust source.
It supports:
  - import declarations
  - variable declarations (let bindings)
  - nested scopes / blocks (for borrow lifetimes)
  - explicit drop() calls
  - statement lists
  - placeholders / holes (for ExportScaffold mode)
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Optional

from rustsynth.core.types import GroundType, TypeForm, VarId
from rustsynth.core.pcpn import TransitionKind
from rustsynth.core.env import SigmaC, PassingMode, ReturnMode
from rustsynth.ir.plan_trace import PlanTrace, FiringInstance


# ---------------------------------------------------------------------------
# Statement types
# ---------------------------------------------------------------------------

class StmtKind(Enum):
    LetBinding = "let"
    MethodCall = "method_call"
    FnCall = "fn_call"
    Drop = "drop"
    BlockOpen = "block_open"
    BlockClose = "block_close"
    Hole = "hole"
    Comment = "comment"
    Borrow = "borrow"
    Deref = "deref"
    FieldAccess = "field_access"
    Clone = "clone"


class HoleKind(Enum):
    Literal = "literal"
    ConstructorArg = "constructor_arg"
    TraitStub = "trait_stub"
    Assertion = "assertion"


@dataclass
class Hole:
    id: str
    kind: HoleKind
    expected_type: Optional[str] = None
    expected_trait: Optional[str] = None
    available_vars: list[str] = field(default_factory=list)


@dataclass
class Stmt:
    kind: StmtKind
    var_name: Optional[str] = None
    var_type: Optional[str] = None
    expr: str = ""
    is_mut: bool = False
    hole: Optional[Hole] = None
    comment: str = ""


# ---------------------------------------------------------------------------
# SnippetIR
# ---------------------------------------------------------------------------

@dataclass
class SnippetIR:
    """Structured IR for a synthesized Rust code snippet."""
    task_id: str = ""
    imports: list[str] = field(default_factory=list)
    stmts: list[Stmt] = field(default_factory=list)
    holes: list[Hole] = field(default_factory=list)
    crate_name: str = ""

    def add_import(self, path: str) -> None:
        if path not in self.imports:
            self.imports.append(path)

    def add_stmt(self, stmt: Stmt) -> None:
        self.stmts.append(stmt)

    def add_hole(self, hole: Hole) -> None:
        self.holes.append(hole)
        self.stmts.append(Stmt(kind=StmtKind.Hole, hole=hole))


# ---------------------------------------------------------------------------
# Lower PlanTrace -> SnippetIR
# ---------------------------------------------------------------------------

def lower_to_snippet(plan: PlanTrace, sigma: SigmaC) -> SnippetIR:
    """Convert a PlanTrace into a SnippetIR."""
    snippet = SnippetIR(task_id=plan.task_id, crate_name=sigma.crate_name)

    var_names: dict[VarId, str] = {}
    var_types: dict[VarId, GroundType] = {}
    var_forms: dict[VarId, TypeForm] = {}
    name_counter = 0
    used_types: set[str] = set()
    hole_counter = 0

    def fresh_name() -> str:
        nonlocal name_counter
        n = f"v{name_counter}"
        name_counter += 1
        return n

    def vid_name(vid: VarId, ty: GroundType, form: TypeForm) -> str:
        if vid not in var_names:
            var_names[vid] = fresh_name()
            var_types[vid] = ty
            var_forms[vid] = form
        return var_names[vid]

    def type_str(ty: GroundType, form: TypeForm) -> str:
        base = ty.short_name()
        if form == TypeForm.RefShr:
            return f"&{base}"
        if form == TypeForm.RefMut:
            return f"&mut {base}"
        return base

    STDLIB_TYPES = {
        "String", "Vec", "Box", "Rc", "Arc", "HashMap", "HashSet",
        "BTreeMap", "BTreeSet", "Option", "Result", "Cow", "Cell",
        "RefCell", "Mutex", "RwLock", "PathBuf", "Path", "OsString",
        "OsStr", "CString", "CStr", "Duration", "Instant",
    }

    def collect_type(ty: GroundType) -> None:
        if ty.kind == "path" and not ty.is_primitive():
            name = ty.name.rsplit("::", 1)[-1]
            if name not in STDLIB_TYPES:
                used_types.add(name)

    for firing in plan.firings:
        kind = firing.kind

        if kind == TransitionKind.CreatePrimitive:
            if firing.produced_vids:
                _, vid, ty, form = firing.produced_vids[0]
                vn = vid_name(vid, ty, form)
                literal = _default_literal(ty)
                snippet.add_stmt(Stmt(
                    kind=StmtKind.LetBinding,
                    var_name=vn,
                    var_type=type_str(ty, form),
                    expr=literal,
                ))

        elif kind in (TransitionKind.ApiCall, TransitionKind.ConstProducer):
            fn_path = firing.fn_path or firing.transition_name
            fn_path_clean = _clean_fn_path(fn_path)

            args = []
            self_var = None
            callable_item = _find_callable(sigma, fn_path)
            for arg_idx, (pid, vid, ty, form) in enumerate(firing.consumed_vids):
                vn = vid_name(vid, ty, form)
                if callable_item:
                    for p in callable_item.params:
                        if p.is_self:
                            self_var = vn
                            break

                if self_var != vn:
                    if form in (TypeForm.RefShr, TypeForm.RefMut):
                        args.append(vn)
                    else:
                        args.append(vn)

            if _is_method_path(fn_path_clean):
                type_name, method_name = _split_method_path(fn_path_clean)
                collect_type(GroundType.path(type_name))

                if self_var:
                    expr = f"{self_var}.{method_name}({', '.join(args)})"
                else:
                    expr = f"{type_name}::{method_name}({', '.join(args)})"
            else:
                expr = f"{fn_path_clean}({', '.join(args)})"

            if firing.produced_vids:
                _, vid, ty, form = firing.produced_vids[0]
                vn = vid_name(vid, ty, form)
                collect_type(ty)
                needs_mut = _needs_mut_binding(vid, plan, firing.step)
                snippet.add_stmt(Stmt(
                    kind=StmtKind.LetBinding,
                    var_name=vn,
                    var_type=type_str(ty, form),
                    expr=expr,
                    is_mut=needs_mut,
                ))
            else:
                snippet.add_stmt(Stmt(kind=StmtKind.FnCall, expr=f"{expr};"))

        elif kind in (TransitionKind.BorrowShrFirst, TransitionKind.BorrowShrNext):
            if firing.consumed_vids and firing.produced_vids:
                _, src_vid, src_ty, _ = firing.consumed_vids[0]
                src_name = vid_name(src_vid, src_ty, TypeForm.Value)
                for _, vid, ty, form in firing.produced_vids:
                    if form == TypeForm.RefShr:
                        vn = vid_name(vid, ty, form)
                        snippet.add_stmt(Stmt(
                            kind=StmtKind.Borrow,
                            var_name=vn,
                            var_type=type_str(ty, TypeForm.RefShr),
                            expr=f"&{src_name}",
                        ))

        elif kind == TransitionKind.BorrowMut:
            if firing.consumed_vids and firing.produced_vids:
                _, src_vid, src_ty, _ = firing.consumed_vids[0]
                src_name = vid_name(src_vid, src_ty, TypeForm.Value)
                for _, vid, ty, form in firing.produced_vids:
                    if form == TypeForm.RefMut:
                        vn = vid_name(vid, ty, form)
                        snippet.add_stmt(Stmt(
                            kind=StmtKind.Borrow,
                            var_name=vn,
                            var_type=type_str(ty, TypeForm.RefMut),
                            expr=f"&mut {src_name}",
                        ))

        elif kind == TransitionKind.Drop:
            if firing.consumed_vids:
                _, vid, ty, form = firing.consumed_vids[0]
                if vid in var_names:
                    vn = var_names[vid]
                    snippet.add_stmt(Stmt(kind=StmtKind.Drop, expr=f"drop({vn});"))

        elif kind in (TransitionKind.EndBorrowShrKeepFrz, TransitionKind.EndBorrowShrUnfreeze,
                       TransitionKind.EndBorrowMut):
            if firing.consumed_vids:
                for _, vid, ty, form in firing.consumed_vids:
                    if vid in var_names and form in (TypeForm.RefShr, TypeForm.RefMut):
                        vn = var_names[vid]
                        snippet.add_stmt(Stmt(kind=StmtKind.Drop, expr=f"drop({vn});"))

        elif kind == TransitionKind.CopyUse:
            if firing.consumed_vids and firing.produced_vids:
                _, src_vid, src_ty, _ = firing.consumed_vids[0]
                src_name = vid_name(src_vid, src_ty, TypeForm.Value)
                _, vid, ty, form = firing.produced_vids[0]
                vn = vid_name(vid, ty, form)
                snippet.add_stmt(Stmt(
                    kind=StmtKind.LetBinding,
                    var_name=vn, var_type=type_str(ty, form),
                    expr=src_name,
                ))

        elif kind == TransitionKind.DerefCopy:
            if firing.consumed_vids and firing.produced_vids:
                _, src_vid, src_ty, _ = firing.consumed_vids[0]
                src_name = vid_name(src_vid, src_ty, TypeForm.RefShr)
                _, vid, ty, form = firing.produced_vids[0]
                vn = vid_name(vid, ty, form)
                snippet.add_stmt(Stmt(
                    kind=StmtKind.Deref,
                    var_name=vn, var_type=type_str(ty, form),
                    expr=f"*{src_name}",
                ))

        elif kind == TransitionKind.DupClone:
            if firing.consumed_vids and firing.produced_vids:
                _, src_vid, src_ty, _ = firing.consumed_vids[0]
                src_name = vid_name(src_vid, src_ty, TypeForm.RefShr)
                _, vid, ty, form = firing.produced_vids[0]
                vn = vid_name(vid, ty, form)
                snippet.add_stmt(Stmt(
                    kind=StmtKind.Clone,
                    var_name=vn, var_type=type_str(ty, form),
                    expr=f"{src_name}.clone()",
                ))

        elif kind == TransitionKind.ProjMove:
            if firing.consumed_vids and firing.produced_vids:
                _, src_vid, src_ty, _ = firing.consumed_vids[0]
                src_name = vid_name(src_vid, src_ty, TypeForm.Value)
                _, vid, ty, form = firing.produced_vids[0]
                vn = vid_name(vid, ty, form)
                fname = firing.field_name or "field"
                snippet.add_stmt(Stmt(
                    kind=StmtKind.FieldAccess,
                    var_name=vn, var_type=type_str(ty, form),
                    expr=f"{src_name}.{fname}",
                ))

        elif kind == TransitionKind.ProjRef:
            if firing.consumed_vids and firing.produced_vids:
                _, src_vid, src_ty, src_form = firing.consumed_vids[0]
                src_name = vid_name(src_vid, src_ty, src_form)
                _, vid, ty, form = firing.produced_vids[0]
                vn = vid_name(vid, ty, form)
                fname = firing.field_name or "field"
                prefix = "&" if form == TypeForm.RefShr else "&mut "
                snippet.add_stmt(Stmt(
                    kind=StmtKind.FieldAccess,
                    var_name=vn, var_type=type_str(ty, form),
                    expr=f"{prefix}{src_name}.{fname}",
                ))

        elif kind == TransitionKind.Reborrow:
            if firing.consumed_vids and firing.produced_vids:
                _, src_vid, src_ty, _ = firing.consumed_vids[0]
                src_name = vid_name(src_vid, src_ty, TypeForm.RefMut)
                _, vid, ty, form = firing.produced_vids[0]
                vn = vid_name(vid, ty, form)
                snippet.add_stmt(Stmt(
                    kind=StmtKind.Borrow,
                    var_name=vn, var_type=type_str(ty, TypeForm.RefShr),
                    expr=f"&*{src_name}",
                ))

    if used_types and sigma.crate_name:
        type_list = ", ".join(sorted(used_types))
        snippet.add_import(f"use {sigma.crate_name}::{{{type_list}}};")
    elif used_types:
        for t in sorted(used_types):
            snippet.add_import(f"use crate::{t};")

    fn_imports = set()
    for firing in plan.firings:
        if firing.fn_path and not _is_method_path(firing.fn_path):
            clean = _clean_fn_path(firing.fn_path)
            if not clean.startswith("__literal_"):
                fn_imports.add(clean)
    if fn_imports and sigma.crate_name:
        for fn_name in sorted(fn_imports):
            snippet.add_import(f"use {sigma.crate_name}::{fn_name};")

    return snippet


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _default_literal(ty: GroundType) -> str:
    literals = {
        "i32": "0i32", "u32": "0u32", "i64": "0i64", "u64": "0u64",
        "bool": "false", "usize": "0usize", "f32": "0.0f32", "f64": "0.0f64",
        "i8": "0i8", "u8": "0u8", "i16": "0i16", "u16": "0u16",
        "i128": "0i128", "u128": "0u128", "isize": "0isize",
        "char": "'a'", "str": '"hello"',
    }
    if ty.kind == "primitive":
        return literals.get(ty.name, "Default::default()")
    if ty.kind == "unit":
        return "()"
    name = ty.name.rsplit("::", 1)[-1]
    if name == "String":
        return 'String::from("hello")'
    return "Default::default()"


def _is_method_path(path: str) -> bool:
    clean = path.split("<")[0]
    return "::" in clean


def _split_method_path(path: str) -> tuple[str, str]:
    clean = path.split("<")[0]
    parts = clean.rsplit("::", 1)
    return parts[0], parts[1]


def _clean_fn_path(path: str) -> str:
    if path.startswith("__literal_"):
        return path
    return path.split("<")[0]


def _find_callable(sigma: SigmaC, fn_path: str) -> Optional:
    clean = fn_path.split("<")[0]
    for c in sigma.callables:
        if c.path == clean or c.path.split("<")[0] == clean:
            return c
    return None


def _needs_mut_binding(vid: VarId, plan: PlanTrace, current_step: int) -> bool:
    """Check if this variable is later consumed by a &mut borrow or &mut self call."""
    for f in plan.firings[current_step + 1:]:
        for _, cvid, _, cform in f.consumed_vids:
            if cvid == vid and cform == TypeForm.RefMut:
                return True
        if f.kind == TransitionKind.BorrowMut:
            for _, cvid, _, _ in f.consumed_vids:
                if cvid == vid:
                    return True
    return False
