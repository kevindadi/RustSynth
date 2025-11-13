#!/bin/bash

echo "=========================================="
echo "Demo 1: String -> T"
echo "=========================================="
cargo run --bin petri_synth -- --rustdoc base64.json --input "String" --goal "T" 2>&1 | tail -10

echo ""
echo "=========================================="
echo "Demo 2: String -> &mut T "
echo "=========================================="
cargo run --bin petri_synth -- --rustdoc base64.json --input "String" --goal "&mut T" 2>&1 | tail -10

echo ""
echo "=========================================="
echo "Demo 3: &String -> T"
echo "=========================================="
cargo run --bin petri_synth -- --rustdoc base64.json --input "&String" --goal "T" 2>&1 | tail -5

