"""
RQ2: Discriminative ablation study.

Uses only the discriminative benchmark suite.
Reports per-family behavior differences.
"""

from __future__ import annotations

import json
import logging
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

from rustsynth.core.fragments import Fragment, classify_task

logger = logging.getLogger(__name__)

ABLATION_VARIANTS = {
    "Full": ({}, True),
    "NoStack": ({"use_stack": False}, True),
    "NoCapability": ({"use_capability": False}, True),
    "NoObligation": ({}, False),
    "TypeOnly": (
        {"use_stack": False, "use_capability": False, "type_only": True},
        False,
    ),
}


@dataclass
class AblationRow:
    task_id: str
    family: str
    variant: str
    witnesses_found: int
    compile_result: str  # "pass" | "fail" | "no_witness"
    false_accept: bool = False
    states_explored: int = 0
    search_time_ms: float = 0.0


@dataclass
class FamilyBreakdown:
    family: str
    variant: str
    total_tasks: int = 0
    pass_count: int = 0
    fail_count: int = 0
    no_witness_count: int = 0
    false_accept_count: int = 0


@dataclass
class RQ2Result:
    rows: list[AblationRow] = field(default_factory=list)
    family_breakdowns: list[FamilyBreakdown] = field(default_factory=list)


def run_rq2(task_files: list[Path]) -> RQ2Result:
    """Run ablation study on discriminative suite."""
    from rustsynth.extractor.syn_fallback import parse_rust_source
    from rustsynth.extractor.sigma import _inject_primitive_providers
    from rustsynth.core.pcpn import PCPN
    from rustsynth.core.search import bounded_reachability
    from rustsynth.ir.plan_trace import PlanTrace
    from rustsynth.ir.snippet_ir import lower_to_snippet
    from rustsynth.emit.emit_full import render_full
    from rustsynth.oracle.compiler_oracle import check_generated_file

    result = RQ2Result()

    for tf in task_files:
        task = json.loads(tf.read_text())
        task_id = task["task_id"]
        bench = task["crate_name"]
        family = task.get("family", "")
        frag_class = classify_task(task)

        if frag_class.fragment != Fragment.F_CORE_EMIT:
            continue

        src_path = Path(f"benchmarks/{bench}/src/lib.rs")
        if not src_path.exists():
            continue

        src = src_path.read_text()
        sigma = parse_rust_source(src, bench)
        _inject_primitive_providers(sigma)

        for vname, (cfg, check_obl) in ABLATION_VARIANTS.items():
            pcpn = PCPN.from_sigma(sigma, check_obligations=check_obl)
            t0 = time.time()
            search_result = bounded_reachability(pcpn, task, search_cfg=cfg)
            elapsed = (time.time() - t0) * 1000

            if search_result.witnesses:
                w = search_result.witnesses[0]
                plan = PlanTrace.from_witness(w, task_id=task_id)
                snippet = lower_to_snippet(plan, sigma)
                code = render_full(snippet, task_id, crate_name=bench)
                try:
                    cr = check_generated_file(
                        Path(task["crate_path"]), code,
                        f"test_{task_id}_{vname}.rs"
                    )
                    compile_str = "pass" if cr.success else "fail"
                except Exception:
                    compile_str = "fail"
                is_false_accept = (compile_str == "fail")
            else:
                compile_str = "no_witness"
                is_false_accept = False

            row = AblationRow(
                task_id=task_id,
                family=family,
                variant=vname,
                witnesses_found=len(search_result.witnesses),
                compile_result=compile_str,
                false_accept=is_false_accept,
                states_explored=search_result.states_explored,
                search_time_ms=elapsed,
            )
            result.rows.append(row)

    result.family_breakdowns = _compute_family_breakdown(result.rows)
    return result


def _compute_family_breakdown(rows: list[AblationRow]) -> list[FamilyBreakdown]:
    """Compute per-family per-variant breakdown."""
    from collections import defaultdict

    groups: dict[tuple[str, str], list[AblationRow]] = defaultdict(list)
    for r in rows:
        groups[(r.family, r.variant)].append(r)

    breakdowns = []
    for (fam, var), rlist in sorted(groups.items()):
        bd = FamilyBreakdown(family=fam, variant=var)
        bd.total_tasks = len(rlist)
        for r in rlist:
            if r.compile_result == "pass":
                bd.pass_count += 1
            elif r.compile_result == "fail":
                bd.fail_count += 1
            else:
                bd.no_witness_count += 1
            if r.false_accept:
                bd.false_accept_count += 1
        breakdowns.append(bd)

    return breakdowns
