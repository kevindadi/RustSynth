# RustSynth - Pushdown CPN Safe Rust Synthesizer

[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)

RustSynth is a **Pushdown Colored Petri Net (PCPN)** based Safe Rust code snippet synthesizer. It parses public API signatures from rustdoc JSON, constructs a PCPN model with the 9-place ownership system, and synthesizes compilable Safe Rust code through bounded reachability search.

LLMs are effective at assisting program development and generating test cases, but they may not simultaneously guarantee **compilation correctness** and **coverage**. RustSynth addresses this by using formal Petri net semantics to model Rust's ownership and lifetime system, ensuring that every generated code snippet is structurally valid with respect to borrow checking rules.

## Theoretical Foundation

### Pushdown Colored Petri Net (PCPN)

A PCPN extends the classical Colored Petri Net (CPN) with a pushdown stack, enabling the modeling of nested borrow scopes. Formally:

**PCPN = (P, T, A, C, G, I, S)** where:

- **P** (Places): represent typed token containers. Each base type T generates 9 places via the cartesian product `{T, &T, &mut T} x {own, frz, blk}`.
- **T** (Transitions): represent API calls and structural operations (borrow, drop, copy).
- **A** (Arcs): connect places to transitions with optional inscriptions for token transformation.
- **C** (Color sets): token colors carry variable IDs (`vid`), type information, region labels, and borrow provenance.
- **G** (Guards): boolean conditions on transition enabling (e.g., `NoFrzNoBlk`, `PlaceCountRange`, `StackDepthMax`).
- **I** (Inscriptions): optional arc expressions that define token transformations.
- **S** (Stack): a LIFO pushdown stack tracking outstanding borrows with `Freeze`, `Shr`, and `Mut` frames.

### 9-Place Ownership Model

For each base type `T`, the model distinguishes 9 places:

```
             own        frz        blk
  T       [T,own]    [T,frz]    [T,blk]
  &T      [&T,own]   [&T,frz]   [&T,blk]
  &mut T  [&mut T,own] [&mut T,frz] [&mut T,blk]
```

- **own** (owned): the token is fully owned, can be moved or borrowed
- **frz** (frozen): the owner is frozen due to an outstanding shared borrow
- **blk** (blocked): the owner is blocked due to an outstanding mutable borrow

### Token Flow Semantics

Key structural transitions model Rust's borrow checker rules:

| Transition | Input | Output | Stack Effect | Guard |
|---|---|---|---|---|
| `borrow_shr_first(T)` | `[T,own]` | `[T,frz] + [&T,own]` | push Freeze, push Shr | NoFrzNoBlk |
| `borrow_shr_next(T)` | (read `[T,frz]`) | `[&T,own]` | push Shr | NoBlk |
| `end_shr_keep_frz(T)` | `[T,frz] + [&T,own]` | `[T,frz]` | pop Shr | StackTopMatches |
| `end_shr_unfreeze(T)` | `[T,frz] + [&T,own]` | `[T,own]` | pop Shr, pop Freeze | StackTopMatches |
| `borrow_mut(T)` | `[T,own]` | `[T,blk] + [&mut T,own]` | push Mut | NoFrzNoOtherBlk |
| `end_mut(T)` | `[T,blk] + [&mut T,own]` | `[T,own]` | pop Mut | StackTopMatches |
| `drop(T)` | `[T,own]` | (empty) | - | NotBlocked |

## Features

- **9-Place Model**: For each base type T, distinguishes `{T, &T, &mut T} x {own, frz, blk}` = 9 places
- **Pushdown Stack**: Tracks outstanding borrows with LIFO borrow/return semantics
- **Multiple Search Strategies**: BFS (shortest path), DFS (memory efficient), IDDFS (optimal with bounded memory)
- **Multi-Trace Generation**: Collects multiple witness traces in a single search for better test coverage
- **Type Unification**: Supports generic function instantiation with bounds checking
- **Canonicalization**: State normalization via vid/region renaming to prevent infinite state explosion
- **Lifetime Elision**: Implements Rust's 3 lifetime elision rules for automatic lifetime inference
- **Extended Guards**: `PlaceCountRange`, `StackDepthMax`, and composite `And` guards
- **Arc Inscriptions**: Optional token transformation expressions on arcs (Identity, Project, Wrap, Filter)
- **0-ary Producer Detection**: Automatically identifies parameterless const fn as value sources
- **TOML Configuration**: Flexible task specification with goals, bounds, filters, and strategy selection
- **Code Generation**: Translates witness firing sequences into compilable Rust code with `use` imports

## Quick Start

### Prerequisites

- Rust toolchain (1.85+)
- Rust nightly (for generating rustdoc JSON)

### Installation

```bash
git clone https://github.com/example/RustSynth.git
cd RustSynth
cargo build --release
```

### Basic Usage

```bash
# 1. Generate rustdoc JSON for your target crate
cd examples/toy_api
cargo +nightly rustdoc -Z unstable-options --output-format json --lib

# 2. Run the synthesizer
cd ../..
cargo run --release -- synth \
    --doc-json examples/toy_api/target/doc/toy_api.json \
    --task examples/toy_api/task.toml \
    --out synthesized.rs

# 3. Verify generated code compiles
rustc --edition 2021 synthesized.rs --crate-type lib
```

### One-Click Test

```bash
python3 run_tests.py
```

Or using Docker:

```bash
docker build -t RustSynth .
docker run --rm RustSynth
```

## Architecture

```
rustdoc JSON
     |
     v
+-----------------+
|  extract.rs     |  rustdoc JSON -> Bipartite API Graph (Functions <-> Types)
+-----------------+
     |
     v
+-----------------+
|  pcpn.rs        |  API Graph -> 9-Place PCPN Model (monomorphization + structural transitions)
+-----------------+
     |
     v
+-----------------+
|  simulator.rs   |  PCPN -> Bounded Reachability Search (BFS/DFS/IDDFS + canonicalization)
+-----------------+
     |
     v
+-----------------+
|  emitter.rs     |  Witness Trace -> Compilable Safe Rust Code (single or multi-trace)
+-----------------+
```

### Data Flow

1. **Extract** (`extract.rs`): Parses rustdoc JSON `Crate`, extracts public function signatures (parameters, return types, self receivers, lifetime bindings), and builds a bipartite `ApiGraph` of `FunctionNode` <-> `TypeNode` connected by `ApiEdge`.

2. **PCPN Construction** (`pcpn.rs`): Converts `ApiGraph` into a Pushdown CPN. Performs generic monomorphization, creates 9 places per type, generates API transitions with guards, and adds structural transitions for borrow/drop/copy operations.

3. **Simulation** (`simulator.rs`): Performs bounded reachability search on the PCPN. Supports BFS (finds shortest witness), DFS (lower memory), and IDDFS (optimal depth with bounded memory). Uses state canonicalization (vid/region renaming) for visited-set deduplication. Supports multi-trace collection.

4. **Code Emission** (`emitter.rs`): Translates a witness firing sequence into compilable Rust code. Handles variable naming, type annotations, method call syntax, borrow expressions, and drop insertion.

## Task Configuration

Task configuration uses TOML format:

```toml
[inputs]
doc_json = "target/doc/my_crate.json"

[search]
stack_depth = 8           # Maximum borrow stack depth
default_place_bound = 2   # Default token bound per place
max_steps = 100           # Maximum search steps
strategy = "bfs"          # Search strategy: "bfs", "dfs", or "iddfs"
max_traces = 1            # Number of witness traces to collect

[search.place_bounds]
"own_i32" = 3             # Override bound for specific place

[filter]
allow = ["Counter::new", "Counter::inc", "Counter::get"]

[goal]
want = "own i32"          # Goal: obtain an owned i32
count = 1
```

### Search Strategies

| Strategy | Description | Best For |
|----------|-------------|----------|
| `bfs` | Breadth-first search (default) | Finding shortest witness traces |
| `dfs` | Depth-first search | Lower memory usage, deep paths |
| `iddfs` | Iterative deepening DFS | Optimal depth with bounded memory |

### Multi-Trace Mode

Set `max_traces > 1` to collect multiple distinct witness traces. Each trace represents a different valid API call sequence reaching the goal. The emitter generates separate test functions for each trace.

## Commands

| Command | Description |
|---------|-------------|
| `synth` | Run full synthesis pipeline with task config |
| `apigraph` | Generate API Graph (DOT/JSON) |
| `pcpn` | Generate PCPN model (DOT/JSON) |
| `simulate` | Run simulator to find witness (supports `--strategy`) |
| `reachability` | Generate reachability graph |
| `generate` | Full pipeline: PCPN -> simulation -> code |

## Module Overview

| Module | Lines | Description |
|--------|-------|-------------|
| `types.rs` | 617 | 9-place type definitions (TypeForm, Capability, Token, Marking, BorrowStack) |
| `type_model.rs` | 491 | Internal type representation (TypeKey, PassingMode) |
| `config.rs` | 331 | TOML task configuration parsing with strategy and multi-trace support |
| `unify.rs` | 395 | Type unification and completion for generic instantiation |
| `pcpn.rs` | 1165 | Pushdown CPN model construction with extended guards and arc inscriptions |
| `simulator.rs` | 1436 | BFS/DFS/IDDFS reachability search with canonicalization and multi-trace |
| `emitter.rs` | 635 | Witness to Rust code translation (single and multi-trace modes) |
| `extract.rs` | 719 | rustdoc JSON -> API Graph extraction |
| `apigraph.rs` | 649 | API bipartite graph (FunctionNode, TypeNode, ApiEdge) |
| `lifetime_analyzer.rs` | 555 | Lifetime analysis with Rust elision rules |
| `rustdoc_loader.rs` | 25 | rustdoc JSON file loading |
| `main.rs` | 529 | CLI entry point with subcommands |

## Example Output

```rust
//! Generated by RustSynth PCPN Synthesizer

fn main() {
    let mut counter_0: Counter = Counter::new();
    let ref_counter_1 = &counter_0;
    let mut i32_2: i32 = ref_counter_1.get();
    drop(ref_counter_1);
}
```

## Supported Rust Constructs

- Primitive types (i32, u64, bool, etc.)
- User-defined structs
- Generics with bounds (Copy, Clone)
- Shared references (&T)
- Mutable references (&mut T)
- Methods (self, &self, &mut self)
- Free functions
- const fn (as 0-ary producers)
- Lifetime elision (3 rules)

## Limitations

- No associated types / trait impl analysis
- No async/await support
- Simplified outlives checking via stack order
- No higher-kinded types or GATs

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

## References

- Pushdown Colored Petri Net theory
- Rust ownership and borrowing semantics
- rustdoc JSON format specification
- Rust Reference: Lifetime Elision Rules
