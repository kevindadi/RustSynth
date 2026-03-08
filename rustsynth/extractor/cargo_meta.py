"""Call cargo metadata to discover workspace packages and targets."""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional


@dataclass
class CrateTarget:
    name: str
    kind: str  # "lib", "bin", "test", etc.
    src_path: Path


@dataclass
class CratePackage:
    name: str
    version: str
    manifest_path: Path
    targets: list[CrateTarget] = field(default_factory=list)


def run_cargo_metadata(crate_path: Path) -> dict:
    """Run `cargo metadata --format-version 1 --no-deps` and return parsed JSON."""
    cmd = ["cargo", "metadata", "--format-version", "1", "--no-deps"]
    result = subprocess.run(
        cmd, cwd=str(crate_path), capture_output=True, text=True, timeout=30,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"cargo metadata failed (exit {result.returncode}):\n{result.stderr}"
        )
    return json.loads(result.stdout)


def discover_packages(crate_path: Path) -> list[CratePackage]:
    """Discover packages from cargo metadata."""
    meta = run_cargo_metadata(crate_path)
    packages = []
    for pkg in meta.get("packages", []):
        targets = []
        for tgt in pkg.get("targets", []):
            kinds = tgt.get("kind", [])
            kind = kinds[0] if kinds else "lib"
            targets.append(CrateTarget(
                name=tgt["name"],
                kind=kind,
                src_path=Path(tgt["src_path"]),
            ))
        packages.append(CratePackage(
            name=pkg["name"],
            version=pkg.get("version", "0.0.0"),
            manifest_path=Path(pkg["manifest_path"]),
            targets=targets,
        ))
    return packages


def find_lib_src(crate_path: Path) -> Optional[Path]:
    """Find the lib.rs source file for a crate."""
    packages = discover_packages(crate_path)
    for pkg in packages:
        for tgt in pkg.targets:
            if tgt.kind == "lib":
                return tgt.src_path
    lib_rs = crate_path / "src" / "lib.rs"
    if lib_rs.exists():
        return lib_rs
    return None
