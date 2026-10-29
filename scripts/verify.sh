#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
# All build output from this verification belongs to this newly created directory.
TASK_BUILD_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/future-classroom-verify.XXXXXXXX")
trap 'rm -rf -- "$TASK_BUILD_ROOT"' EXIT
export CARGO_TARGET_DIR="$TASK_BUILD_ROOT"
cargo fmt --all -- --check
python3 scripts/check-architecture.py
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p fc-bootstrap --locked -- --self-check
