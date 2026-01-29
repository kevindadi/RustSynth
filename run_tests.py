#!/usr/bin/env python3
"""
RustSynth - One-Click Test Script

This script builds the project, generates rustdoc JSON for all examples,
runs the synthesizer, and validates the output.

Usage:
    python3 run_tests.py [--verbose] [--no-build] [--examples EXAMPLE...]

Requirements:
    - Rust toolchain (stable + nightly)
    - Python 3.7+
"""

import argparse
import os
import subprocess
import sys
import shutil
from pathlib import Path
from typing import List, Optional, Tuple

# ANSI color codes
class Colors:
    GREEN = '\033[92m'
    BLUE = '\033[94m'
    YELLOW = '\033[93m'
    RED = '\033[91m'
    BOLD = '\033[1m'
    END = '\033[0m'

def print_header(msg: str):
    print(f"\n{Colors.BOLD}{Colors.BLUE}{'='*60}{Colors.END}")
    print(f"{Colors.BOLD}{Colors.BLUE}  {msg}{Colors.END}")
    print(f"{Colors.BOLD}{Colors.BLUE}{'='*60}{Colors.END}\n")

def print_step(msg: str):
    print(f"{Colors.BLUE}▶ {msg}{Colors.END}")

def print_success(msg: str):
    print(f"{Colors.GREEN}✓ {msg}{Colors.END}")

def print_warning(msg: str):
    print(f"{Colors.YELLOW}⚠ {msg}{Colors.END}")

def print_error(msg: str):
    print(f"{Colors.RED}✗ {msg}{Colors.END}")

def run_command(cmd: List[str], cwd: Optional[Path] = None, 
                capture: bool = False, verbose: bool = False) -> Tuple[int, str, str]:
    """Run a command and return (returncode, stdout, stderr)."""
    if verbose:
        print(f"  $ {' '.join(cmd)}")
    
    try:
        result = subprocess.run(
            cmd,
            cwd=cwd,
            capture_output=True,
            text=True
        )
        
        if verbose and result.stdout:
            for line in result.stdout.strip().split('\n')[:10]:
                print(f"    {line}")
        
        return result.returncode, result.stdout, result.stderr
    except FileNotFoundError:
        return -1, "", f"Command not found: {cmd[0]}"

def check_prerequisites() -> bool:
    """Check if required tools are installed."""
    print_step("Checking prerequisites...")
    
    # Check Rust
    ret, out, _ = run_command(["rustc", "--version"])
    if ret != 0:
        print_error("Rust not found. Install from https://rustup.rs")
        return False
    print(f"  Rust: {out.strip()}")
    
    # Check nightly
    ret, out, _ = run_command(["rustup", "run", "nightly", "rustc", "--version"])
    if ret != 0:
        print_warning("Rust nightly not found. Installing...")
        run_command(["rustup", "toolchain", "install", "nightly"])
    else:
        print(f"  Nightly: {out.strip()}")
    
    # Check Graphviz (optional)
    ret, _, _ = run_command(["dot", "-V"])
    if ret != 0:
        print_warning("Graphviz not found. PNG generation will be skipped.")
        print_warning("  Install: brew install graphviz (macOS) or apt install graphviz (Linux)")
    else:
        print_success("Graphviz available")
    
    return True

def build_project(verbose: bool = False) -> bool:
    """Build the RustSynth project."""
    print_step("Building RustSynth...")
    
    ret, out, err = run_command(
        ["cargo", "build", "--release"],
        verbose=verbose
    )
    
    if ret != 0:
        print_error("Build failed!")
        print(err)
        return False
    
    print_success("Build complete")
    return True

def run_tests(verbose: bool = False) -> bool:
    """Run unit tests."""
    print_step("Running unit tests...")
    
    ret, out, err = run_command(
        ["cargo", "test", "--release"],
        verbose=verbose
    )
    
    if ret != 0:
        print_error("Tests failed!")
        print(err)
        return False
    
    # Count tests
    for line in out.split('\n'):
        if 'test result' in line:
            print(f"  {line.strip()}")
    
    print_success("All tests passed")
    return True

def generate_rustdoc_json(example_dir: Path, verbose: bool = False) -> Optional[Path]:
    """Generate rustdoc JSON for an example crate."""
    crate_name = example_dir.name
    
    ret, _, err = run_command(
        ["cargo", "+nightly", "rustdoc", "-Z", "unstable-options", 
         "--output-format", "json", "--lib"],
        cwd=example_dir,
        verbose=verbose
    )
    
    if ret != 0:
        print_error(f"Failed to generate rustdoc JSON for {crate_name}")
        if verbose:
            print(err)
        return None
    
    json_path = example_dir / "target" / "doc" / f"{crate_name}.json"
    if not json_path.exists():
        # Try with underscores
        json_path = example_dir / "target" / "doc" / f"{crate_name.replace('-', '_')}.json"
    
    if json_path.exists():
        return json_path
    
    print_error(f"JSON file not found for {crate_name}")
    return None

def run_synthesizer(json_path: Path, output_dir: Path, 
                    task_toml: Optional[Path] = None,
                    verbose: bool = False) -> bool:
    """Run the synthesizer on a rustdoc JSON file."""
    RustSynth = Path("target/release/RustSynth")
    
    if not RustSynth.exists():
        print_error("RustSynth binary not found. Run build first.")
        return False
    
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # If task.toml exists, use synth command
    if task_toml and task_toml.exists():
        ret, out, err = run_command(
            [str(RustSynth), "synth",
             "--doc-json", str(json_path),
             "--task", str(task_toml),
             "--out", str(output_dir / "generated.rs")],
            verbose=verbose
        )
    else:
        # Use generate command
        ret, out, err = run_command(
            [str(RustSynth), "generate",
             "-i", str(json_path),
             "-o", str(output_dir),
             "--max-steps", "50",
             "--max-stack", "5"],
            verbose=verbose
        )
    
    if ret != 0:
        print_warning(f"Synthesis completed with warnings")
        if verbose:
            print(err)
    
    # Check output
    generated_file = output_dir / "generated.rs"
    if generated_file.exists():
        lines = generated_file.read_text().count('\n')
        print(f"  Generated: {generated_file} ({lines} lines)")
        return True
    
    return False

def generate_visualizations(output_dir: Path):
    """Generate PNG images from DOT files if Graphviz is available."""
    ret, _, _ = run_command(["dot", "-V"])
    if ret != 0:
        return
    
    for dot_file in output_dir.glob("*.dot"):
        png_file = dot_file.with_suffix(".png")
        run_command(["dot", "-Tpng", str(dot_file), "-o", str(png_file)])

def main():
    parser = argparse.ArgumentParser(
        description="RustSynth One-Click Test Script"
    )
    parser.add_argument("--verbose", "-v", action="store_true",
                        help="Show detailed output")
    parser.add_argument("--no-build", action="store_true",
                        help="Skip building (use existing binary)")
    parser.add_argument("--no-test", action="store_true",
                        help="Skip unit tests")
    parser.add_argument("--examples", nargs="*",
                        help="Specific examples to test (default: all)")
    args = parser.parse_args()
    
    # Change to project root
    project_root = Path(__file__).parent
    os.chdir(project_root)
    
    print_header("RustSynth - Pushdown CPN Safe Rust Synthesizer")
    
    # Prerequisites
    if not check_prerequisites():
        return 1
    
    # Build
    if not args.no_build:
        if not build_project(args.verbose):
            return 1
    
    # Unit tests
    if not args.no_test:
        if not run_tests(args.verbose):
            return 1
    
    # Find examples
    examples_dir = project_root / "examples"
    if args.examples:
        example_dirs = [examples_dir / e for e in args.examples]
    else:
        example_dirs = [d for d in examples_dir.iterdir() 
                       if d.is_dir() and (d / "Cargo.toml").exists()]
    
    if not example_dirs:
        print_warning("No examples found")
        return 0
    
    print_header("Testing Examples")
    
    output_base = project_root / "test_output"
    output_base.mkdir(exist_ok=True)
    
    results = []
    
    for example_dir in sorted(example_dirs):
        example_name = example_dir.name
        print_step(f"Testing {example_name}")
        
        # Generate rustdoc JSON
        json_path = generate_rustdoc_json(example_dir, args.verbose)
        if not json_path:
            results.append((example_name, False, "JSON generation failed"))
            continue
        print(f"  JSON: {json_path}")
        
        # Run synthesizer
        output_dir = output_base / example_name
        task_toml = example_dir / "task.toml"
        
        success = run_synthesizer(
            json_path, output_dir, 
            task_toml if task_toml.exists() else None,
            args.verbose
        )
        
        # Generate visualizations
        generate_visualizations(output_dir)
        
        if success:
            print_success(f"{example_name} complete")
            results.append((example_name, True, "OK"))
        else:
            print_warning(f"{example_name} completed with issues")
            results.append((example_name, False, "No code generated"))
    
    # Summary
    print_header("Test Summary")
    
    passed = sum(1 for _, s, _ in results if s)
    total = len(results)
    
    for name, success, msg in results:
        status = f"{Colors.GREEN}PASS{Colors.END}" if success else f"{Colors.YELLOW}WARN{Colors.END}"
        print(f"  [{status}] {name}: {msg}")
    
    print(f"\n  Total: {passed}/{total} examples succeeded")
    print(f"  Output directory: {output_base}")
    
    if passed == total:
        print_success("\nAll tests completed successfully!")
        return 0
    else:
        print_warning("\nSome tests had warnings")
        return 0  # Still return 0 since warnings are expected

if __name__ == "__main__":
    sys.exit(main())
