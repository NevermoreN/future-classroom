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
# The login gate is process-wide with two KDF slots. Large developer hosts must
# not turn unrelated fixture setup into an intentional overload test. Dedicated
# saturation and race tests still exercise those limits explicitly.
cargo test --workspace --locked -- --test-threads=2
cargo run -p fc-bootstrap --locked -- --self-check
