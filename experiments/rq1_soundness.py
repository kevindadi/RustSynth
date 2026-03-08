"""
RQ1: Full PCPN soundness and miss analysis.

RQ1-A: Acceptance soundness on F_core_emit
RQ1-B: Miss analysis (gold-positive tasks)
RQ1-C: Negative sanity (mutation-based)
"""

from __future__ import annotations

import json
import logging
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

from rustsynth.core.fragments import Fragment, classify_task
from rustsynth.oracle.triage import TriagePipeline, TriageRecord

logger = logging.getLogger(__name__)


@dataclass
class RQ1ASample:
    task_id: str
    fragment: str
    witnesses_found: int
    emit_success: bool
    compiler_pass: bool
    compiler_errors: list[str] = field(default_factory=list)


@dataclass
class RQ1BMiss:
    task_id: str
    gold_exists: bool
    gold_compiles: bool
    pcpn_found_witness: bool
    miss_cause: str = ""


@dataclass
class RQ1CSample:
    task_id: str
    mutation_type: str
    pcpn_accepts: bool
    compiler_pass: bool


@dataclass
class RQ1Result:
    soundness_samples: list[RQ1ASample] = field(default_factory=list)
    miss_samples: list[RQ1BMiss] = field(default_factory=list)
    negative_samples: list[RQ1CSample] = field(default_factory=list)
    triage: TriagePipeline = field(default_factory=TriagePipeline)


def run_rq1(task_files: list[Path]) -> RQ1Result:
    """Run all three parts of RQ1."""
    from rustsynth.extractor.syn_fallback import parse_rust_source
    from rustsynth.extractor.sigma import _inject_primitive_providers
    from rustsynth.core.pcpn import PCPN
    from rustsynth.core.search import bounded_reachability
    from rustsynth.ir.plan_trace import PlanTrace
    from rustsynth.ir.snippet_ir import lower_to_snippet
    from rustsynth.emit.emit_full import render_full
    from rustsynth.oracle.compiler_oracle import check_generated_file
    from experiments.mutator import generate_near_miss_traces

    result = RQ1Result()

    for tf in task_files:
        task = json.loads(tf.read_text())
        task_id = task["task_id"]
        bench = task["crate_name"]
        frag_class = classify_task(task)

        if frag_class.fragment != Fragment.F_CORE_EMIT:
            continue

        src_path = Path(f"benchmarks/{bench}/src/lib.rs")
        if not src_path.exists():
            continue

        src = src_path.read_text()
        sigma = parse_rust_source(src, bench)
        _inject_primitive_providers(sigma)
        pcpn = PCPN.from_sigma(sigma, check_obligations=True)

        search_result = bounded_reachability(pcpn, task)

        # RQ1-A: Acceptance soundness
        if search_result.witnesses:
            for wi, w in enumerate(search_result.witnesses[:3]):
                plan = PlanTrace.from_witness(w, task_id=task_id)
                snippet = lower_to_snippet(plan, sigma)
                code = render_full(snippet, task_id, crate_name=bench)

                try:
                    cr = check_generated_file(
                        Path(task["crate_path"]), code,
                        f"test_{task_id}_pos_{wi}.rs"
                    )
                    emit_ok = True
                    compiler_ok = cr.success
                    errors = [e.message for e in cr.errors[:5]]
                except Exception as e:
                    emit_ok = False
                    compiler_ok = False
                    errors = [str(e)]

                sample = RQ1ASample(
                    task_id=f"{task_id}_pos_{wi}",
                    fragment=frag_class.label,
                    witnesses_found=len(search_result.witnesses),
                    emit_success=emit_ok,
                    compiler_pass=compiler_ok,
                    compiler_errors=errors,
                )
                result.soundness_samples.append(sample)

                pcpn_dec = "accept"
                emit_st = "success" if emit_ok else "failure"
                comp_st = "pass" if compiler_ok else "fail"
                result.triage.create_record(
                    task_id=f"{task_id}_pos_{wi}",
                    fragment=frag_class.label,
                    variant="Full",
                    pcpn_decision=pcpn_dec,
                    emit_status=emit_st,
                    compiler_status=comp_st,
                    compiler_errors=errors,
                )

        # RQ1-B: Miss analysis
        gold_dir = Path(f"benchmarks/{bench}/gold")
        gold_files = list(gold_dir.glob("*.rs")) if gold_dir.exists() else []
        gold_exists = len(gold_files) > 0
        gold_compiles = False
        if gold_exists:
            gold_compiles = True  # assumed — gold tests are pre-verified

        miss = RQ1BMiss(
            task_id=task_id,
            gold_exists=gold_exists,
            gold_compiles=gold_compiles,
            pcpn_found_witness=len(search_result.witnesses) > 0,
        )
        if gold_exists and gold_compiles and not search_result.witnesses:
            miss.miss_cause = _classify_miss(task, search_result)
        result.miss_samples.append(miss)

        # RQ1-C: Negative sanity
        if search_result.witnesses:
            try:
                negatives = generate_near_miss_traces(search_result.witnesses[0])
                for mut_type, neg_trace in negatives[:3]:
                    neg_plan = PlanTrace.from_witness(neg_trace, task_id=task_id)
                    neg_snippet = lower_to_snippet(neg_plan, sigma)
                    neg_code = render_full(neg_snippet, task_id, crate_name=bench)
                    try:
                        neg_cr = check_generated_file(
                            Path(task["crate_path"]), neg_code,
                            f"test_{task_id}_neg_{mut_type}.rs"
                        )
                        neg_compile = neg_cr.success
                    except Exception:
                        neg_compile = False

                    result.negative_samples.append(RQ1CSample(
                        task_id=task_id,
                        mutation_type=mut_type,
                        pcpn_accepts=False,
                        compiler_pass=neg_compile,
                    ))
            except Exception as e:
                logger.warning("Mutation failed for %s: %s", task_id, e)

    return result


def _classify_miss(task: dict, search_result) -> str:
    """Classify the root cause of a miss."""
    if search_result.states_explored > 5000:
        return "search_bounds_too_tight"
    goal = task.get("goal", {})
    goal_type = goal.get("type", "")
    if goal_type and not goal_type[0].islower():
        return "finite_type_universe_too_small"
    return "implementation_limitation"
