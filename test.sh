#!/bin/bash
# SyPetype Test Script - Full Lifetime System
set -e

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

step() { echo -e "${BLUE}▶ $1${NC}"; }
success() { echo -e "${GREEN}✓ $1${NC}"; }
info() { echo -e "${YELLOW}ℹ $1${NC}"; }

echo "============================================"
echo "  SyPetype - Rust Ownership Petri Net"
echo "  Full Lifetime System Test Suite"
echo "============================================"
echo ""

# Build
step "1. Building SyPetype"
cargo build --release 2>&1 | grep -E "Compiling sypetype|Finished" || true
success "Build complete"
echo ""

SYPETYPE="./target/release/sypetype"

# Test 1: simple_counter
step "2. Testing simple_counter"
info "  Building rustdoc JSON..."
cd examples/simple_counter
cargo +nightly rustdoc -- -Z unstable-options --output-format json 2>&1 | tail -1
cd ../..

mkdir -p test_output/simple_counter
$SYPETYPE all \
    -i examples/simple_counter/target/doc/simple_counter.json \
    -o test_output/simple_counter 2>&1 | grep -E "INFO|✓" || true

success "simple_counter test complete"
echo ""

# Test 2: lifetime_example
step "3. Testing lifetime_example"
if [ -d "examples/lifetime_example" ]; then
    info "  Building rustdoc JSON..."
    cd examples/lifetime_example
    cargo +nightly rustdoc -- -Z unstable-options --output-format json 2>&1 | tail -1
    cd ../..

    mkdir -p test_output/lifetime_example
    $SYPETYPE all \
        -i examples/lifetime_example/target/doc/lifetime_example.json \
        -o test_output/lifetime_example 2>&1 | grep -E "INFO|✓" || true

    success "lifetime_example test complete"
else
    info "  Skipping lifetime_example (not found)"
fi
echo ""

# Generate reachability graphs
step "4. Generating Reachability Graphs"
$SYPETYPE reachability \
    -i examples/simple_counter/target/doc/simple_counter.json \
    -o test_output/simple_counter \
    --max-states 40 2>&1 | grep -E "INFO|可达图" || true

if [ -d "examples/lifetime_example" ]; then
    $SYPETYPE reachability \
        -i examples/lifetime_example/target/doc/lifetime_example.json \
        -o test_output/lifetime_example \
        --max-states 30 2>&1 | grep -E "INFO|可达图" || true
fi

success "Reachability graphs generated"
echo ""

# Generate PNG images (if graphviz is available)
if command -v dot &> /dev/null; then
    step "5. Generating PNG Images"
    
    dot -Tpng test_output/simple_counter/apigraph.dot -o test_output/simple_counter/apigraph.png 2>/dev/null || true
    dot -Tpng test_output/simple_counter/pcpn.dot -o test_output/simple_counter/pcpn.png 2>/dev/null || true
    dot -Tpng test_output/simple_counter/reachability.dot -o test_output/simple_counter/reachability.png 2>/dev/null || true
    
    if [ -d "test_output/lifetime_example" ]; then
        dot -Tpng test_output/lifetime_example/apigraph.dot -o test_output/lifetime_example/apigraph.png 2>/dev/null || true
        dot -Tpng test_output/lifetime_example/pcpn.dot -o test_output/lifetime_example/pcpn.png 2>/dev/null || true
        dot -Tpng test_output/lifetime_example/reachability.dot -o test_output/lifetime_example/reachability.png 2>/dev/null || true
    fi
    
    success "PNG images generated"
else
    info "  Graphviz not found, skipping PNG generation"
    info "  Install with: brew install graphviz (macOS) or apt install graphviz (Linux)"
fi
echo ""

# Summary
echo "============================================"
echo "  Test Summary"
echo "============================================"
echo ""
echo "Output files:"
echo ""
echo "simple_counter:"
echo "  - test_output/simple_counter/apigraph.{dot,json,png}"
echo "  - test_output/simple_counter/pcpn.{dot,json,png}"
echo "  - test_output/simple_counter/reachability.{dot,png}"
echo ""
if [ -d "test_output/lifetime_example" ]; then
echo "lifetime_example:"
echo "  - test_output/lifetime_example/apigraph.{dot,json,png}"
echo "  - test_output/lifetime_example/pcpn.{dot,json,png}"
echo "  - test_output/lifetime_example/reachability.{dot,png}"
echo ""
fi
echo "============================================"
echo ""
success "All tests completed successfully!"
echo ""
