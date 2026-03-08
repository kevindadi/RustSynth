"""
Core type system for the PCPN model.

Paper mapping:
  - GroundType          ≈  hat{Ty}  (finite ground type universe)
  - TypeForm            ≈  {T, &T, &mut T}
  - Capability          ≈  {own, frz, blk}
  - PlaceKey            ≈  (base_type, form, cap) — 9 places per type
  - Token               ≈  Col (color set element)
  - StackFrame          ≈  Gamma (pushdown stack alphabet)
  - BorrowStack         ≈  S component of Cfg = <M, S>
  - Marking             ≈  M component of Cfg
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum, auto
from typing import Optional


# ---------------------------------------------------------------------------
# Ground types  —  hat{Ty}
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class GroundType:
    """Finite ground type — either a primitive, a nominal path (possibly with
    ground type args), a tuple, or unit."""
    kind: str  # "primitive" | "path" | "tuple" | "unit"
    name: str = ""
    args: tuple[GroundType, ...] = ()

    # convenience constructors
    @staticmethod
    def primitive(name: str) -> GroundType:
        return GroundType(kind="primitive", name=name)

    @staticmethod
    def path(name: str, args: tuple[GroundType, ...] = ()) -> GroundType:
        return GroundType(kind="path", name=name, args=args)

    @staticmethod
    def tuple_(elems: tuple[GroundType, ...]) -> GroundType:
        if len(elems) == 0:
            return UNIT
        return GroundType(kind="tuple", args=elems)

    @staticmethod
    def unit() -> GroundType:
        return UNIT

    def short_name(self) -> str:
        if self.kind == "primitive":
            return self.name
        if self.kind == "unit":
            return "()"
        if self.kind == "path":
            base = self.name.rsplit("::", 1)[-1]
            if self.args:
                inner = ", ".join(a.short_name() for a in self.args)
                return f"{base}<{inner}>"
            return base
        if self.kind == "tuple":
            inner = ", ".join(a.short_name() for a in self.args)
            return f"({inner})"
        return "?"

    def full_name(self) -> str:
        if self.kind == "primitive":
            return self.name
        if self.kind == "unit":
            return "()"
        if self.kind == "path":
            if self.args:
                inner = ", ".join(a.full_name() for a in self.args)
                return f"{self.name}<{inner}>"
            return self.name
        if self.kind == "tuple":
            inner = ", ".join(a.full_name() for a in self.args)
            return f"({inner})"
        return "?"

    def is_primitive(self) -> bool:
        return self.kind == "primitive" or self.kind == "unit"

    def is_copy(self) -> bool:
        if self.kind in ("primitive", "unit"):
            return True
        if self.kind == "tuple":
            return all(a.is_copy() for a in self.args)
        return False

    def __str__(self) -> str:
        return self.short_name()

    def __repr__(self) -> str:
        return f"GroundType({self.short_name()!r})"


UNIT = GroundType(kind="unit")

PRIMITIVES = [
    "bool", "char", "u8", "u16", "u32", "u64", "u128", "usize",
    "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64",
]


# ---------------------------------------------------------------------------
# TypeScheme  — polymorphic type before monomorphization
# ---------------------------------------------------------------------------

@dataclass
class TypeScheme:
    base: GroundType
    type_vars: list[str] = field(default_factory=list)
    bounds: list[tuple[str, list[str]]] = field(default_factory=list)

    def is_ground(self) -> bool:
        return len(self.type_vars) == 0

    @staticmethod
    def ground(ty: GroundType) -> TypeScheme:
        return TypeScheme(base=ty)


# ---------------------------------------------------------------------------
# 9-place model components
# ---------------------------------------------------------------------------

class TypeForm(Enum):
    Value = "T"
    RefShr = "&T"
    RefMut = "&mut T"


class Capability(Enum):
    Own = "own"
    Frz = "frz"
    Blk = "blk"


@dataclass(frozen=True)
class PlaceKey:
    """Unique key for a place in the 9-place model: (base_type, form, cap)."""
    base_type: GroundType
    form: TypeForm
    cap: Capability


@dataclass
class Place:
    id: int
    base_type: GroundType
    form: TypeForm
    cap: Capability
    budget: int = 3

    @property
    def key(self) -> PlaceKey:
        return PlaceKey(self.base_type, self.form, self.cap)

    def __str__(self) -> str:
        return f"P({self.base_type.short_name()},{self.form.value},{self.cap.value})"


# ---------------------------------------------------------------------------
# Token  (color element)
# ---------------------------------------------------------------------------

VarId = int
RegionLabel = int


@dataclass
class Token:
    vid: VarId
    ty: GroundType
    form: TypeForm
    regions: list[RegionLabel] = field(default_factory=list)
    borrowed_from: Optional[VarId] = None

    def clone(self) -> Token:
        return Token(
            vid=self.vid,
            ty=self.ty,
            form=self.form,
            regions=list(self.regions),
            borrowed_from=self.borrowed_from,
        )


# ---------------------------------------------------------------------------
# Borrow stack  — pushdown component Gamma
# ---------------------------------------------------------------------------

class StackFrameKind(Enum):
    Freeze = auto()
    Shr = auto()
    Mut = auto()


@dataclass
class StackFrame:
    kind: StackFrameKind
    owner_vid: VarId
    ref_vid: Optional[VarId] = None
    region: Optional[RegionLabel] = None
    base_type: Optional[GroundType] = None


@dataclass
class BorrowStack:
    frames: list[StackFrame] = field(default_factory=list)

    def push(self, frame: StackFrame) -> None:
        self.frames.append(frame)

    def pop(self) -> Optional[StackFrame]:
        return self.frames.pop() if self.frames else None

    def peek(self) -> Optional[StackFrame]:
        return self.frames[-1] if self.frames else None

    def depth(self) -> int:
        return len(self.frames)

    def clone(self) -> BorrowStack:
        return BorrowStack(frames=[
            StackFrame(
                kind=f.kind, owner_vid=f.owner_vid,
                ref_vid=f.ref_vid, region=f.region,
                base_type=f.base_type,
            )
            for f in self.frames
        ])


# ---------------------------------------------------------------------------
# Marking  — multiset over places
# ---------------------------------------------------------------------------

@dataclass
class Marking:
    tokens: dict[int, list[Token]] = field(default_factory=dict)

    def add(self, place_id: int, token: Token) -> None:
        self.tokens.setdefault(place_id, []).append(token)

    def remove(self, place_id: int, vid: VarId) -> Optional[Token]:
        toks = self.tokens.get(place_id, [])
        for i, t in enumerate(toks):
            if t.vid == vid:
                return toks.pop(i)
        return None

    def get_tokens(self, place_id: int) -> list[Token]:
        return self.tokens.get(place_id, [])

    def count(self, place_id: int) -> int:
        return len(self.tokens.get(place_id, []))

    def all_tokens(self) -> list[tuple[int, Token]]:
        result = []
        for pid, toks in sorted(self.tokens.items()):
            for t in toks:
                result.append((pid, t))
        return result

    def clone(self) -> Marking:
        m = Marking()
        for pid, toks in self.tokens.items():
            m.tokens[pid] = [t.clone() for t in toks]
        return m


# ---------------------------------------------------------------------------
# Fields
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class FieldInfo:
    name: str
    ty: GroundType
    is_public: bool = True
