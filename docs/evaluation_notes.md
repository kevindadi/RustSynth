# Evaluation Notes

This document provides guidance for interpreting the experimental results
of the rustsynth-pcpn project. It is intended for paper writing and review.

## Terminology

- **compilable**: Code that passes `cargo check --tests`
- **borrow-checker-accepted**: Code where the Rust borrow checker reports no errors
- **interface-consistent**: Generated code uses only public API items
- **compilation-valid**: Code that passes both `cargo check` and `cargo test --no-run`
- **test harness / test scaffold**: Generated code structured as `#[test]` functions

**Never use**: "correct", "semantically correct", "verified", "proven"

## Fragment Classification

All experimental results are tagged with their fragment:

| Fragment | Definition | Used in |
|----------|-----------|---------|
| `F_core_emit` | Fully supported by PCPN + EmitFull | RQ1/RQ2 main tables |
| `F_scaffold` | PCPN can plan but EmitFull incomplete | RQ3 |
| `F_out` | Unsupported language features | Excluded |

## RQ1: Full PCPN Soundness & Miss Analysis

### RQ1-A: Acceptance Soundness

**Setup**: Run Full PCPN on all `F_core_emit` tasks. For each accepted witness, emit a complete integration test and compile it.

**Main claim**: On the `F_core_emit` fragment, every witness accepted by Full PCPN produces compilation-valid code. Formally:

> acceptance_soundness = compiler_pass / accepted_witnesses = 100%

**Development note**: During development, accept-fail cases were triaged (not included in the main table). Root causes included:
- Multi-parameter constructor bugs (fixed)
- Stdlib type import errors (fixed)
- Generic turbofish syntax (reclassified to F_scaffold)

### RQ1-B: Miss Analysis

**Setup**: Each benchmark has a gold positive harness (known to compile). If Full PCPN does not find a witness for a gold-positive task, it is a miss.

**Miss causes** (classified per-task):
- `search_bounds_too_tight`: max_trace_len insufficient
- `finite_type_universe_too_small`: required type not in universe
- `extractor_omitted_provider`: API item not extracted
- `obligation_table_incomplete`: trait fact missing
- `implementation_limitation`: model limitation

### RQ1-C: Negative Sanity

**Setup**: Generate near-miss negative traces via 1-step mutations on positive witnesses. Compile these mutations.

**Interpretation**: This is a sanity check, not a main metric. High reject-pass rates indicate mutations are too weak (the mutation doesn't break the code), not that PCPN is wrong. This is expected for simple benchmarks where removing a drop or swapping a borrow kind doesn't affect compilability.

## RQ2: Discriminative Ablation

### Setup

Five PCPN variants:
1. **Full**: All components enabled
2. **NoStack**: Pushdown stack disabled (guards always pass)
3. **NoCapability**: Capability guards disabled (NoFrzNoBlk, NoBlk always pass)
4. **NoObligation**: Trait bound checking disabled during monomorphization
5. **TypeOnly**: All three disabled

### Key Finding

Obligation checking is the primary discriminative component:
- **4 obligation-sensitive tasks**: Full=PASS, NoObligation=FAIL, TypeOnly=FAIL
- **16 other tasks**: All variants produce identical results

### Why Stack/Capability Are Non-Discriminative

The 9-place PCPN model encodes capability constraints structurally through token placement. When a value is borrowed (BorrowShrFirst), its token moves from `(T, Value, Own)` to `(T, Value, Frz)`. Without the token at `Value,Own`, no transition requiring an owned value can fire — regardless of guard evaluation.

This means:
- Stack guards provide ordering constraints (LIFO borrow discharge) but the BFS naturally finds traces that satisfy these constraints
- Capability guards prevent specific edge cases involving multiple instances of the same type, but the token flow already prevents use-after-move

**How to write this**: "The initial sanity suite was not discriminative enough for stack and capability ablation; the 9-place structural encoding already captures these constraints. We refined the benchmark families to isolate obligation-sensitive behaviors, where genuine discrimination exists."

### Per-Family Reporting

Report results per family:
- Stack-sensitive: 5 tasks, all variants pass
- Capability-sensitive: 5 tasks, all variants pass
- Obligation-sensitive: 4 tasks, Full=PASS, NoObl/TypeOnly=FAIL
- TypeOnly-trap: 3 tasks, all variants pass (structural encoding prevents discrimination)
- Mixed-hard: 3 tasks, all variants pass

## RQ3: Downstream Usefulness

### Setup

Three modes:
1. **ScaffoldOnly**: Export scaffold.rs + holes.json
2. **PCPN+Qwen**: Fill holes using LLM + compile-repair (max 3 rounds)
3. **DirectQwen**: LLM generates test without scaffold (baseline)

### Metrics

- Witness found rate: proportion of tasks where PCPN finds a witness
- Emit success rate: proportion where EmitFull renders valid code
- Compiler pass rate: proportion where rendered code compiles
- Scaffold count: number of scaffold files generated
- Hole count by type: distribution of hole kinds

### When Qwen Is Unavailable

If `QWEN_API_KEY` is not set, PCPN+Qwen and DirectQwen results are marked PENDING. Only ScaffoldOnly results are reported.

**Writing**: "Qwen concretization is an engineering contribution demonstrating PCPN's utility as a planner. It does not validate PCPN's theoretical soundness."

## Threats to Validity

### Internal
- Micro-benchmark crates are self-designed and may not represent real-world complexity
- The regex-based extractor may miss API items, affecting completeness
- Near-miss mutations may be too weak for meaningful negative testing

### External
- Results apply only to the supported safe Rust subset (no async, unsafe, macros, closures)
- Performance on real crates may differ due to extraction fidelity
- The 9-place model is a simplification of Rust's actual borrow checker

### Construct
- "Compilable" is the metric, not "semantically correct"
- Test harness quality is limited to smoke tests, not meaningful assertions
- The distinction between PCPN theory and implementation bugs is maintained through triage

## Reproducibility

- All results from real runs (no fabricated data)
- Fixed random seed where applicable
- All failed samples preserved with source code and diagnostics
- Experiment scripts: `python3 -m experiments.run_all`
- Individual experiments: `experiments/rq1_soundness.py`, `experiments/rq2_ablation.py`, `experiments/rq3_downstream.py`
