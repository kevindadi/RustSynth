"""
Fragment classification system for the PCPN project.

Tasks and items are classified into three tiers:
  F_core_emit  — fully supported: PCPN + EmitFull; used in RQ1/RQ2 main tables
  F_scaffold   — PCPN can plan a skeleton but EmitFull can't fully close; RQ3
  F_out        — unsupported language features; excluded from conclusions
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Optional


class Fragment(Enum):
    F_CORE_EMIT = "core_emit"
    F_SCAFFOLD = "scaffold"
    F_OUT = "out"


@dataclass
class FragmentClassification:
    """Classification of a single task or extracted item."""
    fragment: Fragment
    reasons: list[str] = field(default_factory=list)

    @property
    def is_main_table(self) -> bool:
        return self.fragment == Fragment.F_CORE_EMIT

    @property
    def label(self) -> str:
        return self.fragment.value


_UNSUPPORTED_KEYWORDS = {
    "async", "await", "unsafe", "dyn", "impl Trait",
    "macro_rules!", "proc_macro",
}

_SCAFFOLD_INDICATORS = {
    "generic with complex bounds",
    "associated type not in fact table",
    "closure parameter",
    "trait object parameter",
    "higher-ranked lifetime",
}


def classify_task(task: dict) -> FragmentClassification:
    """Classify a task JSON dict into a fragment tier."""
    explicit = task.get("fragment")
    if explicit:
        try:
            return FragmentClassification(Fragment(explicit))
        except ValueError:
            pass

    family = task.get("family", "")
    features = task.get("required_features", [])
    unsupported = task.get("unsupported_features", [])

    if unsupported:
        return FragmentClassification(
            Fragment.F_OUT,
            reasons=[f"unsupported: {f}" for f in unsupported],
        )

    scaffold_reasons = []
    for feat in features:
        if feat in _SCAFFOLD_INDICATORS:
            scaffold_reasons.append(f"scaffold-only: {feat}")
    if scaffold_reasons:
        return FragmentClassification(Fragment.F_SCAFFOLD, scaffold_reasons)

    return FragmentClassification(Fragment.F_CORE_EMIT)


def classify_callable(item_path: str, features: list[str]) -> FragmentClassification:
    """Classify an extracted callable item."""
    for feat in features:
        for kw in _UNSUPPORTED_KEYWORDS:
            if kw in feat:
                return FragmentClassification(
                    Fragment.F_OUT,
                    reasons=[f"unsupported feature: {feat}"],
                )
    for feat in features:
        for ind in _SCAFFOLD_INDICATORS:
            if ind in feat:
                return FragmentClassification(
                    Fragment.F_SCAFFOLD,
                    reasons=[f"scaffold indicator: {feat}"],
                )
    return FragmentClassification(Fragment.F_CORE_EMIT)


@dataclass
class SupportFilterResult:
    """Result of support_filter for an extracted item."""
    path: str
    category: str  # "supported_core" | "scaffold_only" | "unsupported"
    reasons: list[str] = field(default_factory=list)


def support_filter_item(
    path: str,
    has_async: bool = False,
    has_unsafe: bool = False,
    has_closure_param: bool = False,
    has_trait_object: bool = False,
    has_hrtb: bool = False,
    has_gat: bool = False,
    has_const_generic: bool = False,
    has_complex_generic: bool = False,
    has_macro: bool = False,
) -> SupportFilterResult:
    """Classify an extracted item for support level."""
    reasons: list[str] = []

    if has_async:
        reasons.append("async/await not supported")
    if has_unsafe:
        reasons.append("unsafe code not supported")
    if has_macro:
        reasons.append("macro-dependent item")
    if has_hrtb:
        reasons.append("higher-ranked trait bounds")
    if has_gat:
        reasons.append("generic associated types")
    if has_const_generic:
        reasons.append("const generics")

    if reasons:
        return SupportFilterResult(path, "unsupported", reasons)

    scaffold_reasons: list[str] = []
    if has_closure_param:
        scaffold_reasons.append("closure parameter requires LLM")
    if has_trait_object:
        scaffold_reasons.append("trait object dispatch")
    if has_complex_generic:
        scaffold_reasons.append("complex generic bounds exceed fact table")

    if scaffold_reasons:
        return SupportFilterResult(path, "scaffold_only", scaffold_reasons)

    return SupportFilterResult(path, "supported_core")
