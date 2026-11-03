#!/usr/bin/env python3
"""Hygiene gate for what a clone of this repository would contain.

Checks tracked files only, so the gate reports what a reader or a clone would
see, not the local working directory. Standard library only, no network.

Run:
    python3 scripts/hygiene.py

Exit 0 when every rule holds, 1 when one does not.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

MUST_IGNORE = ["target/", "out/", "dist/", "*.log", "Research_and_Development/"]

fails: list[str] = []
notes: list[str] = []


def tracked_files() -> list[Path]:
    out = subprocess.run(["git", "ls-files"], cwd=REPO, capture_output=True, text=True).stdout
    return [REPO / p for p in out.splitlines()]


def rel(path: Path) -> str:
    return path.relative_to(REPO).as_posix()


def check_dashes(path: Path, text: str) -> None:
    for line, value in enumerate(text.splitlines(), 1):
        for ch in "\u2014\u2013":
            if ch in value:
                fails.append(f"{rel(path)}:{line}: dash character U+{ord(ch):04X} (use a hyphen)")


def check_paths(path: Path, text: str) -> None:
    for line, value in enumerate(text.splitlines(), 1):
        if "noqa: hygiene" in value:  # the pattern definitions in this file
            continue
        if re.search(r"/home/|/Users/|C:\\\\", value):  # noqa: hygiene
            fails.append(f"{rel(path)}:{line}: machine-specific path in a tracked file")


def check_credentials(path: Path, text: str) -> None:
    for line, value in enumerate(text.splitlines(), 1):
        if "noqa: hygiene" in value:  # the pattern definitions in this file
            continue
        if re.search(r"ghp_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|AKIA[0-9A-Z]{16}", value):
            fails.append(f"{rel(path)}:{line}: looks like a real token")
        if "BEGIN" in value and "PRIVATE KEY" in value:  # noqa: hygiene
            fails.append(f"{rel(path)}:{line}: private key material is tracked")
        for match in re.finditer(r"password=([A-Za-z0-9_\-\.\!]+)", value):  # noqa: hygiene
            if match.group(1) not in {"demo", "secret", "REDACTED", "changeme", "password"}:
                fails.append(f"{rel(path)}:{line}: credential in a tracked file")


def check_endings(path: Path, raw: bytes) -> None:
    if b"\r\n" in raw:
        fails.append(f"{rel(path)}: CRLF line endings (.gitattributes pins LF)")
    if raw and not raw.endswith(b"\n"):
        fails.append(f"{rel(path)}: no newline at end of file")
    if len(raw) > 200_000:
        fails.append(f"{rel(path)}: {len(raw)} bytes is too large to ship as source")


def check_links(path: Path, text: str) -> None:
    if path.suffix != ".md":
        return
    for match in re.finditer(r"\[([^\]]+)\]\(([^)\s]+)\)", text):
        target = match.group(2)
        if target.startswith(("http://", "https://", "mailto:")) or target.startswith("#"):
            continue
        target = target.split("#")[0]
        if target and not (REPO / target).exists():
            fails.append(f"{rel(path)}: link target does not exist: {target}")
    without_links = re.sub(r"\[([^\]]*)\]\(([^)]+)\)", " ", text)
    for match in re.finditer(r"`(scripts/[^`\s]+|src/[^`\s]+|examples/[^`\s]+|keys/[^`\s]+)`", without_links):
        target = match.group(1)
        if not (REPO / target).exists():
            fails.append(f"{rel(path)}: referenced path does not exist: {target}")


def check_gitignore() -> None:
    text = (REPO / ".gitignore").read_text(errors="replace")
    for entry in MUST_IGNORE:
        if entry not in text:
            fails.append(f".gitignore does not cover {entry}")


def check_licence_policy() -> None:
    for name in ("Cargo.toml", "Cargo.lock", "deny.toml"):
        if (REPO / name).exists() and "psycopg" in (REPO / name).read_text(errors="replace"):
            fails.append(f"{name}: psycopg is LGPL and may not appear in the dependency tree")
    toolchain = REPO / "rust-toolchain.toml"
    if not toolchain.exists() or "channel" not in toolchain.read_text(errors="replace"):
        fails.append("rust-toolchain.toml does not pin a toolchain channel")


def check_key_is_public() -> None:
    for path in REPO.glob("keys/*.asc"):
        head = path.read_text(errors="replace")[:200]
        if "BEGIN PGP PUBLIC KEY BLOCK" not in head:  # noqa: hygiene
            fails.append(f"{rel(path)}: a tracked key must be a public key block")


def check_no_data_tracked() -> None:
    for path in tracked_files():
        parts = path.relative_to(REPO).parts
        if parts and parts[0] in {"target", "out", "out-a", "out-b", "out-rel", "dist", "reports"}:
            fails.append(f"{rel(path)}: generated data or build output is tracked")


def main() -> int:
    files = tracked_files()
    if not files:
        fails.append("git ls-files returned nothing")
    for path in files:
        if not path.exists():
            fails.append(f"{rel(path)}: tracked but missing from the working tree")
            continue
        raw = path.read_bytes()
        text = raw.decode("utf-8", errors="replace")
        check_dashes(path, text)
        check_paths(path, text)
        check_credentials(path, text)
        check_endings(path, raw)
        check_links(path, text)
        if any(ord(ch) > 127 for ch in text) and path.suffix in {".rs", ".sh", ".py", ".sql", ".csv"}:
            notes.append(f"{rel(path)}: contains non-ASCII characters")
    check_gitignore()
    check_licence_policy()
    check_key_is_public()
    check_no_data_tracked()

    for note in notes:
        print(f"note: {note}")
    if fails:
        for failure in fails:
            print(f"FAIL: {failure}")
        print(f"{len(fails)} hygiene finding(s)")
        return 1
    print(f"hygiene: ok ({len(files)} tracked files)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
