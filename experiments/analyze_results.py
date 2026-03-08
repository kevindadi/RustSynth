"""
Analyze raw experiment results and generate CSV files.
"""

from __future__ import annotations

import csv
import json
import logging
from pathlib import Path

logger = logging.getLogger(__name__)


def generate_csvs(results_dir: Path) -> None:
    csv_dir = results_dir / "csv"
    csv_dir.mkdir(parents=True, exist_ok=True)

    _generate_rq1_csv(results_dir, csv_dir)
    _generate_rq2_csv(results_dir, csv_dir)
    _generate_rq3_csv(results_dir, csv_dir)


def _generate_rq1_csv(results_dir: Path, csv_dir: Path) -> None:
    raw_path = results_dir / "raw" / "rq1_samples.jsonl"
    if not raw_path.exists():
        logger.warning("RQ1 raw data not found")
        return

    samples = []
    with open(raw_path) as f:
        for line in f:
            if line.strip():
                samples.append(json.loads(line))

    out_path = csv_dir / "rq1_samples.csv"
    with open(out_path, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=[
            "task_id", "sample_id", "pcpn_accepts", "compiler_passes",
            "mutation_type", "error_category", "is_emitter_failure",
        ])
        writer.writeheader()
        for s in samples:
            writer.writerow(s)

    summary_path = results_dir / "rq1_summary.json"
    if summary_path.exists():
        summary = json.loads(summary_path.read_text())
        cm_path = csv_dir / "rq1_confusion_matrix.csv"
        with open(cm_path, "w", newline="") as f:
            writer = csv.writer(f)
            writer.writerow(["metric", "value"])
            for k, v in summary.items():
                writer.writerow([k, v])

    by_category: dict[str, int] = {}
    for s in samples:
        cat = s.get("error_category", "other")
        by_category[cat] = by_category.get(cat, 0) + 1

    cat_path = csv_dir / "rq1_error_categories.csv"
    with open(cat_path, "w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(["category", "count"])
        for cat, count in sorted(by_category.items()):
            writer.writerow([cat, count])

    logger.info("RQ1 CSVs written to %s", csv_dir)


def _generate_rq2_csv(results_dir: Path, csv_dir: Path) -> None:
    raw_path = results_dir / "raw" / "rq2_ablation.jsonl"
    if not raw_path.exists():
        logger.warning("RQ2 raw data not found")
        return

    rows = []
    with open(raw_path) as f:
        for line in f:
            if line.strip():
                rows.append(json.loads(line))

    out_path = csv_dir / "rq2_ablation.csv"
    with open(out_path, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=[
            "variant", "task_id", "witnesses_found", "states_explored",
            "search_time_ms", "compile_pass", "compile_fail",
            "false_accept", "compile_pass_rate",
        ])
        writer.writeheader()
        for r in rows:
            writer.writerow(r)

    logger.info("RQ2 CSVs written to %s", csv_dir)


def _generate_rq3_csv(results_dir: Path, csv_dir: Path) -> None:
    raw_path = results_dir / "raw" / "rq3_tasks.jsonl"
    if not raw_path.exists():
        logger.warning("RQ3 raw data not found")
        return

    rows = []
    with open(raw_path) as f:
        for line in f:
            if line.strip():
                rows.append(json.loads(line))

    out_path = csv_dir / "rq3_downstream.csv"
    with open(out_path, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=[
            "task_id", "witness_found", "emit_success", "compiler_pass",
            "scaffold_generated", "hole_count", "error_info",
        ])
        writer.writeheader()
        for r in rows:
            row = {k: r.get(k, "") for k in writer.fieldnames}
            writer.writerow(row)

    logger.info("RQ3 CSVs written to %s", csv_dir)
