#!/usr/bin/env bash
# check.sh - the gate set for this repository: format, lint, tests, licence audit.
# cargo-deny is optional here: the crate has no dependencies, so the licence gate
# is a guard against a dependency being added later.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
cd "$ROOT"

cargo fmt --check
echo "format: ok"

cargo clippy --all-targets -- -D warnings
echo "clippy: ok"

cargo test
echo "tests: ok"

if command -v cargo-deny >/dev/null 2>&1; then
  cargo deny check
  echo "cargo-deny: ok"
else
  echo "cargo-deny: skipped (cargo-deny not installed)"
fi

echo "all gates passed"
