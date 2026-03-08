"""Generate matplotlib figures from experiment results."""

from __future__ import annotations

import csv
import json
from pathlib import Path

try:
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    HAS_MPL = True
except ImportError:
    HAS_MPL = False


def render_all_figures(rq1_stats: dict, rq2_stats: dict, rq3_stats: dict) -> None:
    if not HAS_MPL:
        return

    out = Path("results/figures")
    out.mkdir(parents=True, exist_ok=True)

    _render_rq2_pass_rate(out / "rq2_pass_rate.png", rq2_stats)
    _render_rq2_false_accepts(out / "rq2_false_accepts.png", rq2_stats)


def _render_rq2_pass_rate(path: Path, stats: dict) -> None:
    breakdowns = stats.get("family_breakdowns", [])
    if not breakdowns:
        return

    variants = ["Full", "NoStack", "NoCapability", "NoObligation", "TypeOnly"]
    families = sorted(set(fb["family"] for fb in breakdowns))

    fig, ax = plt.subplots(figsize=(10, 6))

    x_labels = families
    bar_width = 0.15
    x = range(len(families))

    for i, variant in enumerate(variants):
        rates = []
        for fam in families:
            match = [fb for fb in breakdowns if fb["family"] == fam and fb["variant"] == variant]
            if match:
                fb = match[0]
                total = fb["pass"] + fb["fail"]
                rate = fb["pass"] / total if total > 0 else 0
            else:
                rate = 0
            rates.append(rate)
        offset = (i - len(variants) / 2 + 0.5) * bar_width
        ax.bar([xi + offset for xi in x], rates, bar_width, label=variant)

    ax.set_xlabel("Benchmark Family")
    ax.set_ylabel("Compile Pass Rate")
    ax.set_title("RQ2: Compile Pass Rate by Ablation Variant")
    ax.set_xticks(list(x))
    ax.set_xticklabels([f.replace("_", "\n") for f in families], fontsize=8)
    ax.legend(fontsize=8)
    ax.set_ylim(0, 1.1)
    plt.tight_layout()
    fig.savefig(path, dpi=150)
    plt.close(fig)


def _render_rq2_false_accepts(path: Path, stats: dict) -> None:
    breakdowns = stats.get("family_breakdowns", [])
    if not breakdowns:
        return

    variants = ["Full", "NoStack", "NoCapability", "NoObligation", "TypeOnly"]
    families = sorted(set(fb["family"] for fb in breakdowns))

    fig, ax = plt.subplots(figsize=(10, 6))

    bar_width = 0.15
    x = range(len(families))

    for i, variant in enumerate(variants):
        counts = []
        for fam in families:
            match = [fb for fb in breakdowns if fb["family"] == fam and fb["variant"] == variant]
            if match:
                counts.append(match[0].get("false_accept", 0))
            else:
                counts.append(0)
        offset = (i - len(variants) / 2 + 0.5) * bar_width
        ax.bar([xi + offset for xi in x], counts, bar_width, label=variant)

    ax.set_xlabel("Benchmark Family")
    ax.set_ylabel("False Accept Count")
    ax.set_title("RQ2: False Accepts by Ablation Variant")
    ax.set_xticks(list(x))
    ax.set_xticklabels([f.replace("_", "\n") for f in families], fontsize=8)
    ax.legend(fontsize=8)
    plt.tight_layout()
    fig.savefig(path, dpi=150)
    plt.close(fig)
