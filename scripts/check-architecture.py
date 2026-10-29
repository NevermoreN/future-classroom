#!/usr/bin/env python3
"""Validate the D11 crate dependency boundaries without building Cargo artifacts."""
from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys


ALLOWED = {
    "fc-domain": set(),
    "fc-ports": {"fc-domain"},
    "fc-runtime": {"fc-domain", "fc-ports"},
    "fc-application": {"fc-domain", "fc-ports"},
    "fc-storage": {"fc-domain", "fc-ports"},
    "fc-security": {"fc-domain", "fc-ports"},
    "fc-materials": {"fc-domain", "fc-ports"},
    "fc-audio": {"fc-domain", "fc-ports"},
    "fc-presentation": {"fc-domain", "fc-ports"},
    "fc-protocol": {"fc-domain", "fc-ports"},
    "fc-windows": {
        "fc-domain", "fc-ports", "fc-runtime", "fc-presentation", "fc-protocol"
    },
    "fc-bootstrap": {
        "fc-domain", "fc-ports", "fc-runtime", "fc-application", "fc-storage",
        "fc-security", "fc-materials", "fc-audio", "fc-presentation", "fc-protocol",
        "fc-windows"
    },
}


def validate(metadata: dict) -> tuple[list[str], int]:
    """Include all dependency kinds/targets; renamed edges cannot evade checks."""
    member_ids = set(metadata["workspace_members"])
    packages = {
        package["name"]: package
        for package in metadata["packages"]
        if package["id"] in member_ids
    }
    errors = []
    actual = set(packages)
    expected = set(ALLOWED)
    if actual != expected:
        errors.append(
            f"workspace must contain exactly the 12 D11 crates; "
            f"missing={sorted(expected - actual)}, unexpected={sorted(actual - expected)}"
        )
    graph = {name: set() for name in packages}
    edges = 0
    for name, package in packages.items():
        dependencies = package["dependencies"]
        if name == "fc-domain" and dependencies:
            errors.append("fc-domain must have zero dependencies, including external/dev/build")
        for dependency in dependencies:
            target = dependency["name"]
            if not target.startswith("fc-"):
                continue
            edges += 1
            graph[name].add(target)
            if target not in actual:
                errors.append(f"{name} references unknown first-party crate {target}")
            if target not in ALLOWED.get(name, set()):
                kind = dependency.get("kind") or "normal"
                cfg = dependency.get("target") or "all targets"
                errors.append(f"forbidden dependency {name} -> {target} ({kind}, {cfg})")

    visiting: set[str] = set()
    complete: set[str] = set()

    def visit(name: str, route: list[str]) -> None:
        if name in visiting:
            errors.append("dependency cycle: " + " -> ".join([*route, name]))
            return
        if name in complete or name not in graph:
            return
        visiting.add(name)
        for target in sorted(graph[name]):
            visit(target, [*route, name])
        visiting.remove(name)
        complete.add(name)

    for name in sorted(graph):
        visit(name, [])
    return errors, edges


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    # metadata does not compile or create target artifacts. --locked/--offline
    # prevents dependency resolution from modifying Cargo.lock or using network.
    command = [
        "cargo", "metadata", "--no-deps", "--format-version", "1", "--locked",
        "--offline", "--manifest-path", str(root / "Cargo.toml"),
    ]
    try:
        result = subprocess.run(command, cwd=root, capture_output=True, text=True, check=True)
        errors, edges = validate(json.loads(result.stdout))
    except (OSError, subprocess.CalledProcessError, ValueError, KeyError) as error:
        print(f"Architecture check could not read Cargo metadata: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            print(error.stderr.rstrip(), file=sys.stderr)
        return 2
    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1
    print(f"Architecture OK: 12 crates, {edges} first-party edges; D11 boundaries hold.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
