"""Generate LaTeX tables from experiment results."""

from __future__ import annotations

import csv
import json
from pathlib import Path


def render_all_tables(rq1_stats: dict, rq2_stats: dict, rq3_stats: dict) -> None:
    out = Path("results/tables")
    out.mkdir(parents=True, exist_ok=True)

    _render_rq1_main(out / "rq1_main.tex", rq1_stats)
    _render_rq1_miss(out / "rq1_miss.tex", rq1_stats)
    _render_rq2_main(out / "rq2_main.tex", rq2_stats)
    _render_rq3_main(out / "rq3_main.tex", rq3_stats)


def _render_rq1_main(path: Path, stats: dict) -> None:
    total = stats.get("rq1a_total", 0)
    passed = stats.get("rq1a_pass", 0)
    af = stats.get("rq1a_accept_fail", 0)
    soundness = "100\\%" if af == 0 else f"{passed}/{total}"

    lines = [
        "\\begin{table}[t]",
        "\\centering",
        "\\caption{RQ1-A: Acceptance Soundness on $\\mathcal{F}_{\\text{core\\_emit}}$}",
        "\\label{tab:rq1_soundness}",
        "\\begin{tabular}{lrr}",
        "\\toprule",
        "Metric & Value \\\\",
        "\\midrule",
        f"Accepted witnesses & {total} \\\\",
        f"Compiler pass & {passed} \\\\",
        f"Accept-fail & {af} \\\\",
        f"Acceptance soundness & {soundness} \\\\",
        "\\bottomrule",
        "\\end{tabular}",
        "\\end{table}",
    ]
    path.write_text("\n".join(lines) + "\n")


def _render_rq1_miss(path: Path, stats: dict) -> None:
    total = stats.get("rq1b_total", 0)
    misses = stats.get("rq1b_misses", 0)
    miss_rate = f"{misses}/{total}" if total > 0 else "N/A"

    lines = [
        "\\begin{table}[t]",
        "\\centering",
        "\\caption{RQ1-B: Miss Analysis}",
        "\\label{tab:rq1_miss}",
        "\\begin{tabular}{lrr}",
        "\\toprule",
        "Metric & Value \\\\",
        "\\midrule",
        f"Gold-positive tasks & {total} \\\\",
        f"Misses (gold exists, PCPN no witness) & {misses} \\\\",
        f"Miss rate & {miss_rate} \\\\",
        "\\bottomrule",
        "\\end{tabular}",
        "\\end{table}",
    ]
    path.write_text("\n".join(lines) + "\n")


def _render_rq2_main(path: Path, stats: dict) -> None:
    breakdowns = stats.get("family_breakdowns", [])

    variants = ["Full", "NoStack", "NoCapability", "NoObligation", "TypeOnly"]
    families = sorted(set(fb["family"] for fb in breakdowns))

    lines = [
        "\\begin{table}[t]",
        "\\centering",
        "\\caption{RQ2: Ablation Study — Compile Pass Rate by Family}",
        "\\label{tab:rq2_ablation}",
        "\\begin{tabular}{l" + "r" * len(variants) + "}",
        "\\toprule",
        "Family & " + " & ".join(v.replace("No", "No\\-") for v in variants) + " \\\\",
        "\\midrule",
    ]

    for fam in families:
        row = [fam.replace("_", "\\_")]
        for v in variants:
            match = [fb for fb in breakdowns if fb["family"] == fam and fb["variant"] == v]
            if match:
                fb = match[0]
                total = fb["pass"] + fb["fail"] + fb.get("no_witness", 0)
                rate = f"{fb['pass']}/{total}" if total > 0 else "---"
                if fb.get("false_accept", 0) > 0:
                    rate += f" ({fb['false_accept']}FA)"
            else:
                rate = "---"
            row.append(rate)
        lines.append(" & ".join(row) + " \\\\")

    lines.extend([
        "\\bottomrule",
        "\\end{tabular}",
        "\\end{table}",
    ])
    path.write_text("\n".join(lines) + "\n")


def _render_rq3_main(path: Path, stats: dict) -> None:
    lines = [
        "\\begin{table}[t]",
        "\\centering",
        "\\caption{RQ3: Downstream Usefulness}",
        "\\label{tab:rq3_downstream}",
        "\\begin{tabular}{lrr}",
        "\\toprule",
        "Metric & Value \\\\",
        "\\midrule",
        f"EmitFull total tasks & {stats.get('emit_full_total', 0)} \\\\",
        f"Witness found & {stats.get('emit_full_witness', 0)} \\\\",
        f"Compiler pass & {stats.get('emit_full_compile', 0)} \\\\",
        f"Scaffolds generated & {stats.get('scaffolds_generated', 0)} \\\\",
        f"Total holes & {stats.get('total_holes', 0)} \\\\",
    ]

    if stats.get("qwen_available"):
        lines.append("Qwen concretization & AVAILABLE \\\\")
    else:
        lines.append("Qwen concretization & PENDING \\\\")

    lines.extend([
        "\\bottomrule",
        "\\end{tabular}",
        "\\end{table}",
    ])
    path.write_text("\n".join(lines) + "\n")
