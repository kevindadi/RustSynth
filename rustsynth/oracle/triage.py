"""
Unified triage pipeline for classifying experiment anomalies.

Every sample that passes through the pipeline gets a TriageRecord
with a final_bucket classification used to determine which results
appear in the main experiment tables.
"""

from __future__ import annotations

import csv
import json
import logging
from dataclasses import dataclass, field, asdict
from enum import Enum
from pathlib import Path
from typing import Optional

logger = logging.getLogger(__name__)


class FinalBucket(Enum):
    CORE_VALID = "core_valid"
    MISS = "miss"
    EMITTER_FAILURE = "emitter_failure"
    EXTRACTOR_FAILURE = "extractor_failure"
    SCHEMA_BUG = "schema_bug"
    UNSUPPORTED_FEATURE = "unsupported_feature"
    SCAFFOLD_ONLY = "scaffold_only"
    QWEN_FAILURE = "qwen_failure"
    NOT_RUN = "not_run"


@dataclass
class TriageRecord:
    task_id: str
    fragment: str  # "core_emit" | "scaffold" | "out"
    variant: str   # "Full" | "NoStack" | "NoCapability" | "NoObligation" | "TypeOnly" | "Qwen"
    pcpn_decision: str  # "accept" | "reject" | "no_witness"
    emit_status: str    # "success" | "failure" | "not_attempted"
    compiler_status: str  # "pass" | "fail" | "not_run"
    root_cause: str = ""
    compiler_errors: list[str] = field(default_factory=list)
    fixed_or_not: str = "not_fixed"
    final_bucket: str = ""

    def classify(self) -> None:
        """Auto-classify into final_bucket based on status fields."""
        if self.emit_status == "not_attempted" and self.compiler_status == "not_run":
            self.final_bucket = FinalBucket.NOT_RUN.value
            return

        if self.pcpn_decision == "accept" and self.compiler_status == "pass":
            self.final_bucket = FinalBucket.CORE_VALID.value
            return

        if self.pcpn_decision in ("reject", "no_witness"):
            self.final_bucket = FinalBucket.MISS.value
            return

        if self.pcpn_decision == "accept" and self.emit_status == "failure":
            self.final_bucket = FinalBucket.EMITTER_FAILURE.value
            return

        if self.pcpn_decision == "accept" and self.compiler_status == "fail":
            self._classify_accept_fail()
            return

        self.final_bucket = FinalBucket.NOT_RUN.value

    def _classify_accept_fail(self) -> None:
        """Classify accept-fail cases based on compiler errors."""
        err_text = " ".join(self.compiler_errors).lower()

        if "unresolved import" in err_text or "cannot find" in err_text:
            self.final_bucket = FinalBucket.EMITTER_FAILURE.value
            self.root_cause = "emitter import/path bug"
            return

        if "macro" in err_text or "async" in err_text or "unsafe" in err_text:
            self.final_bucket = FinalBucket.UNSUPPORTED_FEATURE.value
            self.root_cause = "unsupported language feature"
            return

        if "trait" in err_text and "not implemented" in err_text:
            if self.fragment == "core_emit":
                self.final_bucket = FinalBucket.SCHEMA_BUG.value
                self.root_cause = "obligation fact table incomplete"
            else:
                self.final_bucket = FinalBucket.SCAFFOLD_ONLY.value
            return

        if any(kw in err_text for kw in ["borrow", "moved", "lifetime", "outlives"]):
            if self.fragment == "core_emit":
                self.final_bucket = FinalBucket.SCHEMA_BUG.value
                self.root_cause = "PCPN structural rule or stack logic bug"
            else:
                self.final_bucket = FinalBucket.SCAFFOLD_ONLY.value
            return

        if "type" in err_text and "mismatch" in err_text:
            self.final_bucket = FinalBucket.EXTRACTOR_FAILURE.value
            self.root_cause = "extractor type resolution error"
            return

        self.final_bucket = FinalBucket.EMITTER_FAILURE.value
        self.root_cause = "unclassified accept-fail (defaulting to emitter)"


class TriagePipeline:
    """Collects triage records and outputs CSV reports."""

    def __init__(self) -> None:
        self.records: list[TriageRecord] = []

    def add(self, record: TriageRecord) -> None:
        if not record.final_bucket:
            record.classify()
        self.records.append(record)

    def create_record(
        self,
        task_id: str,
        fragment: str,
        variant: str,
        pcpn_decision: str,
        emit_status: str,
        compiler_status: str,
        compiler_errors: Optional[list[str]] = None,
    ) -> TriageRecord:
        rec = TriageRecord(
            task_id=task_id,
            fragment=fragment,
            variant=variant,
            pcpn_decision=pcpn_decision,
            emit_status=emit_status,
            compiler_status=compiler_status,
            compiler_errors=compiler_errors or [],
        )
        rec.classify()
        self.add(rec)
        return rec

    def get_accept_fail_triage(self) -> list[TriageRecord]:
        return [
            r for r in self.records
            if r.pcpn_decision == "accept" and r.compiler_status == "fail"
        ]

    def get_core_emit_accept_fail(self) -> list[TriageRecord]:
        return [
            r for r in self.records
            if r.fragment == "core_emit"
            and r.pcpn_decision == "accept"
            and r.compiler_status == "fail"
            and r.final_bucket not in (
                FinalBucket.EMITTER_FAILURE.value,
                FinalBucket.UNSUPPORTED_FEATURE.value,
            )
        ]

    def check_soundness(self) -> bool:
        """Return True if no unexplained accept-fail exists on F_core_emit."""
        violations = self.get_core_emit_accept_fail()
        if violations:
            for v in violations:
                logger.error(
                    "SOUNDNESS VIOLATION: task=%s variant=%s bucket=%s cause=%s",
                    v.task_id, v.variant, v.final_bucket, v.root_cause,
                )
        return len(violations) == 0

    def write_csv(self, output_dir: Path) -> None:
        output_dir.mkdir(parents=True, exist_ok=True)

        all_path = output_dir / "triage_all.csv"
        self._write_records(all_path, self.records)

        af_records = self.get_accept_fail_triage()
        if af_records:
            af_path = output_dir / "rq1_accept_fail_triage.csv"
            self._write_records(af_path, af_records)

    def write_jsonl(self, output_path: Path) -> None:
        output_path.parent.mkdir(parents=True, exist_ok=True)
        with open(output_path, "w") as f:
            for r in self.records:
                d = asdict(r)
                f.write(json.dumps(d) + "\n")

    @staticmethod
    def _write_records(path: Path, records: list[TriageRecord]) -> None:
        if not records:
            return
        fieldnames = [
            "task_id", "fragment", "variant", "pcpn_decision",
            "emit_status", "compiler_status", "root_cause",
            "fixed_or_not", "final_bucket",
        ]
        with open(path, "w", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=fieldnames)
            writer.writeheader()
            for r in records:
                row = {k: getattr(r, k) for k in fieldnames}
                writer.writerow(row)

    def summary(self) -> dict:
        """Return summary statistics."""
        from collections import Counter
        bucket_counts = Counter(r.final_bucket for r in self.records)
        fragment_counts = Counter(r.fragment for r in self.records)
        return {
            "total": len(self.records),
            "by_bucket": dict(bucket_counts),
            "by_fragment": dict(fragment_counts),
            "soundness_ok": self.check_soundness(),
        }
