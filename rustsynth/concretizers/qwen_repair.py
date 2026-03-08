"""
Compile-repair loop using Qwen to fill holes and fix compilation errors.

Constraints:
  - Qwen can only fill holes, write helper stubs, and add necessary imports
  - Qwen cannot rewrite the PCPN scaffold structure
  - Maximum 3 repair rounds by default
  - All prompts, responses, and diagnostics are logged
"""

from __future__ import annotations

import json
import logging
import re
from pathlib import Path
from typing import Optional

logger = logging.getLogger(__name__)


def compile_repair_loop(
    adapter,
    scaffold_path: Path,
    holes_path: Path,
    crate_path: Path,
    sigma,
    max_rounds: int = 3,
) -> tuple[str, int, bool]:
    """
    Run the compile-repair loop.

    Returns: (final_code, repair_rounds_used, success)
    """
    from rustsynth.oracle.compiler_oracle import check_generated_file

    scaffold = scaffold_path.read_text()
    holes = json.loads(holes_path.read_text()) if holes_path.exists() else []

    prompt = _build_fill_prompt(scaffold, holes, sigma)
    filled_code = adapter.complete(prompt)
    filled_code = _extract_code(filled_code)

    cr = check_generated_file(crate_path, filled_code, "test_qwen_filled.rs")
    if cr.success:
        return filled_code, 0, True

    current_code = filled_code
    for round_num in range(1, max_rounds + 1):
        errors = [e.message for e in cr.errors[:10]]
        repair_prompt = _build_repair_prompt(current_code, errors, sigma)
        repaired = adapter.complete(repair_prompt)
        repaired = _extract_code(repaired)

        cr = check_generated_file(crate_path, repaired, f"test_qwen_repair_{round_num}.rs")
        current_code = repaired
        if cr.success:
            return current_code, round_num, True

    return current_code, max_rounds, False


def _build_fill_prompt(scaffold: str, holes: list, sigma) -> str:
    api_summary = _api_summary(sigma)
    holes_desc = json.dumps(holes, indent=2)[:1500] if holes else "No explicit holes"

    return (
        "Fill in the holes in this Rust test scaffold. "
        "You must ONLY fill the marked hole regions and add necessary imports. "
        "Do NOT restructure the test or change the overall flow.\n\n"
        f"## API Summary\n```\n{api_summary}\n```\n\n"
        f"## Scaffold\n```rust\n{scaffold}\n```\n\n"
        f"## Holes\n```json\n{holes_desc}\n```\n\n"
        "Output ONLY the complete Rust file with holes filled. No explanation."
    )


def _build_repair_prompt(code: str, errors: list[str], sigma) -> str:
    api_summary = _api_summary(sigma)
    error_text = "\n".join(f"- {e}" for e in errors)

    return (
        "Fix the following Rust compilation errors. "
        "Make MINIMAL changes — only fix the errors, do not restructure.\n\n"
        f"## API Summary\n```\n{api_summary}\n```\n\n"
        f"## Current Code\n```rust\n{code}\n```\n\n"
        f"## Compiler Errors\n{error_text}\n\n"
        "Output ONLY the fixed Rust file. No explanation."
    )


def _api_summary(sigma) -> str:
    lines = []
    for c in sigma.callables[:20]:
        if c.path.startswith("__literal_"):
            continue
        params = ", ".join(f"{p.name}: {p.ty.short_name()}" for p in c.params)
        ret = f" -> {c.return_type.short_name()}" if c.return_type else ""
        lines.append(f"fn {c.path}({params}){ret}")
    return "\n".join(lines)


def _extract_code(response: str) -> str:
    """Extract Rust code from a markdown code block if present."""
    m = re.search(r"```(?:rust)?\s*\n(.*?)```", response, re.DOTALL)
    if m:
        return m.group(1).strip()
    return response.strip()
