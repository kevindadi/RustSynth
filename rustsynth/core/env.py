"""
Compiler-extracted environment Sigma(C).

Paper mapping:
  - SigmaC              ≈  Sigma(C)  — the compiler-extracted environment
  - CallableItem        ≈  function / method signatures in Sigma(C)
  - ImplFact            ≈  impl Trait for Type facts
  - AssocFact           ≈  associated type equalities <T as Trait>::Assoc = U
  - FieldDef            ≈  visible struct field definitions
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Optional

from rustsynth.core.types import GroundType, TypeScheme, FieldInfo


# ---------------------------------------------------------------------------
# Passing mode for parameters
# ---------------------------------------------------------------------------

class PassingMode(Enum):
    Move = "move"
    Copy = "copy"
    BorrowShr = "borrow_shr"
    BorrowMut = "borrow_mut"


class ReturnMode(Enum):
    Owned = "owned"
    BorrowShr = "borrow_shr"
    BorrowMut = "borrow_mut"


# ---------------------------------------------------------------------------
# Callable item (function or method)
# ---------------------------------------------------------------------------

@dataclass
class ParamInfo:
    name: str
    ty: GroundType
    passing: PassingMode
    is_self: bool = False


@dataclass
class CallableItem:
    """A public function or method extracted from the crate API."""
    path: str                         # e.g. "Counter::get" or "make_pair"
    params: list[ParamInfo] = field(default_factory=list)
    return_type: Optional[GroundType] = None
    return_mode: ReturnMode = ReturnMode.Owned
    type_vars: list[str] = field(default_factory=list)
    bounds: list[tuple[str, list[str]]] = field(default_factory=list)
    lifetime_params: list[str] = field(default_factory=list)
    is_constructor: bool = False
    is_const: bool = False

    @property
    def is_method(self) -> bool:
        return any(p.is_self for p in self.params)

    @property
    def self_param(self) -> Optional[ParamInfo]:
        for p in self.params:
            if p.is_self:
                return p
        return None

    @property
    def non_self_params(self) -> list[ParamInfo]:
        return [p for p in self.params if not p.is_self]

    @property
    def type_name(self) -> Optional[str]:
        if "::" in self.path:
            return self.path.rsplit("::", 1)[0]
        return None

    @property
    def method_name(self) -> Optional[str]:
        if "::" in self.path:
            return self.path.rsplit("::", 1)[1]
        return None

    def to_dict(self) -> dict:
        return {
            "path": self.path,
            "params": [
                {"name": p.name, "ty": p.ty.full_name(),
                 "passing": p.passing.value, "is_self": p.is_self}
                for p in self.params
            ],
            "return_type": self.return_type.full_name() if self.return_type else None,
            "return_mode": self.return_mode.value,
            "type_vars": self.type_vars,
            "bounds": self.bounds,
            "is_constructor": self.is_constructor,
        }


# ---------------------------------------------------------------------------
# Trait / associated type facts
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class ImplFact:
    """Records that `ty` implements `trait_name`."""
    ty: GroundType
    trait_name: str


@dataclass(frozen=True)
class AssocFact:
    """<ty as trait_name>::assoc_name = assoc_ty"""
    ty: GroundType
    trait_name: str
    assoc_name: str
    assoc_ty: GroundType


@dataclass(frozen=True)
class CopyCloneFact:
    ty: GroundType
    is_copy: bool
    is_clone: bool


# ---------------------------------------------------------------------------
# Struct definition
# ---------------------------------------------------------------------------

@dataclass
class StructDef:
    name: str
    full_path: str
    fields: list[FieldInfo] = field(default_factory=list)
    type_vars: list[str] = field(default_factory=list)
    is_copy: bool = False
    is_clone: bool = False


# ---------------------------------------------------------------------------
# Sigma(C) — the compiler-extracted environment
# ---------------------------------------------------------------------------

@dataclass
class SigmaC:
    """The compiler-extracted environment Sigma(C).

    Contains all public API surface information needed to construct the PCPN.
    """
    crate_name: str = ""
    callables: list[CallableItem] = field(default_factory=list)
    structs: list[StructDef] = field(default_factory=list)
    impl_facts: list[ImplFact] = field(default_factory=list)
    assoc_facts: list[AssocFact] = field(default_factory=list)
    copy_clone_facts: list[CopyCloneFact] = field(default_factory=list)
    type_universe: list[GroundType] = field(default_factory=list)

    def add_callable(self, item: CallableItem) -> None:
        self.callables.append(item)

    def add_struct(self, s: StructDef) -> None:
        self.structs.append(s)

    def add_impl(self, fact: ImplFact) -> None:
        self.impl_facts.append(fact)

    def add_assoc(self, fact: AssocFact) -> None:
        self.assoc_facts.append(fact)

    def register_type(self, ty: GroundType) -> None:
        if ty not in self.type_universe:
            self.type_universe.append(ty)

    def get_struct(self, name: str) -> Optional[StructDef]:
        for s in self.structs:
            if s.name == name or s.full_path == name:
                return s
        return None

    def implements_trait(self, ty: GroundType, trait_name: str) -> bool:
        for f in self.impl_facts:
            if f.ty == ty and f.trait_name == trait_name:
                return True
        if trait_name == "Copy":
            return ty.is_copy() or any(
                cf.ty == ty and cf.is_copy for cf in self.copy_clone_facts
            )
        if trait_name == "Clone":
            return ty.is_copy() or any(
                cf.ty == ty and cf.is_clone for cf in self.copy_clone_facts
            )
        return False

    def resolve_assoc(self, ty: GroundType, trait_name: str, assoc_name: str) -> Optional[GroundType]:
        for f in self.assoc_facts:
            if f.ty == ty and f.trait_name == trait_name and f.assoc_name == assoc_name:
                return f.assoc_ty
        return None

    def is_type_copy(self, ty: GroundType) -> bool:
        return self.implements_trait(ty, "Copy")

    def is_type_clone(self, ty: GroundType) -> bool:
        return self.implements_trait(ty, "Clone")

    def visible_fields(self, ty: GroundType) -> list[FieldInfo]:
        s = self.get_struct(ty.name if ty.kind == "path" else "")
        if s is None:
            return []
        return [f for f in s.fields if f.is_public]

    def to_dict(self) -> dict:
        return {
            "crate_name": self.crate_name,
            "callables": [c.to_dict() for c in self.callables],
            "type_universe": [t.full_name() for t in self.type_universe],
            "structs": [
                {"name": s.name, "fields": [
                    {"name": f.name, "ty": f.ty.full_name()} for f in s.fields
                ]}
                for s in self.structs
            ],
            "impl_facts": [
                {"ty": f.ty.full_name(), "trait": f.trait_name}
                for f in self.impl_facts
            ],
            "assoc_facts": [
                {"ty": f.ty.full_name(), "trait": f.trait_name,
                 "assoc": f.assoc_name, "assoc_ty": f.assoc_ty.full_name()}
                for f in self.assoc_facts
            ],
        }
