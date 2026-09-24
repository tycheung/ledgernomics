#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
echo "ci_check OK"
