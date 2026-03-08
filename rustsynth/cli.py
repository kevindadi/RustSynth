"""Command-line interface for rustsynth-pcpn."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def cmd_extract(args: argparse.Namespace) -> None:
    from rustsynth.extractor.sigma import build_sigma
    sigma = build_sigma(Path(args.crate_path))
    out = Path(args.output) if args.output else Path("sigma.json")
    out.write_text(json.dumps(sigma.to_dict(), indent=2))
    print(f"Sigma(C) written to {out}  ({len(sigma.callables)} callables)")


def cmd_build(args: argparse.Namespace) -> None:
    from rustsynth.extractor.sigma import build_sigma
    from rustsynth.core.pcpn import PCPN
    sigma = build_sigma(Path(args.crate_path))
    pcpn = PCPN.from_sigma(sigma)
    print(f"PCPN built: {len(pcpn.places)} places, {len(pcpn.transitions)} transitions")
    if args.output:
        Path(args.output).write_text(json.dumps(pcpn.to_dict(), indent=2))


def cmd_synth(args: argparse.Namespace) -> None:
    from rustsynth.extractor.sigma import build_sigma
    from rustsynth.core.pcpn import PCPN
    from rustsynth.core.search import bounded_reachability
    from rustsynth.ir.plan_trace import PlanTrace
    from rustsynth.ir.snippet_ir import lower_to_snippet
    from rustsynth.emit.emit_full import render_full

    task = json.loads(Path(args.task).read_text())
    sigma = build_sigma(Path(args.crate_path))
    pcpn = PCPN.from_sigma(sigma)

    search_cfg = {
        "max_trace_len": task.get("max_trace_len", 6),
        "stack_depth": task.get("bounds", {}).get("stack_depth", 4),
        "token_per_place": task.get("bounds", {}).get("token_per_place", 3),
    }
    result = bounded_reachability(pcpn, task, search_cfg)
    if not result.witnesses:
        print("No witness found.")
        sys.exit(1)

    for i, witness in enumerate(result.witnesses):
        plan = PlanTrace.from_witness(witness)
        snippet = lower_to_snippet(plan, sigma)
        code = render_full(snippet, task["task_id"], crate_name=task.get("crate_name"))
        out_dir = Path(args.out_dir) if args.out_dir else Path("tests/generated")
        out_dir.mkdir(parents=True, exist_ok=True)
        out_path = out_dir / f"{task['task_id']}_{i}.rs"
        out_path.write_text(code)
        print(f"Wrote {out_path}")


def cmd_run_experiments(args: argparse.Namespace) -> None:
    from experiments.run_all import run_all
    run_all(Path(args.benchmarks_dir) if args.benchmarks_dir else Path("benchmarks"))


def main() -> None:
    parser = argparse.ArgumentParser(prog="rustsynth", description="PCPN-based safe Rust synthesizer")
    sub = parser.add_subparsers(dest="command")

    p_ext = sub.add_parser("extract", help="Extract Sigma(C) from a Rust crate")
    p_ext.add_argument("crate_path", help="Path to the Rust crate root")
    p_ext.add_argument("-o", "--output", help="Output JSON path")

    p_bld = sub.add_parser("build", help="Build PCPN from a crate")
    p_bld.add_argument("crate_path")
    p_bld.add_argument("-o", "--output")

    p_syn = sub.add_parser("synth", help="Synthesize code for a task")
    p_syn.add_argument("crate_path")
    p_syn.add_argument("task", help="Path to task.json")
    p_syn.add_argument("--out-dir", help="Output directory for generated tests")

    p_exp = sub.add_parser("experiments", help="Run all experiments")
    p_exp.add_argument("--benchmarks-dir")

    args = parser.parse_args()
    if args.command == "extract":
        cmd_extract(args)
    elif args.command == "build":
        cmd_build(args)
    elif args.command == "synth":
        cmd_synth(args)
    elif args.command == "experiments":
        cmd_run_experiments(args)
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
