"""
RQ3: Downstream usefulness — PCPN scaffold + optional Qwen concretization.

Three modes:
  1. ScaffoldOnly — export scaffold.rs + holes.json
  2. PCPN+Qwen — fill holes using Qwen + compile-repair
  3. DirectQwen — Qwen generates test without PCPN scaffold (baseline)
"""

from __future__ import annotations

import json
import logging
import os
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

from rustsynth.core.fragments import Fragment, classify_task

logger = logging.getLogger(__name__)


@dataclass
class ScaffoldStats:
    task_id: str
    fragment: str
    witnesses_found: int
    scaffold_generated: bool
    hole_count: int = 0
    hole_types: dict[str, int] = field(default_factory=dict)


@dataclass
class QwenRunResult:
    task_id: str
    mode: str  # "pcpn_qwen" | "direct_qwen"
    compile_pass: bool = False
    repair_rounds: int = 0
    compile_repair_pass: bool = False
    error: str = ""


@dataclass
class RQ3Result:
    scaffold_stats: list[ScaffoldStats] = field(default_factory=list)
    qwen_runs: list[QwenRunResult] = field(default_factory=list)
    qwen_available: bool = False

    # EmitFull stats (for tasks in F_core_emit)
    emit_full_witness_found: int = 0
    emit_full_emit_success: int = 0
    emit_full_compiler_pass: int = 0
    emit_full_total: int = 0


def run_rq3(task_files: list[Path]) -> RQ3Result:
    """Run RQ3 experiment."""
    from rustsynth.extractor.syn_fallback import parse_rust_source
    from rustsynth.extractor.sigma import _inject_primitive_providers
    from rustsynth.core.pcpn import PCPN
    from rustsynth.core.search import bounded_reachability
    from rustsynth.ir.plan_trace import PlanTrace
    from rustsynth.ir.snippet_ir import lower_to_snippet
    from rustsynth.emit.emit_full import render_full
    from rustsynth.emit.export_scaffold import export_scaffold
    from rustsynth.oracle.compiler_oracle import check_generated_file

    result = RQ3Result()
    result.qwen_available = bool(os.environ.get("DASHSCOPE_API_KEY"))

    for tf in task_files:
        task = json.loads(tf.read_text())
        task_id = task["task_id"]
        bench = task["crate_name"]
        frag_class = classify_task(task)

        src_path = Path(f"benchmarks/{bench}/src/lib.rs")
        if not src_path.exists():
            continue

        src = src_path.read_text()
        sigma = parse_rust_source(src, bench)
        _inject_primitive_providers(sigma)
        pcpn = PCPN.from_sigma(sigma, check_obligations=True)

        search_result = bounded_reachability(pcpn, task)

        # EmitFull stats for core_emit tasks
        if frag_class.fragment == Fragment.F_CORE_EMIT:
            result.emit_full_total += 1
            if search_result.witnesses:
                result.emit_full_witness_found += 1
                w = search_result.witnesses[0]
                plan = PlanTrace.from_witness(w, task_id=task_id)
                snippet = lower_to_snippet(plan, sigma)
                code = render_full(snippet, task_id, crate_name=bench)
                try:
                    cr = check_generated_file(
                        Path(task["crate_path"]), code,
                        f"test_{task_id}_rq3.rs"
                    )
                    result.emit_full_emit_success += 1
                    if cr.success:
                        result.emit_full_compiler_pass += 1
                except Exception:
                    pass

        # ScaffoldOnly for all tasks with witnesses
        if search_result.witnesses:
            w = search_result.witnesses[0]
            plan = PlanTrace.from_witness(w, task_id=task_id)
            snippet = lower_to_snippet(plan, sigma)

            scaffold_dir = Path("results/scaffolds") / task_id
            scaffold_dir.mkdir(parents=True, exist_ok=True)
            try:
                export_scaffold(snippet, task_id, str(scaffold_dir))
                holes_file = scaffold_dir / f"{task_id}.holes.json"
                hole_count = 0
                hole_types: dict[str, int] = {}
                if holes_file.exists():
                    holes = json.loads(holes_file.read_text())
                    hole_count = len(holes)
                    for h in holes:
                        ht = h.get("kind", "unknown")
                        hole_types[ht] = hole_types.get(ht, 0) + 1
                scaffold_ok = True
            except Exception as e:
                scaffold_ok = False
                hole_count = 0
                hole_types = {}
                logger.warning("Scaffold export failed for %s: %s", task_id, e)

            result.scaffold_stats.append(ScaffoldStats(
                task_id=task_id,
                fragment=frag_class.label,
                witnesses_found=len(search_result.witnesses),
                scaffold_generated=scaffold_ok,
                hole_count=hole_count,
                hole_types=hole_types,
            ))
        else:
            result.scaffold_stats.append(ScaffoldStats(
                task_id=task_id,
                fragment=frag_class.label,
                witnesses_found=0,
                scaffold_generated=False,
            ))

        # PCPN+Qwen mode
        if result.qwen_available and search_result.witnesses:
            qwen_result = _run_pcpn_qwen(task, sigma, search_result, scaffold_dir)
            result.qwen_runs.append(qwen_result)

        # DirectQwen baseline
        if result.qwen_available:
            direct_result = _run_direct_qwen(task, sigma)
            result.qwen_runs.append(direct_result)

    return result


def _run_pcpn_qwen(task, sigma, search_result, scaffold_dir) -> QwenRunResult:
    """Run PCPN+Qwen concretization (stub — requires Qwen API)."""
    try:
        from rustsynth.concretizers.qwen_adapter import QwenAdapter
        from rustsynth.concretizers.qwen_repair import compile_repair_loop

        adapter = QwenAdapter.from_env()
        scaffold_path = scaffold_dir / f"{task['task_id']}.rs"
        holes_path = scaffold_dir / f"{task['task_id']}.holes.json"

        if not scaffold_path.exists():
            return QwenRunResult(task_id=task["task_id"], mode="pcpn_qwen",
                                error="scaffold not found")

        code, rounds, success = compile_repair_loop(
            adapter, scaffold_path, holes_path,
            Path(task["crate_path"]), sigma,
            max_rounds=3,
        )
        return QwenRunResult(
            task_id=task["task_id"], mode="pcpn_qwen",
            compile_pass=success and rounds == 0,
            repair_rounds=rounds,
            compile_repair_pass=success,
        )
    except ImportError:
        return QwenRunResult(task_id=task["task_id"], mode="pcpn_qwen",
                            error="qwen adapter not available")
    except Exception as e:
        return QwenRunResult(task_id=task["task_id"], mode="pcpn_qwen",
                            error=str(e))


def _run_direct_qwen(task, sigma) -> QwenRunResult:
    """Run DirectQwen baseline (stub — requires Qwen API)."""
    try:
        from rustsynth.concretizers.qwen_adapter import QwenAdapter

        adapter = QwenAdapter.from_env()
        prompt = _build_direct_prompt(task, sigma)
        response = adapter.complete(prompt)

        from rustsynth.oracle.compiler_oracle import check_generated_file
        cr = check_generated_file(
            Path(task["crate_path"]), response, f"test_direct_{task['task_id']}.rs"
        )
        return QwenRunResult(
            task_id=task["task_id"], mode="direct_qwen",
            compile_pass=cr.success,
        )
    except ImportError:
        return QwenRunResult(task_id=task["task_id"], mode="direct_qwen",
                            error="qwen adapter not available")
    except Exception as e:
        return QwenRunResult(task_id=task["task_id"], mode="direct_qwen",
                            error=str(e))


def _build_direct_prompt(task: dict, sigma) -> str:
    api_desc = json.dumps(sigma.to_dict(), indent=2)[:2000]
    return (
        f"Write a Rust integration test for crate `{task['crate_name']}` "
        f"that exercises the public API.\n\n"
        f"Task: {task.get('description', 'synthesize a compilable test')}\n\n"
        f"API signatures:\n{api_desc}\n\n"
        f"Requirements:\n"
        f"- Use only public API\n"
        f"- Include #[test] attribute\n"
        f"- Must compile with cargo test --no-run\n"
        f"- Output ONLY the Rust code, no explanation\n"
    )
