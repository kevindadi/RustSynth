"""
Run all experiments: RQ1, RQ2, RQ3, benchmark refinement, generate artifacts.
"""

from __future__ import annotations

import csv
import json
import logging
import sys
import time
from pathlib import Path

logging.basicConfig(level=logging.INFO, format="%(levelname)s: %(message)s")
logger = logging.getLogger(__name__)


def collect_task_files() -> tuple[list[Path], list[Path], list[Path]]:
    """Collect task files split into sanity + discriminative suites."""
    tasks_dir = Path("benchmarks/tasks")
    all_tasks = sorted(tasks_dir.glob("*.json"))

    sanity = []
    discriminative = []

    for tf in all_tasks:
        task = json.loads(tf.read_text())
        family = task.get("family", "")
        if family:
            discriminative.append(tf)
        else:
            sanity.append(tf)

    return all_tasks, sanity, discriminative


def run_benchmark_refinement(all_tasks: list[Path]) -> None:
    """Run the benchmark refinement loop."""
    from experiments.benchmark_refinement import run_refinement_loop

    logger.info("=== Benchmark Refinement Loop ===")
    output_dir = Path("results/csv")
    results, dist_rows = run_refinement_loop(all_tasks, output_dir)

    disc_count = sum(1 for r in dist_rows if r.is_discriminative)
    total = len(dist_rows)
    logger.info("Distinguishability: %d/%d tasks are discriminative", disc_count, total)

    families = {}
    for r in dist_rows:
        fam = r.family
        if fam not in families:
            families[fam] = {"total": 0, "disc": 0}
        families[fam]["total"] += 1
        if r.is_discriminative:
            families[fam]["disc"] += 1

    for fam, counts in sorted(families.items()):
        logger.info("  %s: %d/%d discriminative", fam, counts["disc"], counts["total"])


def run_rq1(all_tasks: list[Path]) -> dict:
    """Run RQ1: Soundness + miss analysis + negative sanity."""
    from experiments.rq1_soundness import run_rq1 as _run_rq1

    logger.info("=== RQ1: Full PCPN Soundness & Miss Analysis ===")
    result = _run_rq1(all_tasks)

    # RQ1-A summary
    total_a = len(result.soundness_samples)
    pass_a = sum(1 for s in result.soundness_samples if s.compiler_pass)
    emit_ok = sum(1 for s in result.soundness_samples if s.emit_success)
    logger.info("RQ1-A: %d samples, %d emit success, %d compiler pass", total_a, emit_ok, pass_a)

    accept_fail = [s for s in result.soundness_samples if s.emit_success and not s.compiler_pass]
    if accept_fail:
        logger.warning("RQ1-A: %d accept-fail cases (needs triage)", len(accept_fail))
    else:
        logger.info("RQ1-A: 0 accept-fail — acceptance soundness achieved")

    # RQ1-B summary
    total_b = len(result.miss_samples)
    misses = [m for m in result.miss_samples if m.gold_exists and m.gold_compiles and not m.pcpn_found_witness]
    logger.info("RQ1-B: %d tasks, %d misses", total_b, len(misses))
    for m in misses:
        logger.info("  Miss: %s — %s", m.task_id, m.miss_cause)

    # RQ1-C summary
    total_c = len(result.negative_samples)
    neg_pass = sum(1 for s in result.negative_samples if s.compiler_pass)
    logger.info("RQ1-C: %d negative samples, %d compiler pass (reject-pass)", total_c, neg_pass)

    # Write triage
    result.triage.write_csv(Path("results/csv"))
    result.triage.write_jsonl(Path("results/raw/triage.jsonl"))

    # Write CSVs
    _write_rq1_csvs(result)

    return {
        "rq1a_total": total_a,
        "rq1a_pass": pass_a,
        "rq1a_accept_fail": len(accept_fail),
        "rq1b_total": total_b,
        "rq1b_misses": len(misses),
        "rq1c_total": total_c,
        "rq1c_reject_pass": neg_pass,
        "soundness_ok": len(accept_fail) == 0,
    }


def run_rq2(disc_tasks: list[Path]) -> dict:
    """Run RQ2: Ablation on discriminative suite."""
    from experiments.rq2_ablation import run_rq2 as _run_rq2

    logger.info("=== RQ2: Discriminative Ablation ===")
    result = _run_rq2(disc_tasks)

    _write_rq2_csvs(result)

    full_pass = sum(1 for r in result.rows if r.variant == "Full" and r.compile_result == "pass")
    full_total = sum(1 for r in result.rows if r.variant == "Full")
    logger.info("RQ2: Full pass rate: %d/%d", full_pass, full_total)

    for variant in ["NoStack", "NoCapability", "NoObligation", "TypeOnly"]:
        v_pass = sum(1 for r in result.rows if r.variant == variant and r.compile_result == "pass")
        v_fa = sum(1 for r in result.rows if r.variant == variant and r.false_accept)
        v_total = sum(1 for r in result.rows if r.variant == variant)
        logger.info("  %s: pass=%d/%d false_accept=%d", variant, v_pass, v_total, v_fa)

    return {
        "total_tasks": full_total,
        "full_pass": full_pass,
        "family_breakdowns": [
            {
                "family": fb.family,
                "variant": fb.variant,
                "pass": fb.pass_count,
                "fail": fb.fail_count,
                "false_accept": fb.false_accept_count,
            }
            for fb in result.family_breakdowns
        ],
    }


def run_rq3(all_tasks: list[Path]) -> dict:
    """Run RQ3: Downstream usefulness."""
    from experiments.rq3_downstream import run_rq3 as _run_rq3

    logger.info("=== RQ3: Downstream Usefulness ===")
    result = _run_rq3(all_tasks)

    _write_rq3_csvs(result)

    logger.info("RQ3 EmitFull: witness=%d emit=%d compile=%d (of %d)",
                result.emit_full_witness_found, result.emit_full_emit_success,
                result.emit_full_compiler_pass, result.emit_full_total)

    scaffolds_ok = sum(1 for s in result.scaffold_stats if s.scaffold_generated)
    total_holes = sum(s.hole_count for s in result.scaffold_stats)
    logger.info("RQ3 Scaffold: %d generated, %d total holes", scaffolds_ok, total_holes)

    if result.qwen_available:
        qwen_pass = sum(1 for r in result.qwen_runs if r.compile_pass)
        qwen_repair = sum(1 for r in result.qwen_runs if r.compile_repair_pass)
        logger.info("RQ3 Qwen: %d one-shot pass, %d repair pass", qwen_pass, qwen_repair)
    else:
        logger.info("RQ3 Qwen: NOT AVAILABLE (DASHSCOPE_API_KEY not set)")

    return {
        "emit_full_total": result.emit_full_total,
        "emit_full_witness": result.emit_full_witness_found,
        "emit_full_compile": result.emit_full_compiler_pass,
        "scaffolds_generated": scaffolds_ok,
        "total_holes": total_holes,
        "qwen_available": result.qwen_available,
    }


def _write_rq1_csvs(result) -> None:
    out = Path("results/csv")
    out.mkdir(parents=True, exist_ok=True)

    with open(out / "rq1_soundness.csv", "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["task_id", "fragment", "witnesses_found", "emit_success",
                     "compiler_pass", "errors"])
        for s in result.soundness_samples:
            w.writerow([s.task_id, s.fragment, s.witnesses_found,
                        s.emit_success, s.compiler_pass,
                        "; ".join(s.compiler_errors[:3])])

    with open(out / "rq1_miss_analysis.csv", "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["task_id", "gold_exists", "gold_compiles",
                     "pcpn_found_witness", "miss_cause"])
        for m in result.miss_samples:
            w.writerow([m.task_id, m.gold_exists, m.gold_compiles,
                        m.pcpn_found_witness, m.miss_cause])

    with open(out / "rq1_negative_sanity.csv", "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["task_id", "mutation_type", "pcpn_accepts", "compiler_pass"])
        for s in result.negative_samples:
            w.writerow([s.task_id, s.mutation_type, s.pcpn_accepts, s.compiler_pass])


def _write_rq2_csvs(result) -> None:
    out = Path("results/csv")
    out.mkdir(parents=True, exist_ok=True)

    with open(out / "rq2_ablation.csv", "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["task_id", "family", "variant", "witnesses_found",
                     "compile_result", "false_accept", "states_explored",
                     "search_time_ms"])
        for r in result.rows:
            w.writerow([r.task_id, r.family, r.variant, r.witnesses_found,
                        r.compile_result, r.false_accept, r.states_explored,
                        f"{r.search_time_ms:.1f}"])

    with open(out / "rq2_family_breakdown.csv", "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["family", "variant", "total_tasks", "pass_count",
                     "fail_count", "no_witness", "false_accept_count"])
        for fb in result.family_breakdowns:
            w.writerow([fb.family, fb.variant, fb.total_tasks, fb.pass_count,
                        fb.fail_count, fb.no_witness_count, fb.false_accept_count])


def _write_rq3_csvs(result) -> None:
    out = Path("results/csv")
    out.mkdir(parents=True, exist_ok=True)

    with open(out / "rq3_scaffold_stats.csv", "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["task_id", "fragment", "witnesses_found",
                     "scaffold_generated", "hole_count", "hole_types"])
        for s in result.scaffold_stats:
            w.writerow([s.task_id, s.fragment, s.witnesses_found,
                        s.scaffold_generated, s.hole_count,
                        json.dumps(s.hole_types)])

    if result.qwen_runs:
        with open(out / "rq3_qwen_runs.csv", "w", newline="") as f:
            w = csv.writer(f)
            w.writerow(["task_id", "mode", "compile_pass", "repair_rounds",
                         "compile_repair_pass", "error"])
            for r in result.qwen_runs:
                w.writerow([r.task_id, r.mode, r.compile_pass,
                            r.repair_rounds, r.compile_repair_pass, r.error])


def generate_tables_and_figures(rq1_stats, rq2_stats, rq3_stats) -> None:
    """Generate LaTeX tables and figures."""
    from experiments.render_tables import render_all_tables
    from experiments.render_figures import render_all_figures

    render_all_tables(rq1_stats, rq2_stats, rq3_stats)
    render_all_figures(rq1_stats, rq2_stats, rq3_stats)


def write_summary(rq1_stats, rq2_stats, rq3_stats) -> None:
    """Write final summary markdown."""
    out = Path("results/summary.md")
    out.parent.mkdir(parents=True, exist_ok=True)

    with open(out, "w") as f:
        f.write("# rustsynth-pcpn v2 Experiment Summary\n\n")

        f.write("## RQ1: Full PCPN Soundness & Miss Analysis\n\n")
        f.write(f"- **Acceptance soundness**: {'ACHIEVED (0 accept-fail)' if rq1_stats.get('soundness_ok') else 'NOT ACHIEVED'}\n")
        f.write(f"- Total RQ1-A samples: {rq1_stats.get('rq1a_total', 0)}\n")
        f.write(f"- Compiler pass: {rq1_stats.get('rq1a_pass', 0)}\n")
        f.write(f"- Accept-fail cases: {rq1_stats.get('rq1a_accept_fail', 0)}\n")
        f.write(f"- RQ1-B misses: {rq1_stats.get('rq1b_misses', 0)} / {rq1_stats.get('rq1b_total', 0)}\n")
        f.write(f"- RQ1-C negative samples: {rq1_stats.get('rq1c_total', 0)}, reject-pass: {rq1_stats.get('rq1c_reject_pass', 0)}\n\n")

        f.write("## RQ2: Discriminative Ablation\n\n")
        f.write(f"- Total tasks: {rq2_stats.get('total_tasks', 0)}\n")
        f.write(f"- Full PCPN pass: {rq2_stats.get('full_pass', 0)}\n\n")

        f.write("### Per-Family Breakdown\n\n")
        f.write("| Family | Variant | Pass | Fail | False Accept |\n")
        f.write("|--------|---------|------|------|-------------|\n")
        for fb in rq2_stats.get("family_breakdowns", []):
            f.write(f"| {fb['family']} | {fb['variant']} | {fb['pass']} | {fb['fail']} | {fb['false_accept']} |\n")
        f.write("\n")

        f.write("## RQ3: Downstream Usefulness\n\n")
        f.write(f"- EmitFull: witness={rq3_stats.get('emit_full_witness', 0)}, "
                f"compile={rq3_stats.get('emit_full_compile', 0)} "
                f"/ {rq3_stats.get('emit_full_total', 0)}\n")
        f.write(f"- Scaffolds generated: {rq3_stats.get('scaffolds_generated', 0)}\n")
        f.write(f"- Total holes: {rq3_stats.get('total_holes', 0)}\n")
        if rq3_stats.get("qwen_available"):
            f.write("- Qwen concretization: AVAILABLE\n")
        else:
            f.write("- Qwen concretization: PENDING (DASHSCOPE_API_KEY not set)\n")
        f.write("\n")

        f.write("---\n")
        f.write("*Generated by rustsynth-pcpn v2. All results from real runs.*\n")


def main() -> None:
    all_tasks, sanity_tasks, disc_tasks = collect_task_files()

    logger.info("Found %d total tasks (%d sanity, %d discriminative)",
                len(all_tasks), len(sanity_tasks), len(disc_tasks))

    t0 = time.time()

    run_benchmark_refinement(disc_tasks)

    rq1_stats = run_rq1(all_tasks)

    rq2_stats = run_rq2(disc_tasks)

    rq3_stats = run_rq3(all_tasks)

    try:
        generate_tables_and_figures(rq1_stats, rq2_stats, rq3_stats)
    except Exception as e:
        logger.warning("Table/figure generation failed: %s", e)

    write_summary(rq1_stats, rq2_stats, rq3_stats)

    elapsed = time.time() - t0
    logger.info("All experiments completed in %.1f seconds", elapsed)
    logger.info("Results written to results/")


if __name__ == "__main__":
    main()
