#!/usr/bin/env bash
# windows_check.sh - build the generator for Windows and prove it produces the
# same bytes as the native build, through Wine.
#
#   ./scripts/windows_check.sh
#
# Wine is a compatibility layer, not Windows. This gate shows the generator's
# arithmetic and file output behave the same through the Win32 surface Wine
# implements. Determinism is the point: the same seed and counts must produce
# byte-identical datasets on either platform.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TARGET=x86_64-pc-windows-gnu
EXE="$ROOT/target/$TARGET/release/synthetic-financial-data.exe"

if ! command -v wine >/dev/null 2>&1; then
  echo "wine is not installed; the Windows gate cannot run"
  exit 0
fi
if ! rustup target list --installed | grep -q "^$TARGET$"; then
  echo "target $TARGET is not installed. Run: rustup target add $TARGET"
  exit 0
fi

cargo build --release --target "$TARGET" --quiet
if [ ! -f "$EXE" ]; then
  echo "no executable at $EXE"
  exit 1
fi

echo "--- version banner under Wine"
if wine "$EXE" --help 2>/dev/null | grep -qi 'synthetic-financial-data'; then
  echo "  ok"
else
  echo "  FAIL: the exe did not run"
  exit 1
fi

STAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
NATIVE="/tmp/sfd-native-$STAMP"
WINDOWS="/tmp/sfd-windows-$STAMP"
rm -rf "$NATIVE" "$WINDOWS"

cargo build --release --quiet
cargo run --release --quiet -- --out "$NATIVE" --dataset life --seed 20260101 >/dev/null 2>&1
wine "$EXE" --out "$WINDOWS" --dataset life --seed 20260101 >/dev/null 2>&1

echo "--- output comparison (same seed, same counts)"
if diff -rq "$NATIVE" "$WINDOWS" >/dev/null 2>&1; then
  echo "  ok: the Windows build produces byte-identical output"
  exit 0
fi
echo "  FAIL: the two builds differ"
diff -rq "$NATIVE" "$WINDOWS" | head -5
rm -rf "$NATIVE" "$WINDOWS"
exit 1
