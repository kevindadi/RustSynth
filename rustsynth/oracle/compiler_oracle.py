"""
Compiler oracle — two-level compilation checking with diagnostic classification.

Level 1: cargo check --tests --message-format=json
Level 2: cargo test --tests --no-run --message-format=json  (for Level 1 passes)

Diagnostic categories:
  - type_mismatch
  - move_after_use
  - conflicting_borrows
  - lifetime_outlives
  - trait_bound_unsatisfied
  - assoc_type_mismatch
  - unresolved_import
  - emitter_bug
  - unsupported_feature
  - other
"""

from __future__ import annotations

import json
import subprocess
import logging
import re
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Optional

logger = logging.getLogger(__name__)


class DiagnosticCategory(Enum):
    TypeMismatch = "type_mismatch"
    MoveAfterUse = "move_after_use"
    ConflictingBorrows = "conflicting_borrows"
    LifetimeOutlives = "lifetime_outlives"
    TraitBoundUnsatisfied = "trait_bound_unsatisfied"
    AssocTypeMismatch = "assoc_type_mismatch"
    UnresolvedImport = "unresolved_import"
    EmitterBug = "emitter_bug"
    UnsupportedFeature = "unsupported_feature"
    Other = "other"
    Pass = "pass"


@dataclass
class CompileResult:
    success: bool
    level: int  # 1 or 2
    errors: list[DiagnosticInfo] = field(default_factory=list)
    raw_stderr: str = ""
    raw_json: list[dict] = field(default_factory=list)

    @property
    def primary_category(self) -> DiagnosticCategory:
        if self.success:
            return DiagnosticCategory.Pass
        if self.errors:
            return self.errors[0].category
        return DiagnosticCategory.Other


@dataclass
class DiagnosticInfo:
    category: DiagnosticCategory
    message: str
    code: Optional[str] = None
    spans: list[dict] = field(default_factory=list)


def compile_check(crate_path: Path, test_file: Optional[Path] = None) -> CompileResult:
    """Two-level compilation check.

    Level 1: cargo check --tests
    Level 2: cargo test --tests --no-run  (only if level 1 passes)
    """
    level1 = _run_cargo_check(crate_path)
    if not level1.success:
        return level1

    level2 = _run_cargo_test_no_run(crate_path)
    return level2


def _run_cargo_check(crate_path: Path) -> CompileResult:
    cmd = ["cargo", "check", "--tests", "--message-format=json"]
    return _run_cargo_cmd(cmd, crate_path, level=1)


def _run_cargo_test_no_run(crate_path: Path) -> CompileResult:
    cmd = ["cargo", "test", "--tests", "--no-run", "--message-format=json"]
    return _run_cargo_cmd(cmd, crate_path, level=2)


def _run_cargo_cmd(cmd: list[str], crate_path: Path, level: int) -> CompileResult:
    try:
        result = subprocess.run(
            cmd, cwd=str(crate_path),
            capture_output=True, text=True, timeout=120,
        )
    except subprocess.TimeoutExpired:
        return CompileResult(
            success=False, level=level,
            errors=[DiagnosticInfo(
                category=DiagnosticCategory.Other,
                message="Compilation timed out",
            )],
        )

    messages = []
    errors = []
    for line in result.stdout.split("\n"):
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
            messages.append(msg)
            if msg.get("reason") == "compiler-message":
                diag = msg.get("message", {})
                if diag.get("level") == "error":
                    errors.append(_classify_diagnostic(diag))
        except json.JSONDecodeError:
            continue

    success = result.returncode == 0 and len(errors) == 0

    return CompileResult(
        success=success,
        level=level,
        errors=errors,
        raw_stderr=result.stderr,
        raw_json=messages,
    )


def _classify_diagnostic(diag: dict) -> DiagnosticInfo:
    """Classify a compiler diagnostic into a category."""
    message = diag.get("message", "")
    code_info = diag.get("code")
    code = code_info.get("code", "") if isinstance(code_info, dict) else ""
    spans = diag.get("spans", [])

    category = _categorize_error(message, code)

    return DiagnosticInfo(
        category=category,
        message=message,
        code=code,
        spans=spans,
    )


def _categorize_error(message: str, code: str) -> DiagnosticCategory:
    msg_lower = message.lower()

    if code == "E0308" or "mismatched types" in msg_lower:
        return DiagnosticCategory.TypeMismatch

    if code == "E0382" or "use of moved value" in msg_lower or "value used here after move" in msg_lower:
        return DiagnosticCategory.MoveAfterUse

    if code == "E0502" or code == "E0499" or "cannot borrow" in msg_lower:
        return DiagnosticCategory.ConflictingBorrows

    if code == "E0597" or code == "E0505" or "does not live long enough" in msg_lower or "borrowed value" in msg_lower:
        return DiagnosticCategory.LifetimeOutlives

    if code == "E0277" or "the trait bound" in msg_lower or "doesn't implement" in msg_lower:
        return DiagnosticCategory.TraitBoundUnsatisfied

    if "associated type" in msg_lower:
        return DiagnosticCategory.AssocTypeMismatch

    if code == "E0432" or code == "E0433" or "unresolved" in msg_lower or "not found" in msg_lower:
        return DiagnosticCategory.UnresolvedImport

    if "cannot find" in msg_lower or "no method named" in msg_lower:
        return DiagnosticCategory.EmitterBug

    return DiagnosticCategory.Other


def check_generated_file(
    crate_path: Path,
    test_file_content: str,
    test_filename: str = "generated_test.rs",
) -> CompileResult:
    """Write a generated test file into the crate's tests/ dir and compile-check."""
    tests_dir = crate_path / "tests"
    tests_dir.mkdir(exist_ok=True)

    test_path = tests_dir / test_filename
    test_path.write_text(test_file_content)

    try:
        result = compile_check(crate_path, test_path)
    finally:
        if test_path.exists():
            test_path.unlink()

    return result
