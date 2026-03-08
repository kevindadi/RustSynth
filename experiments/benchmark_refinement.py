"""
Benchmark refinement loop.

Runs Full PCPN + all ablation variants on all benchmarks.
Computes per-task distinguishability. Documents which families are
discriminative and which are not.
"""

from __future__ import annotations

import csv
import json
import logging
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

logger = logging.getLogger(__name__)

ABLATION_VARIANTS = {
    "Full": ({}, True),
    "NoStack": ({"use_stack": False}, True),
    "NoCapability": ({"use_capability": False}, True),
    "NoObligation": ({}, False),
    "TypeOnly": ({"use_stack": False, "use_capability": False, "type_only": True}, False),
}


@dataclass
class TaskResult:
    task_id: str
    family: str
    variant: str
    witnesses_found: int
    compile_result: str  # "pass" | "fail" | "no_witness"
    states_explored: int
    search_time_ms: float


@dataclass
class DistinguishabilityRow:
    task_id: str
    family: str
    full_result: str
    result_vector: dict[str, str] = field(default_factory=dict)
    is_discriminative: bool = False
    discriminative_for: list[str] = field(default_factory=list)


def run_refinement_loop(
    task_files: list[Path],
    output_dir: Path,
) -> tuple[list[TaskResult], list[DistinguishabilityRow]]:
    """Run all variants on all tasks and compute distinguishability."""
    from rustsynth.extractor.syn_fallback import parse_rust_source
    from rustsynth.extractor.sigma import _inject_primitive_providers
    from rustsynth.core.pcpn import PCPN
    from rustsynth.core.search import bounded_reachability
    from rustsynth.ir.plan_trace import PlanTrace
    from rustsynth.ir.snippet_ir import lower_to_snippet
    from rustsynth.emit.emit_full import render_full
    from rustsynth.oracle.compiler_oracle import check_generated_file

    all_results: list[TaskResult] = []
    dist_rows: list[DistinguishabilityRow] = []

    for tf in task_files:
        task = json.loads(tf.read_text())
        task_id = task["task_id"]
        bench = task["crate_name"]
        family = task.get("family", "sanity")
        src_path = Path(f"benchmarks/{bench}/src/lib.rs")
        if not src_path.exists():
            logger.warning("Source missing for %s", bench)
            continue

        src = src_path.read_text()
        sigma = parse_rust_source(src, bench)
        _inject_primitive_providers(sigma)

        result_vector: dict[str, str] = {}

        for vname, (cfg, check_obl) in ABLATION_VARIANTS.items():
            pcpn = PCPN.from_sigma(sigma, check_obligations=check_obl)
            t0 = time.time()
            result = bounded_reachability(pcpn, task, search_cfg=cfg)
            elapsed = (time.time() - t0) * 1000

            if result.witnesses:
                w = result.witnesses[0]
                plan = PlanTrace.from_witness(w, task_id=task_id)
                snippet = lower_to_snippet(plan, sigma)
                code = render_full(snippet, task_id, crate_name=bench)
                cr = check_generated_file(
                    Path(task["crate_path"]), code, f"test_{bench}_{vname}.rs"
                )
                compile_str = "pass" if cr.success else "fail"
            else:
                compile_str = "no_witness"

            tr = TaskResult(
                task_id=task_id,
                family=family,
                variant=vname,
                witnesses_found=len(result.witnesses),
                compile_result=compile_str,
                states_explored=result.states_explored,
                search_time_ms=elapsed,
            )
            all_results.append(tr)
            result_vector[vname] = compile_str

        full_result = result_vector.get("Full", "no_witness")
        disc_for = [
            v for v, r in result_vector.items()
            if v != "Full" and r != full_result
        ]
        dist_rows.append(DistinguishabilityRow(
            task_id=task_id,
            family=family,
            full_result=full_result,
            result_vector=result_vector,
            is_discriminative=len(disc_for) > 0,
            discriminative_for=disc_for,
        ))

    _write_results(output_dir, all_results, dist_rows)
    return all_results, dist_rows


def _write_results(
    output_dir: Path,
    results: list[TaskResult],
    dist_rows: list[DistinguishabilityRow],
) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)

    dist_path = output_dir / "benchmark_distinguishability.csv"
    with open(dist_path, "w", newline="") as f:
        writer = csv.writer(f)
        header = ["task_id", "family", "full_result",
                  "NoStack", "NoCapability", "NoObligation", "TypeOnly",
                  "is_discriminative", "discriminative_for"]
        writer.writerow(header)
        for row in dist_rows:
            writer.writerow([
                row.task_id, row.family, row.full_result,
                row.result_vector.get("NoStack", ""),
                row.result_vector.get("NoCapability", ""),
                row.result_vector.get("NoObligation", ""),
                row.result_vector.get("TypeOnly", ""),
                row.is_discriminative,
                ";".join(row.discriminative_for),
            ])

    detail_path = output_dir / "refinement_detail.csv"
    with open(detail_path, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=[
            "task_id", "family", "variant", "witnesses_found",
            "compile_result", "states_explored", "search_time_ms",
        ])
        writer.writeheader()
        for r in results:
            writer.writerow({
                "task_id": r.task_id, "family": r.family,
                "variant": r.variant, "witnesses_found": r.witnesses_found,
                "compile_result": r.compile_result,
                "states_explored": r.states_explored,
                "search_time_ms": f"{r.search_time_ms:.1f}",
            })

    summary = _compute_summary(dist_rows)
    summary_path = output_dir / "refinement_summary.json"
    with open(summary_path, "w") as f:
        json.dump(summary, f, indent=2)


def _compute_summary(dist_rows: list[DistinguishabilityRow]) -> dict:
    families: dict[str, dict] = {}
    for row in dist_rows:
        fam = row.family
        if fam not in families:
            families[fam] = {"total": 0, "discriminative": 0, "tasks": []}
        families[fam]["total"] += 1
        if row.is_discriminative:
            families[fam]["discriminative"] += 1
        families[fam]["tasks"].append({
            "task_id": row.task_id,
            "is_discriminative": row.is_discriminative,
            "discriminative_for": row.discriminative_for,
        })

    total_disc = sum(1 for r in dist_rows if r.is_discriminative)
    return {
        "total_tasks": len(dist_rows),
        "discriminative_tasks": total_disc,
        "by_family": families,
    }
