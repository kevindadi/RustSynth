# SyPetype - Rust Ownership & Lifetime Petri Net Modeling Tool

[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**SyPetype** is a formal modeling tool that converts Rust APIs into **Pushdown Colored Petri Nets (PCPN)** to analyze ownership, borrowing, and lifetime semantics. It automatically generates API graphs, PCPN models, and reachability graphs from Rust documentation JSON.

## ✨ Features

### Core Capabilities
- 🔍 **API Graph Generation**: Bipartite graph representation of Rust APIs with ownership annotations
- 🎯 **PCPN Conversion**: Translates ownership/borrowing into formal Petri net model
- 📊 **Reachability Analysis**: Generates state space graph showing all possible execution paths
- 🔐 **Lifetime Tracking**: Full token-level lifetime management with automatic stack operations
- ✅ **Guard Conditions**: Enforces Rust's borrowing rules at token granularity

### Advanced Features
- **Token Instance Tracking**: Each resource instance has unique ID and borrowing history
- **Reference Levels**: Supports multi-level references (`T`, `&T`, `&&T`, ...)
- **Automatic Lifetime Binding**: API calls automatically push/pop lifetime frames
- **Token-Level Guards**: Precise blocking checks (not just type-level)
- **Copy Type Semantics**: Automatic duplication for `Copy` types

## 🚀 Quick Start

### Prerequisites
```bash
# Rust nightly toolchain (required for rustdoc JSON)
rustup toolchain install nightly

# Graphviz (optional, for visualization)
# macOS
brew install graphviz
# Linux
sudo apt install graphviz
```

### Installation
```bash
git clone https://github.com/your-repo/SyPetype.git
cd SyPetype
cargo build --release
```

### Basic Usage

#### 1. Generate rustdoc JSON
```bash
cd examples/simple_counter
cargo +nightly rustdoc -- -Z unstable-options --output-format json
```

#### 2. Generate all outputs
```bash
./target/release/sypetype all \
    -i examples/simple_counter/target/doc/simple_counter.json \
    -o test_output/simple_counter
```

This generates:
- `apigraph.{dot,json}` - API Graph representation
- `pcpn.{dot,json}` - Petri Net model
- Visualization with graphviz (if available)

#### 3. Generate reachability graph
```bash
./target/release/sypetype reachability \
    -i examples/simple_counter/target/doc/simple_counter.json \
    -o test_output/simple_counter \
    --max-states 40
```

### Run Test Suite
```bash
chmod +x test.sh
./test.sh
```

## 📖 Examples

### Simple Counter
```rust
pub struct Counter {
    count: i32,
}

impl Counter {
    pub fn new() -> Self { Counter { count: 0 } }
    pub fn increment(&mut self) { self.count += 1; }
    pub fn get(&self) -> i32 { self.count }
}
```

**Generated PCPN includes:**
- 3 places per type: `Counter[own]`, `Counter[shr]`, `Counter[mut]`
- API transitions: `Counter::new`, `Counter::increment`, `Counter::get`
- Structural transitions: `borrow_mut`, `borrow_shr`, `end_borrow`, `drop`, `deref`

### Lifetime Example
```rust
pub struct Container { value: i32 }

impl Container {
    pub fn get_ref(&self) -> &i32 { &self.value }  // Returns reference
    pub fn get_mut(&mut self) -> &mut i32 { &mut self.value }
}
```

**Lifetime tracking:**
- `get_ref()` pushes lifetime frame, blocks source token
- Returned reference cannot be dropped while borrowed
- `end_borrow` pops frame, unblocks source token

## 🏗️ Architecture

### PCPN Model

#### Places (3 per type)
- **`T[own]`**: Ownership place (e.g., `Counter`, `String`)
- **`T[shr]`**: Shared reference place (e.g., `&Counter`)
- **`T[mut]`**: Mutable borrow place (e.g., `&mut Counter`)

#### Transitions
1. **API Calls**: Original Rust methods
2. **Structural Operations**:
   - `borrow_mut(T)`: `T[own] → T[mut]`
   - `borrow_shr(T)`: `T[own] → T[shr]`
   - `end_borrow_mut(T)`: `T[mut] → T[own]`
   - `end_borrow_shr(T)`: `T[shr] → T[own]`
   - `deref(T)`: `&&T → &T` (reduces ref_level)
   - `drop(T)`: `T[own] → ε`
   - `const_T`: `ε → T[own]` (for primitives)

#### Guards (Rust Borrowing Rules)
- **`RequireOwn`**: No `shr` or `mut` tokens exist (for ownership transfer)
- **`RequireShr`**: No `mut` tokens exist (for shared borrowing)
- **`RequireMut`**: No `shr` tokens exist (for mutable borrowing)
- **`RequireNotBorrowed`**: Token not blocked by lifetime stack (for drop)

### Token System

```rust
Token {
    id: TokenId,                      // Unique identifier
    type_key: TypeKey,                // Rust type
    capability: Capability,           // Own/Shr/Mut
    borrowed_from: Option<TokenId>,   // Borrow source tracking
    ref_level: usize,                 // 0=T, 1=&T, 2=&&T
    lifetime: Option<String>,         // Lifetime annotation
}
```

### Lifetime Stack

```rust
LifetimeStack {
    frames: Vec<LifetimeFrame>
}

LifetimeFrame {
    lifetime: String,       // Lifetime identifier
    borrows: Vec<TokenId>,  // Borrowed tokens in this frame
    blocks: Vec<TokenId>,   // Source tokens that are blocked
}
```

**Operations:**
- **Push**: API call returns reference → create frame, add borrow, block source
- **Pop**: `end_borrow` → remove frame, unblock source tokens
- **Check**: `is_blocked(token_id)` → used by `RequireNotBorrowed` guard

## 📊 Output Formats

### API Graph (DOT)
- **Nodes**: Function nodes (boxes) and type nodes (ellipses)
- **Edges**: Parameter/return edges with ownership annotations
  - Color: Black (Own), Blue (Shr), Red (Mut)
  - Label: `PassingMode[Ownership]`

### PCPN (DOT)
- **Places**: Circles with type and capability
  - Color: Blue (Own), Cyan (Shr), Pink (Mut)
- **Transitions**: Boxes with method names
  - Structural transitions in wheat color
- **Arcs**: Solid lines (no inhibitor arcs)
- **Guards**: Displayed as `[G:n]` count

### Reachability Graph (DOT)
- **States**: Circles with marking (token counts per place)
- **Transitions**: Directed edges with transition names
- Statistics: State count, edge count, example traces

## 🔬 Technical Details

### Key Design Decisions

1. **Token-Level vs Type-Level**
   - Old: "Any `Counter` borrowed → all `Counter` blocked"
   - New: "Only `Counter_token_1` blocked → `Counter_token_2` free"

2. **Automatic Lifetime Binding**
   - Detects reference returns by checking place capability
   - Generates lifetime ID: `'fn{fn_id}_{source_token_id}`
   - No need to parse complex rustdoc lifetime annotations

3. **EndBorrow Auto-Pop**
   - `end_borrow` automatically searches and removes lifetime frame
   - Unblocks source tokens when reference is dropped

4. **Copy Type Handling**
   - `Copy` types add `ReturnArc` in API calls (auto-duplication)
   - No explicit `dup_copy` transitions needed

### Limitations

- **Simplified Lifetime Model**: Assumes single lifetime parameter
- **No Complex Lifetimes**: Doesn't handle `'a`, `'b` multiple parameters
- **No Recursive Functions**: Flat lifetime stack model
- **Primitive Budget**: Limited to 3 instances per primitive type

## 📚 Documentation

- **Implementation Details**: `TOKEN_LIFETIME_IMPLEMENTATION.md`
- **TODO Tracking**: `TODO_TRACKING.md`
- **API Reference**: Generated by `cargo doc`

## 🧪 Testing

```bash
# Run full test suite
./test.sh

# Build rustdoc JSON manually
cd examples/simple_counter
cargo +nightly rustdoc -- -Z unstable-options --output-format json

# Generate only API graph
./target/release/sypetype apigraph \
    -i examples/simple_counter/target/doc/simple_counter.json \
    -o output/

# Generate only PCPN
./target/release/sypetype pcpn \
    -i examples/simple_counter/target/doc/simple_counter.json \
    -o output/
```

## 🤝 Contributing

Contributions are welcome! Please:
1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- Rust compiler team for rustdoc JSON format
- Petri net community for formal modeling foundations
- Graphviz for visualization tools

## 📞 Contact

For questions or issues, please open an issue on GitHub.

---

**Made with ❤️ for Rust formal verification**
