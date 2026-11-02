#!/usr/bin/env bash
# generate.sh - write the datasets under the output directory.
#
#   ./scripts/generate.sh --dataset all --seed 20260101
#   ./scripts/generate.sh --dataset retail_banking --customers 1000 --transactions 50000
#
# Output layout: out/<dataset>/schema.sql, out/<dataset>/seed.sql,
# out/<dataset>/manifest.csv and out/<dataset>/csv/<table>.csv.
# Extra arguments are passed straight to the generator.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"

cd "$ROOT"
cargo build --quiet
cargo run --quiet -- --out out "$@"
