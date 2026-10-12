#!/usr/bin/env bash
# Local quality gate: format, lint, test. Exits non-zero on the first failure.
set -euo pipefail

cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
