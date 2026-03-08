"""
RQ1: Compiler-consistency experiment.

Validates that PCPN's guarded enabling / firing predictions are consistent
with Rust compiler's actual accept/reject decisions.

Procedure:
  1. For each benchmark task, generate witness traces (PCPN-accepted)
  2. Generate near-miss negative traces via 1-step mutations (PCPN-rejected)
  3. Lower both to SnippetIR -> EmitFull -> .rs test file
  4. Compile each with compiler oracle
  5. Build confusion matrix and compute precision/recall/F1
"""

from __future__ import annotations

import json
import logging
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

from rustsynth.extractor.sigma import build_sigma
from rustsynth.core.pcpn import PCPN
from rustsynth.core.search import bounded_reachability, SearchResult
from rustsynth.ir.plan_trace import PlanTrace
from rustsynth.ir.snippet_ir import lower_to_snippet
from rustsynth.emit.emit_full import render_full
from rustsynth.oracle.compiler_oracle import check_generated_file, DiagnosticCategory
from experiments.mutator import generate_near_miss_traces

logger = logging.getLogger(__name__)


@dataclass
class RQ1Sample:
    task_id: str
    sample_id: str
    pcpn_accepts: bool
    compiler_passes: bool
    mutation_type: str = "none"
    error_category: str = "pass"
    is_emitter_failure: bool = False


@dataclass
class RQ1Result:
    samples: list[RQ1Sample] = field(default_factory=list)
    tp: int = 0  # PCPN accept & compiler pass
    fp: int = 0  # PCPN accept & compiler fail
    fn_: int = 0  # PCPN reject & compiler pass
    tn: int = 0  # PCPN reject & compiler fail
    emitter_failures: int = 0

    @property
    def precision(self) -> float:
        denom = self.tp + self.fp
        return self.tp / denom if denom > 0 else 0.0

    @property
    def recall(self) -> float:
        denom = self.tp + self.fn_
        return self.tp / denom if denom > 0 else 0.0

    @property
    def f1(self) -> float:
        p, r = self.precision, self.recall
        return 2 * p * r / (p + r) if (p + r) > 0 else 0.0

    def confusion_matrix(self) -> dict:
        return {
            "true_positive": self.tp,
            "false_positive": self.fp,
            "false_negative": self.fn_,
            "true_negative": self.tn,
            "precision": round(self.precision, 4),
            "recall": round(self.recall, 4),
            "f1": round(self.f1, 4),
            "emitter_failures": self.emitter_failures,
        }


def run_rq1(
    benchmarks_dir: Path,
    results_dir: Path,
    task_files: list[Path],
) -> RQ1Result:
    """Run the RQ1 compiler-consistency experiment."""
    results_dir.mkdir(parents=True, exist_ok=True)
    raw_dir = results_dir / "raw"
    raw_dir.mkdir(parents=True, exist_ok=True)

    rq1 = RQ1Result()
    all_samples = []

    for task_file in task_files:
        task = json.loads(task_file.read_text())
        task_id = task["task_id"]
        crate_path = benchmarks_dir.parent / task["crate_path"]

        logger.info("RQ1: Processing task %s", task_id)

        try:
            sigma = build_sigma(crate_path)
        except Exception as e:
            logger.warning("Failed to build sigma for %s: %s", task_id, e)
            continue

        pcpn = PCPN.from_sigma(sigma)
        search_cfg = {
            "max_trace_len": task.get("max_trace_len", 6),
            "stack_depth": task.get("bounds", {}).get("stack_depth", 4),
            "token_per_place": task.get("bounds", {}).get("token_per_place", 3),
            "max_witnesses": 3,
        }

        result = bounded_reachability(pcpn, task, search_cfg)

        for i, witness in enumerate(result.witnesses):
            plan = PlanTrace.from_witness(witness, task_id=task_id)
            snippet = lower_to_snippet(plan, sigma)
            code = render_full(snippet, task_id, crate_name=task.get("crate_name"), test_index=i)

            compile_result = check_generated_file(
                crate_path, code, f"rq1_pos_{task_id}_{i}.rs",
            )

            sample = RQ1Sample(
                task_id=task_id,
                sample_id=f"pos_{i}",
                pcpn_accepts=True,
                compiler_passes=compile_result.success,
                error_category=compile_result.primary_category.value,
            )

            if not compile_result.success:
                if compile_result.primary_category in (
                    DiagnosticCategory.UnresolvedImport,
                    DiagnosticCategory.EmitterBug,
                ):
                    sample.is_emitter_failure = True
                    rq1.emitter_failures += 1

            all_samples.append(sample)

            neg_traces = generate_near_miss_traces(witness, seed=42 + i)
            for mut_name, neg_trace in neg_traces:
                neg_plan = PlanTrace.from_witness(neg_trace, task_id=task_id)
                neg_snippet = lower_to_snippet(neg_plan, sigma)
                neg_code = render_full(
                    neg_snippet, task_id, crate_name=task.get("crate_name"),
                    test_index=100 + i,
                )

                neg_compile = check_generated_file(
                    crate_path, neg_code, f"rq1_neg_{task_id}_{i}_{mut_name}.rs",
                )

                neg_sample = RQ1Sample(
                    task_id=task_id,
                    sample_id=f"neg_{i}_{mut_name}",
                    pcpn_accepts=False,
                    compiler_passes=neg_compile.success,
                    mutation_type=mut_name,
                    error_category=neg_compile.primary_category.value,
                )
                all_samples.append(neg_sample)

    for s in all_samples:
        if s.is_emitter_failure:
            continue
        if s.pcpn_accepts and s.compiler_passes:
            rq1.tp += 1
        elif s.pcpn_accepts and not s.compiler_passes:
            rq1.fp += 1
        elif not s.pcpn_accepts and s.compiler_passes:
            rq1.fn_ += 1
        else:
            rq1.tn += 1

    rq1.samples = all_samples

    raw_path = raw_dir / "rq1_samples.jsonl"
    with open(raw_path, "w") as f:
        for s in all_samples:
            f.write(json.dumps({
                "task_id": s.task_id,
                "sample_id": s.sample_id,
                "pcpn_accepts": s.pcpn_accepts,
                "compiler_passes": s.compiler_passes,
                "mutation_type": s.mutation_type,
                "error_category": s.error_category,
                "is_emitter_failure": s.is_emitter_failure,
            }) + "\n")

    summary_path = results_dir / "rq1_summary.json"
    summary_path.write_text(json.dumps(rq1.confusion_matrix(), indent=2))

    logger.info("RQ1 complete: %s", rq1.confusion_matrix())
    return rq1
