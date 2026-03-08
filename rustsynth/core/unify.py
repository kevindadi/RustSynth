"""
Type unification and instantiation.

Paper mapping:
  - Substitution    ≈  sigma in unify judgment
  - unify()         ≈  unification of type scheme against ground type
  - join()          ≈  substitution join (merge two consistent substitutions)
  - complete()      ≈  completion of unbound type vars over finite universe
  - TypeUniverse    ≈  hat{Ty}  (the finite ground type universe)
  - enumerate_instantiations ≈ Cartesian product of candidates per type var
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional

from rustsynth.core.types import GroundType


@dataclass
class Substitution:
    type_vars: dict[str, GroundType] = field(default_factory=dict)
    region_vars: dict[str, int] = field(default_factory=dict)

    def bind_type(self, var: str, ty: GroundType) -> None:
        self.type_vars[var] = ty

    def get_type(self, var: str) -> Optional[GroundType]:
        return self.type_vars.get(var)

    def apply(self, ty: GroundType) -> Optional[GroundType]:
        return self._apply_inner(ty)

    def _apply_inner(self, ty: GroundType) -> Optional[GroundType]:
        if ty.kind in ("primitive", "unit"):
            return ty
        if ty.kind == "path":
            if not ty.args and _is_type_var(ty.name):
                bound = self.type_vars.get(ty.name)
                return bound if bound else ty
            new_args = []
            for a in ty.args:
                resolved = self._apply_inner(a)
                if resolved is None:
                    return None
                new_args.append(resolved)
            return GroundType.path(ty.name, tuple(new_args))
        if ty.kind == "tuple":
            new_elems = []
            for a in ty.args:
                resolved = self._apply_inner(a)
                if resolved is None:
                    return None
                new_elems.append(resolved)
            return GroundType.tuple_(tuple(new_elems))
        return ty

    @staticmethod
    def unify(scheme_ty: GroundType, ground_ty: GroundType) -> Optional[Substitution]:
        subst = Substitution()
        if _unify_inner(scheme_ty, ground_ty, subst):
            return subst
        return None

    def join(self, other: Substitution) -> Optional[Substitution]:
        result = Substitution(
            type_vars=dict(self.type_vars),
            region_vars=dict(self.region_vars),
        )
        for var, ty in other.type_vars.items():
            if var in result.type_vars:
                if result.type_vars[var] != ty:
                    return None
            else:
                result.type_vars[var] = ty
        for var, label in other.region_vars.items():
            if var in result.region_vars:
                if result.region_vars[var] != label:
                    return None
            else:
                result.region_vars[var] = label
        return result

    def is_complete(self, type_vars: list[str]) -> bool:
        return all(v in self.type_vars for v in type_vars)

    def to_dict(self) -> dict[str, GroundType]:
        return dict(self.type_vars)


def _is_type_var(name: str) -> bool:
    return len(name) <= 2 and name[0].isupper()


def _unify_inner(scheme: GroundType, ground: GroundType, subst: Substitution) -> bool:
    if scheme.kind == "primitive" and ground.kind == "primitive":
        return scheme.name == ground.name
    if scheme.kind == "unit" and ground.kind == "unit":
        return True

    if scheme.kind == "path" and _is_type_var(scheme.name) and not scheme.args:
        existing = subst.type_vars.get(scheme.name)
        if existing is not None:
            return existing == ground
        subst.type_vars[scheme.name] = ground
        return True

    if scheme.kind == "path" and ground.kind == "path":
        if not _names_match(scheme.name, ground.name):
            return False
        if len(scheme.args) != len(ground.args):
            return False
        for a, b in zip(scheme.args, ground.args):
            if not _unify_inner(a, b, subst):
                return False
        return True

    if scheme.kind == "tuple" and ground.kind == "tuple":
        if len(scheme.args) != len(ground.args):
            return False
        for a, b in zip(scheme.args, ground.args):
            if not _unify_inner(a, b, subst):
                return False
        return True

    return False


def _names_match(n1: str, n2: str) -> bool:
    if n1 == n2:
        return True
    return n1.rsplit("::", 1)[-1] == n2.rsplit("::", 1)[-1]


# ---------------------------------------------------------------------------
# TypeUniverse
# ---------------------------------------------------------------------------

@dataclass
class TypeUniverse:
    types: list[GroundType] = field(default_factory=list)

    @staticmethod
    def with_primitives() -> TypeUniverse:
        prims = [GroundType.primitive(n) for n in ("i32", "u32", "i64", "u64", "bool", "usize")]
        return TypeUniverse(types=prims)

    def add(self, ty: GroundType) -> None:
        if ty not in self.types:
            self.types.append(ty)

    def candidates_for_bounds(
        self,
        bounds: list[str],
        sigma: object = None,
        check_obligations: bool = True,
    ) -> list[GroundType]:
        return [
            ty for ty in self.types
            if _satisfies_bounds(ty, bounds, sigma, check_obligations)
        ]


def _satisfies_bounds(
    ty: GroundType,
    bounds: list[str],
    sigma: object = None,
    check_obligations: bool = True,
) -> bool:
    for b in bounds:
        if b == "Copy":
            if not ty.is_copy():
                if sigma is not None and hasattr(sigma, "implements_trait"):
                    if not sigma.implements_trait(ty, "Copy"):
                        return False
                else:
                    return False
        elif b == "Default":
            if not ty.is_primitive():
                return False
        elif b == "Clone":
            if sigma is not None and hasattr(sigma, "implements_trait"):
                if not sigma.implements_trait(ty, "Clone"):
                    if not ty.is_copy():
                        return False
            elif not ty.is_copy():
                return False
        else:
            if check_obligations and sigma is not None and hasattr(sigma, "implements_trait"):
                if not sigma.implements_trait(ty, b):
                    return False
    return True


def enumerate_instantiations(
    type_vars_with_bounds: list[tuple[str, list[str]]],
    universe: TypeUniverse,
    sigma: object = None,
    check_obligations: bool = True,
) -> list[dict[str, GroundType]]:
    """Enumerate all valid substitutions for type variables given bounds."""
    if not type_vars_with_bounds:
        return [{}]

    results: list[dict[str, GroundType]] = []
    _enumerate_helper(type_vars_with_bounds, 0, {}, universe, results,
                      sigma, check_obligations)
    return results


def _enumerate_helper(
    tvbs: list[tuple[str, list[str]]],
    idx: int,
    current: dict[str, GroundType],
    universe: TypeUniverse,
    results: list[dict[str, GroundType]],
    sigma: object = None,
    check_obligations: bool = True,
) -> None:
    if idx >= len(tvbs):
        results.append(dict(current))
        return

    var_name, bounds = tvbs[idx]
    candidates = universe.candidates_for_bounds(
        bounds, sigma=sigma, check_obligations=check_obligations,
    )

    for candidate in candidates:
        current[var_name] = candidate
        _enumerate_helper(tvbs, idx + 1, current, universe, results,
                          sigma, check_obligations)

    if var_name in current:
        del current[var_name]
