# Assumptions and Deviations from Paper

This document records every point where the rustsynth-pcpn implementation
deviates from or approximates the definitions in the paper
*"A Synthesis Method of Safe Rust Code Based on Pushdown Colored Petri Nets"*.

## 1. Type Universe

**Paper**: Infinite type universe with full Rust type system.
**Implementation**: Finite ground type universe (`hat{Ty}`) restricted to:
- Primitive types present in the crate's API
- Named struct types extracted from the crate
- Simple tuples (limited depth)
- No closures, trait objects, or dynamically-sized types (except `str` via `&str` literal)

**Impact on RQ1/RQ2/RQ3**: The finite type universe causes conservative misses (false negatives) when the required type is not in the universe. This is tracked as `finite_type_universe_too_small` in miss analysis.

## 2. Monomorphization

**Paper**: Full unification-based instantiation.
**Implementation**: Cartesian product enumeration over the finite type universe, filtered by:
- `Copy`/`Clone` trait checking via fact table
- Custom trait bounds via `ImplFact` table (when `check_obligations=True`)
- `Default` trait checking for primitive types

**Impact**: Over-generates instantiations for types without bounds. Under-generates when the fact table is incomplete. Ablation of obligation checking (`NoObligation`) disables trait bound filtering, producing invalid monomorphic instances.

## 3. Lifetime Modeling

**Paper**: Explicit lifetime parameters and region-based reasoning.
**Implementation**: Simplified stack-based lifetime tracking:
- No named lifetime parameters
- Borrow lifetimes implied by pushdown stack frames
- `outlives` checking reduced to stack order comparison
- No higher-ranked trait bounds (HRTB)

**Impact**: Cannot synthesize code requiring explicit lifetime annotations. Conservative: rejects valid programs that need lifetime threading. Tracked as `implementation_limitation` in miss analysis.

## 4. 9-Place Capability Model

**Paper**: Per-token capability tracking with own/frz/blk states.
**Implementation**: 9 places per ground type: (Value/RefShr/RefMut) × (Own/Frz/Blk).
Capability transitions:
- `BorrowShrFirst`: Value,Own → RefShr,Own + Value,Frz
- `BorrowMut`: Value,Own → RefMut,Own + Value,Blk
- `EndBorrowShrUnfreeze`: RefShr,Own + Value,Frz → Value,Own
- `EndBorrowMut`: RefMut,Own + Value,Blk → Value,Own

**Critical finding**: The 9-place structural model already encodes capability constraints through token placement. Guards (`NoFrzNoBlk`, `NoBlk`) are largely redundant — the token flow prevents most invalid states structurally. This means the `NoCapability` ablation variant does not produce different results from `Full` for most benchmarks.

**Impact on RQ2**: Capability ablation is non-discriminative. This is documented in `docs/benchmark_refinement.md` and acknowledged in the evaluation.

## 5. Obligation Checking

**Paper**: Full trait entailment, associated type equality, outlives reasoning.
**Implementation**: 
- Trait bound checking via `ImplFact` entries extracted from source
- Associated type resolution via `AssocFact` entries
- Simplified `outlives` via stack depth comparison
- Obligation checking gates monomorphization (not transition firing)

**Key design choice**: Obligation checking is applied at PCPN construction time (during `PCPN.from_sigma`), not at search time. This means the PCPN for Full PCPN has fewer transitions than NoObligation, because invalid monomorphizations are filtered out during construction.

**Impact on RQ2**: This is the primary discriminative axis. NoObligation allows invalid trait instantiations, producing false accepts.

## 6. CloseStack

**Paper**: Deterministic cleanup of remaining borrows after witness extraction.
**Implementation**: Greedy close_stack that iterates EndBorrow and Drop transitions until the stack is empty and all tokens are consumed. Uses the same guard evaluation as the main search.

**Deviation**: The greedy strategy may not find the optimal closing sequence in all cases.

## 7. Extractor Fidelity

**Paper**: Assumes complete compiler-level extraction.
**Implementation**: Two-tier extraction:
1. Rustdoc JSON (preferred but often unavailable for nightly features)
2. Regex-based source parsing (conservative fallback)

The regex fallback may miss:
- Complex generic bounds with where clauses
- Trait implementations in separate files
- Re-exports and glob imports
- Conditional compilation (`#[cfg(...)]`)

**Impact**: Some callable items may be missing from Sigma(C), causing false negatives.

## 8. Emitter Limitations

**Paper**: Assumes the emitter can faithfully render any witness trace.
**Implementation**: EmitFull has known limitations:
- No turbofish syntax for generic type parameters
- No closure/async rendering
- Limited pattern matching
- String literal handling for `&str` via special provider

Items that exceed emitter capability are classified as `F_scaffold` (not `F_core_emit`) and excluded from RQ1/RQ2 main tables.

## 9. Fragment Classification

**Paper**: No explicit fragment system.
**Implementation**: Three-tier classification:
- `F_core_emit`: Fully supported, used in RQ1/RQ2 main tables
- `F_scaffold`: PCPN can plan but EmitFull can't fully render
- `F_out`: Unsupported features, excluded from conclusions

**Rationale**: This ensures that the acceptance soundness claim (0 accept-fail) applies only to the supported fragment, not to the entire Rust language.

## 10. Search Bounds

Default values:
- `max_trace_len = 6` (increased to 8 for discriminative suite)
- `stack_depth = 4`
- `token_per_place = 3`
- `max_witnesses = 5`

**Impact**: Search bounds cause false negatives for tasks requiring longer traces or deeper nesting. These are classified as `search_bounds_too_tight` in miss analysis.

## 11. Near-Miss Mutations

**Paper**: Not specified.
**Implementation**: 6 mutation strategies for RQ1-C negative sanity:
- `delete_drop`: Remove a necessary drop
- `swap_borrow_kind`: Change shared↔mutable borrow
- `reuse_moved`: Attempt to use a moved value
- `scramble_discharge`: Reorder borrow discharge
- `delete_end_borrow`: Skip ending a borrow
- `duplicate_mut_borrow`: Create conflicting mutable borrows

**Limitation**: Many mutations produce code that still compiles (62/71 in current run), indicating the mutations are too weak or the generated code is too simple to break. This is acknowledged in the evaluation.
