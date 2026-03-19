# Benchmark Refinement Process

This document describes the benchmark refinement process for the rustsynth-pcpn project.

---

## 1. Initial Sanity Suite Was Not Discriminative

- **Scale**: 16 benchmarks
- **Finding**: All 5 ablation variants (Full, NoStack, NoCapability, NoObligation, TypeOnly) produced **identical results** on every task

| Variant | Description |
|---------|-------------|
| Full | Complete PCPN model |
| NoStack | Removes stack/scope guards |
| NoCapability | Removes capability guards |
| NoObligation | Removes obligation (trait bound) checks |
| TypeOnly | Type-only checks, no stack/capability/obligation |

---

## 2. Root Cause

**Core insight**: The 9-place PCPN model encodes capability constraints **structurally** through token placement. Guards (e.g., `NoFrzNoBlk`, `StackTopMatches`) are mostly **redundant** given the token flow. Removing guards therefore does not produce invalid traces.

- Token flow itself constrains valid state transitions
- Guards primarily provide pruning/efficiency, not correctness-critical behavior
- The initial benchmarks did not cover scenarios where these guards would distinguish correct from incorrect traces

---

## 3. Discriminative Benchmark Suite

To test discriminative power of each ablation variant, we created **20 discriminative benchmarks** across 5 families:

| Family | Count | Design Goal |
|--------|-------|-------------|
| Stack-sensitive | 5 | Test impact of stack/scope/nesting on traces |
| Capability-sensitive | 5 | Test use-after-move, conflicting borrows, etc. |
| Obligation-sensitive | 4 | Test trait bound checking during monomorphization |
| TypeOnly-trap | 3 | Test "trap" scenarios under type-only checking |
| Mixed-hard | 3 | Hard tasks combining multiple constraints |

---

## 4. Obligation-Sensitive Family Is Discriminative

**Result**: The Obligation-sensitive family is the **only** one that showed discriminative power in experiments.

- **Full PCPN**: Correctly checks trait bounds during monomorphization, filtering invalid instantiations
- **NoObligation / TypeOnly**: Skip this check, accepting wrong type instantiations (e.g., `print_it<bool>` when only `PrintableItem` implements `Printable`)

| Tasks | Full | NoObligation | TypeOnly |
|-------|------|--------------|----------|
| 4/4 | PASS | FAIL | FAIL |

**Conclusion**: Obligation checking is the genuinely discriminative component; removing it leads to false accepts.

---

## 5. Non-Discriminative Families and Why

### 5.1 Stack-Sensitive

- **Observation**: Full and NoStack produce identical results
- **Reason**: BFS finds the shortest valid trace, which typically does not require complex scope nesting; stack guards are not triggered on shortest paths

### 5.2 Capability-Sensitive

- **Observation**: Full and NoCapability produce identical results
- **Reason**: Token flow in the 9-place model already prevents use-after-move and simultaneous conflicting borrows at the **structural** level; capability guards are redundant

### 5.3 TypeOnly-Trap

- **Observation**: Full and TypeOnly produce identical results
- **Reason**: Removing all checks does not yield shorter invalid paths; structural constraints suffice

---

## 6. Conclusion

| Component | Discriminative? | Role |
|-----------|-----------------|------|
| **Obligation checking** | ✅ Yes | Correctness-critical: filters invalid trait instantiations |
| **Stack tracking** | ❌ No (on current benchmarks) | Provides pruning/efficiency; correctness ensured structurally |
| **Capability tracking** | ❌ No (on current benchmarks) | Same as above; token flow guarantees structural correctness |

**Summary**: Obligation checking is the only component that demonstrates discriminative power on the benchmark suite. Stack and capability tracking provide efficiency benefits (pruning) but their correctness is already ensured structurally by the 9-place PCPN model.
