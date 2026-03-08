# rustsynth-pcpn (v2)

Safe Rust code synthesis via Pushdown Colored Petri Nets (PCPN).

A research prototype implementing the paper *"A Synthesis Method of Safe Rust Code Based on Pushdown Colored Petri Nets"*. This tool extracts public API signatures from Rust crates, constructs a bounded PCPN model, performs reachability search to find valid API call sequences, and generates compilation-valid integration test harnesses.

**Important**: Generated code is *compilation-valid* and *borrow-checker-accepted*, not *semantically correct*. See [docs/evaluation_notes.md](docs/evaluation_notes.md) for precise claims.

## Environment Setup

```bash
# Requires Python 3.11+ and Rust (stable toolchain)
pip3 install matplotlib

# Optional: nightly toolchain for rustdoc JSON extraction
rustup toolchain install nightly
```

## Quick Start

### 1. Run all experiments

```bash
python3 -m experiments.run_all
```

### 2. Run a single task

```python
from rustsynth.extractor.syn_fallback import parse_rust_source
from rustsynth.extractor.sigma import _inject_primitive_providers
from rustsynth.core.pcpn import PCPN
from rustsynth.core.search import bounded_reachability
from rustsynth.ir.plan_trace import PlanTrace
from rustsynth.ir.snippet_ir import lower_to_snippet
from rustsynth.emit.emit_full import render_full
from pathlib import Path
import json

# Extract
src = Path("benchmarks/bench_move_copy/src/lib.rs").read_text()
sigma = parse_rust_source(src, "bench_move_copy")
_inject_primitive_providers(sigma)

# Build PCPN & Search
pcpn = PCPN.from_sigma(sigma)
task = json.loads(Path("benchmarks/tasks/bench_move_copy.json").read_text())
result = bounded_reachability(pcpn, task)
print(f"Found {len(result.witnesses)} witnesses")

# Emit
if result.witnesses:
    plan = PlanTrace.from_witness(result.witnesses[0], task_id="bench_move_copy")
    snippet = lower_to_snippet(plan, sigma)
    code = render_full(snippet, "bench_move_copy", crate_name="bench_move_copy")
    print(code)
```

### 3. Generate paper tables and figures

After running experiments, find outputs in:

- `results/csv/` — CSV data files
- `results/tables/` — LaTeX table sources (`.tex`)
- `results/figures/` — PNG figures
- `results/summary.md` — Human-readable summary

## Architecture

```
Single PCPN Backend → Two Output Adapters → Optional Qwen Concretizer

rustsynth/
├── extractor/         # Sigma(C) extraction from Rust crates
│   ├── cargo_meta.py     # cargo metadata for package discovery
│   ├── rustdoc_parser.py # primary: rustdoc JSON parsing
│   ├── syn_fallback.py   # fallback: regex-based source parsing
│   └── sigma.py          # orchestration + primitive injection
├── core/              # PCPN model and search
│   ├── types.py          # GroundType, TypeForm, Capability, Token, Marking, BorrowStack
│   ├── env.py            # SigmaC, CallableItem, ImplFact, AssocFact
│   ├── pcpn.py           # PCPN construction (places, transitions, guards)
│   ├── structural_rules.py # Borrow/Drop/Copy/Proj/Reborrow transitions
│   ├── api_transitions.py  # API call → transition schema + monomorphization
│   ├── unify.py          # type unification, substitution, obligation filtering
│   ├── obligations.py    # trait/assoc/outlives entailment
│   ├── canon.py          # beta-renaming canonicalization
│   ├── search.py         # BFS bounded reachability + CloseStack
│   └── fragments.py      # F_core_emit / F_scaffold / F_out classification
├── ir/                # Intermediate representations
│   ├── plan_trace.py     # PlanTrace (firing sequence)
│   └── snippet_ir.py     # SnippetIR (structured code IR with holes)
├── emit/              # Output modes
│   ├── emit_full.py      # Mode A: complete #[test] integration test
│   └── export_scaffold.py # Mode B: scaffold.rs + holes.json for LLM
├── oracle/            # Compiler oracle + triage
│   ├── compiler_oracle.py # two-level cargo check/test + diagnostic classification
│   └── triage.py         # unified triage pipeline with final_bucket
└── concretizers/      # Optional Qwen integration (RQ3 only)
    ├── qwen_adapter.py    # OpenAI-compatible API client
    ├── qwen_repair.py     # compile-repair loop
    └── qwen_prompts/      # prompt templates

experiments/
├── run_all.py           # orchestrate all experiments
├── benchmark_refinement.py  # benchmark distinguishability analysis
├── rq1_soundness.py     # RQ1: soundness + miss + negative sanity
├── rq2_ablation.py      # RQ2: discriminative ablation
├── rq3_downstream.py    # RQ3: scaffold + Qwen concretization
├── render_tables.py     # LaTeX table generation
├── render_figures.py    # matplotlib figure generation
└── mutator.py           # near-miss mutation strategies
```

## Pipeline

```
Rust crate → extractor → Sigma(C) → PCPN → BFS search → PlanTrace → SnippetIR
                                                                         ↓
                                                        emit_full.py  ──→  tests/generated/*.rs
                                                        export_scaffold.py → scaffold.rs + holes.json
                                                                                    ↓ (optional)
                                                                            Qwen concretizer
```

## Supported Rust Subset

**Supported**:
- Public free functions and inherent methods
- Shared borrows (`&T`) and mutable borrows (`&mut T`)
- Reborrow (`&mut T` → `&T`)
- Explicit `drop()`
- Block scopes for ending borrows
- Field projection on visible fields
- Trait bounds (checked via fact table)
- Associated type equalities (finite fact table)
- Simple tuples and nominal structs
- `Copy` / `Clone` facts

**Not supported**: unsafe, async/await, macros, HRTB, specialization, GAT, const generics, closures as synthesis targets, trait object dispatch.

## Fragment Classification

| Fragment | Definition | Used in |
|----------|-----------|---------|
| `F_core_emit` | Fully supported by PCPN + EmitFull | RQ1/RQ2 main tables |
| `F_scaffold` | PCPN can plan but EmitFull incomplete | RQ3 |
| `F_out` | Unsupported language features | Excluded from conclusions |

Results in RQ1/RQ2 main tables are **only** from `F_core_emit`. This ensures the acceptance soundness claim applies only to the supported fragment.

## Experiments

### RQ1: Acceptance Soundness & Miss Analysis

- **RQ1-A**: Verifies 0 accept-fail cases for Full PCPN on `F_core_emit`
- **RQ1-B**: Analyzes misses (gold-positive tasks where PCPN finds no witness)
- **RQ1-C**: Negative sanity via near-miss mutations

### RQ2: Discriminative Ablation

Compares 5 variants on a purpose-built discriminative suite:
- **Full PCPN**: All components enabled
- **NoStack**: Pushdown stack disabled
- **NoCapability**: Capability guards disabled
- **NoObligation**: Trait bound checking disabled
- **TypeOnly**: All three disabled

Key finding: obligation checking is the primary discriminative component.

### RQ3: Downstream Usefulness

Three modes: ScaffoldOnly, PCPN+Qwen, DirectQwen (baseline).
Qwen is a *concretizer*, not a *planner*.

## Qwen Configuration

```bash
# Install DashScope SDK
pip install dashscope

# Set API key (required for RQ3 Qwen experiments)
export DASHSCOPE_API_KEY="sk-xxxxxxxxxxxxxxxx"

# Optional: specify model (default: qwen-plus)
export QWEN_MODEL="qwen-plus"
```

See [docs/qwen_protocol.md](docs/qwen_protocol.md) for full protocol.

## Default Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `max_trace_len` | 6 (8 for disc.) | Maximum firing sequence length |
| `stack_depth` | 4 | Maximum borrow stack depth |
| `token_per_place` | 3 | Maximum tokens per PCPN place |
| `max_witnesses` | 5 | Maximum witness traces to collect |
| `qwen_repair_rounds` | 3 | Maximum compile-repair rounds |

## Benchmark Suite

- **Sanity suite** (16 crates): Basic patterns — move, borrow, drop, field projection, etc.
- **Discriminative suite** (20 crates): Purpose-built for ablation differentiation
  - Stack-sensitive (5): nested scopes, reborrow depth, discharge order
  - Capability-sensitive (5): move vs copy, freeze/unfreeze, aliasing
  - Obligation-sensitive (4): trait-gated functions, custom return types
  - TypeOnly-trap (3): borrow kind confusion, dead providers
  - Mixed-hard (3): combined patterns, longer chains

Each benchmark includes `Cargo.toml`, `src/lib.rs`, `tasks/*.json`, and `gold/*.rs`.

## Key Results (v2)

| Metric | Value |
|--------|-------|
| RQ1-A: Acceptance soundness | **100%** (0 accept-fail on F_core_emit) |
| RQ1-B: Miss rate | 0/35 |
| RQ2: Full PCPN pass rate | 20/20 |
| RQ2: NoObligation pass rate | 16/20 (4 false accepts) |
| RQ3: Scaffolds generated | 35 |
| RQ3: Qwen | PENDING (API key not set) |

## Documentation

- [docs/assumptions.md](docs/assumptions.md) — Deviations from paper definitions
- [docs/evaluation_notes.md](docs/evaluation_notes.md) — Metrics, threats, writing guide
- [docs/benchmark_refinement.md](docs/benchmark_refinement.md) — Refinement process
- [docs/qwen_protocol.md](docs/qwen_protocol.md) — Qwen integration protocol

## Result Interpretation

- **Main theory results**: RQ1-A soundness + RQ2 obligation discrimination on `F_core_emit`
- **Engineering results**: RQ3 scaffold generation + Qwen integration
- **Honest limitations**: Stack/capability ablation non-discriminative (structural encoding), near-miss mutations too weak

## License

MIT / Apache-2.0 dual license
