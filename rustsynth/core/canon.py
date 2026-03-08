"""
Canonicalization — beta-renaming insensitive state equivalence.

Paper mapping:
  Canon(Cfg) normalizes a configuration <M, S> so that two configurations
  that differ only in variable/region naming are mapped to the same
  canonical form.

Algorithm:
  1. Traverse marking places in sorted order
  2. Assign fresh canonical vid/region ids in encounter order
  3. Traverse stack frames in order
  4. Produce a hashable CanonState
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional

from rustsynth.core.types import (
    GroundType, TypeForm, Capability, VarId, RegionLabel,
    Marking, BorrowStack, StackFrameKind,
)


@dataclass(frozen=True)
class CanonToken:
    vid: int
    ty: GroundType
    form: TypeForm
    regions: tuple[int, ...]
    borrowed_from: Optional[int]


@dataclass(frozen=True)
class CanonFrame:
    kind: StackFrameKind
    owner_vid: int
    ref_vid: Optional[int]
    region: Optional[int]


@dataclass(frozen=True)
class CanonState:
    marking: tuple[tuple[int, tuple[CanonToken, ...]], ...]
    stack: tuple[CanonFrame, ...]

    def hash_key(self) -> str:
        parts = []
        for pid, tokens in self.marking:
            if tokens:
                tstr = ",".join(f"v{t.vid}" for t in tokens)
                parts.append(f"p{pid}:[{tstr}]")
        sstr = ",".join(f.kind.name for f in self.stack)
        return f"M:{';'.join(parts)}|S:{sstr}"


class Canonicalizer:
    """Stateless canonicalizer for configurations."""

    def canonicalize(self, marking: Marking, stack: BorrowStack) -> CanonState:
        vid_map: dict[VarId, int] = {}
        region_map: dict[RegionLabel, int] = {}
        next_vid = 0
        next_region = 0

        def map_vid(v: VarId) -> int:
            nonlocal next_vid
            if v not in vid_map:
                vid_map[v] = next_vid
                next_vid += 1
            return vid_map[v]

        def map_region(r: RegionLabel) -> int:
            nonlocal next_region
            if r not in region_map:
                region_map[r] = next_region
                next_region += 1
            return region_map[r]

        canon_marking = []
        for pid in sorted(marking.tokens.keys()):
            tokens = marking.tokens[pid]
            canon_tokens = []
            for t in tokens:
                cv = map_vid(t.vid)
                cb = map_vid(t.borrowed_from) if t.borrowed_from is not None else None
                cr = tuple(map_region(r) for r in t.regions)
                canon_tokens.append(CanonToken(
                    vid=cv, ty=t.ty, form=t.form,
                    regions=cr, borrowed_from=cb,
                ))
            canon_tokens.sort(key=lambda ct: ct.vid)
            canon_marking.append((pid, tuple(canon_tokens)))

        canon_stack = []
        for f in stack.frames:
            co = map_vid(f.owner_vid)
            cr_vid = map_vid(f.ref_vid) if f.ref_vid is not None else None
            cr_reg = map_region(f.region) if f.region is not None else None
            canon_stack.append(CanonFrame(
                kind=f.kind, owner_vid=co,
                ref_vid=cr_vid, region=cr_reg,
            ))

        return CanonState(
            marking=tuple(canon_marking),
            stack=tuple(canon_stack),
        )
