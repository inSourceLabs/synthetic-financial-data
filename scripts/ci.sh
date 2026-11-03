#!/usr/bin/env bash
# ci.sh - every gate for this repository, run locally.
#
#   ./scripts/ci.sh              # rust gates, licence gate, determinism, data checks, hygiene
#   ./scripts/ci.sh fast         # rust gates and hygiene only; nothing is generated
#   ./scripts/ci.sh full         # the above, plus a Postgres load and a gateway round trip
#   ./scripts/ci.sh --no-advisories  # skip the RustSec fetch; runs offline
#
# Everything runs on this machine against this checkout. The only network access
# is cargo fetching crates and cargo-deny fetching the RustSec advisory database,
# which --no-advisories turns off.
#
# Requires: cargo, rustfmt, clippy, python3, git. cargo-deny is optional here
# (the crate has no dependencies) and a fallback check runs without it.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
cd "$ROOT"

MODE=all
ADVISORIES=1
NO_LOG=0
for arg in "$@"; do
  case "$arg" in
    fast) MODE=fast ;;
    full) MODE=full ;;
    all) MODE=all ;;
    --no-advisories) ADVISORIES=0 ;;
    --no-log) NO_LOG=1 ;;
    *) echo "unknown argument: $arg"; exit 2 ;;
  esac
done

STAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
LOG="$ROOT/reports/ci/ci-$STAMP.log"
if [ "$NO_LOG" = 0 ]; then
  mkdir -p "$ROOT/reports/ci"
  exec > >(tee -a "$LOG") 2>&1
fi

RESULTS=()
FAILS=0
SKIPS=0

record() {
  RESULTS+=("$2|$1|${3:-}")
  case "$2" in
    FAIL) FAILS=$((FAILS + 1)) ;;
    SKIP) SKIPS=$((SKIPS + 1)) ;;
  esac
}

run() {
  label="$1"; shift
  echo "--- $label"
  output="$("$@" 2>&1)"
  rc=$?
  if [ "$rc" -eq 0 ]; then
    record "$label" PASS
    echo "  ok"
  else
    reason="$(echo "$output" | tail -n 4 | tr '\n' ' ')"
    record "$label" FAIL "$reason"
    echo "  FAIL: $reason"
  fi
}

have() { command -v "$1" >/dev/null 2>&1; }

echo "synthetic-financial-data local CI, $STAMP"
echo "mode: $MODE"
echo

# ---------------------------------------------------------------- preflight
ok=1
for tool in cargo rustc python3 git; do
  if ! have "$tool"; then
    echo "missing required tool: $tool"
    ok=0
  fi
done
if [ "$ok" = 0 ]; then
  echo "preflight failed; nothing else can run"
  exit 1
fi

PINNED="$(sed -n 's/^channel = "\([^"]*\)".*/\1/p' rust-toolchain.toml)"
RUSTC="$(rustc --version | awk '{print $2}')"
if [ -n "$PINNED" ] && [ "${RUSTC%%.*}" != "${PINNED%%.*}" ]; then
  echo "rustc $RUSTC does not match the pinned toolchain channel $PINNED"
  exit 1
fi
echo "toolchain: rustc $RUSTC (rust-toolchain.toml pins $PINNED)"

# ---------------------------------------------------------------- rust gates
run "format" cargo fmt --check
run "clippy" cargo clippy --all-targets -- -D warnings
run "build" cargo build
run "unit tests" cargo test

# ---------------------------------------------------------------- licence gate
if have cargo-deny; then
  run "licence and source gate" cargo deny check licenses bans sources
  if [ "$ADVISORIES" = 1 ]; then
    echo "--- RustSec advisory database"
    if cargo deny check advisories >/dev/null 2>&1; then
      record "advisories" PASS
      echo "  ok"
    else
      out="$(cargo deny check advisories 2>&1 | head -n 5)"
      if echo "$out" | grep -qiE 'error.*(fetch|network|resolve|connect)'; then
        record "advisories" SKIP "the advisory database could not be reached"
        echo "  SKIP: advisory database unreachable (offline)"
      else
        record "advisories" FAIL "$out"
        echo "  FAIL: $out"
      fi
    fi
  else
    record "advisories" SKIP "requested with --no-advisories"
    echo "--- RustSec advisory database: skipped (--no-advisories)"
  fi
else
  # cargo-deny is not installed. The fallback is a graph check: the crate ships
  # with no dependencies, so anything in Cargo.lock other than the crate itself
  # is a finding that needs cargo-deny to licence-audit.
  echo "--- cargo-deny is not installed; running the dependency graph fallback"
  extras="$(grep -c '^\[\[package\]\]' Cargo.lock)"
  if [ "$extras" -le 1 ]; then
    record "licence gate fallback" PASS
    echo "  ok: no dependencies in Cargo.lock to licence-audit"
  else
    record "licence gate fallback" FAIL "Cargo.lock lists dependencies but cargo-deny is missing"
    echo "  FAIL: install cargo-deny to audit the licences"
  fi
fi

# ---------------------------------------------------------------- data gates
if [ "$MODE" != "fast" ]; then
  A="/tmp/sfd-ci-a-$STAMP"
  B="/tmp/sfd-ci-b-$STAMP"
  echo "--- determinism: the same seed and counts must produce identical bytes"
  cargo build --release --quiet
  cargo run --release --quiet -- --out "$A" --dataset all >/dev/null 2>&1
  cargo run --release --quiet -- --out "$B" --dataset all >/dev/null 2>&1
  same=1
  for dataset in retail_banking non_life life; do
    if [ -d "$A/$dataset" ] && [ -d "$B/$dataset" ]; then
      if diff -rq "$A/$dataset" "$B/$dataset" >/dev/null 2>&1; then
        echo "  ok: $dataset identical across two runs"
      else
        same=0
        echo "  FAIL: $dataset differs between two runs with the same seed"
      fi
    else
      same=0
      echo "  FAIL: dataset $dataset was not written"
    fi
  done
  record "determinism" "$([ "$same" = 1 ] && echo PASS || echo FAIL)"

  echo "--- serialisation and manifest consistency"
  if python3 scripts/sql_csv_check.py "$A" >/dev/null 2>&1; then
    record "sql matches csv" PASS
    echo "  ok"
  else
    record "sql matches csv" FAIL "scripts/sql_csv_check.py reported a mismatch"
    echo "  FAIL"
  fi
  if python3 scripts/data_check.py "$A"; then
    record "data hygiene" PASS
  else
    record "data hygiene" FAIL "see the data check output above"
  fi
  rm -rf "$A" "$B"
fi

# ---------------------------------------------------------------- repo hygiene
echo "--- repository hygiene"
if python3 scripts/hygiene.py; then
  record "hygiene" PASS
else
  record "hygiene" FAIL "see the hygiene output above"
fi

# ---------------------------------------------------------------- gateway round trip
if [ "$MODE" = "full" ]; then
  echo "--- gateway round trip (needs a running pg-guardrails gateway)"
  if ! have psql && [ -z "${PGBIN:-}" ]; then
    record "gateway round trip" SKIP "no psql available for scripts/load.sh"
    echo "  SKIP: no psql"
  elif ! bash scripts/load.sh >/dev/null 2>&1; then
    record "gateway round trip" FAIL "scripts/load.sh could not load the datasets"
    echo "  FAIL"
  elif ! curl -s -o /dev/null "${MCP_URL:-http://127.0.0.1:8411/mcp}"; then
    record "gateway round trip" SKIP "no gateway listening at ${MCP_URL:-http://127.0.0.1:8411/mcp}"
    echo "  SKIP: start the gateway first, then re-run with --full"
  else
    run "gateway checks (verify.sh)" bash scripts/verify.sh
  fi
fi

if [ "$NO_LOG" = 0 ]; then
  echo
  echo "log: $LOG"
fi

echo
echo "=========================="
for entry in "${RESULTS[@]}"; do
  status="${entry%%|*}"; rest="${entry#*|}"; gate="${rest%%|*}"; reason="${rest#*|}"
  printf "%-6s %-30s %s\n" "$status" "$gate" "$reason"
done
echo "=========================="
echo "gates: ${#RESULTS[@]}, failed: $FAILS, skipped: $SKIPS"
[ "$FAILS" -eq 0 ] || exit 1
