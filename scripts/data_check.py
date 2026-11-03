#!/usr/bin/env python3
"""Check generated datasets against their manifests and scan them for personal data.

For every dataset under the given root:
  - every file named in manifest.csv exists;
  - the CSV header matches the columns recorded in the manifest;
  - the row count in the manifest matches the CSV and the SQL inserts;
  - no email address appears in any file;
  - identity columns hold placeholder values, not plausible real ones.

Standard library only. No network access.

Usage:
    python3 scripts/data_check.py out
    python3 scripts/data_check.py /tmp/sfd-ci-a
"""

from __future__ import annotations

import csv
import re
import sys
from pathlib import Path

EMAIL = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")

# Columns that would carry a person's identity if the data were real.
IDENTITY = re.compile(
    r"(^|_)(name|phone|national_id|email|reference|number|code)$", re.I
)
# Everything the generator produces for these columns: "Customer 1",
# "CUST-000001", "PHONE-000001", "ACC-000000001", "AGENT-105", or an integer.
SYNTHETIC = re.compile(r"^(?:[A-Za-z][A-Za-z ]*\d+|[A-Z0-9]+(?:-[A-Z0-9]+)*)$")

fails: list[str] = []


def sql_insert_counts(text: str) -> dict[str, int]:
    """Rows inserted per table, counting the tuples after each VALUES keyword."""
    counts: dict[str, int] = {}
    for match in re.finditer(r"INSERT INTO (\w+)\s*(?:\([^)]*\))?\s*VALUES(.*?);", text, re.S):
        table = match.group(1)
        body = match.group(2)
        tuples = 0
        depth = 0
        in_string = False
        for ch in body:
            if in_string:
                if ch == "'":
                    in_string = False
                continue
            if ch == "'":
                in_string = True
            elif ch == "(":
                depth += 1
                if depth == 1:
                    tuples += 1
            elif ch == ")":
                depth -= 1
        counts[table] = counts.get(table, 0) + tuples
    return counts


def check_dataset(root: Path, label: str) -> None:
    manifest = root / "manifest.csv"
    if not manifest.is_file():
        fails.append(f"{label}: no manifest.csv")
        return

    entries = list(csv.DictReader(manifest.read_text().splitlines()))
    if not entries:
        fails.append(f"{label}: the manifest lists no tables")
        return

    seed = (root / "seed.sql").read_text(errors="replace")
    sql_counts = sql_insert_counts(seed)

    for entry in entries:
        table = entry["table"]
        claimed = int(entry["rows"])
        path = root / entry["file"]
        if not path.is_file():
            fails.append(f"{label}: manifest names {entry['file']} but the file is missing")
            continue

        header = next(csv.reader(path.read_text().splitlines()), [])
        declared = [c.strip() for c in entry["columns"].split(",")]
        if header != declared:
            fails.append(f"{label}: {table} header {header} differs from the manifest {declared}")

        rows = list(csv.DictReader(path.read_text().splitlines()))
        if len(rows) != claimed:
            fails.append(f"{label}: {table} manifest says {claimed} rows, the CSV has {len(rows)}")
        if sql_counts.get(table) != claimed:
            fails.append(
                f"{label}: {table} manifest says {claimed} rows, seed.sql inserts {sql_counts.get(table)}"
            )

        for column in header:
            if not IDENTITY.search(column):
                continue
            for row in rows:
                value = (row[column] or "").strip()
                if value and not SYNTHETIC.match(value):
                    fails.append(
                        f"{label}: {table}.{column} has a value that is not a placeholder: {value!r}"
                    )
                    break

    for path in sorted(root.rglob("*")):
        if path.is_dir():
            continue
        if EMAIL.search(path.read_text(errors="replace")):
            fails.append(f"{label}: {path.name} contains an email address")


def main() -> int:
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("out")
    if not root.is_dir():
        print(f"FAIL: {root} is not a directory")
        return 1
    datasets = [d for d in sorted(root.iterdir()) if d.is_dir() and (d / "manifest.csv").is_file()]
    if not datasets:
        print(f"FAIL: no datasets with a manifest under {root}")
        return 1
    for dataset in datasets:
        check_dataset(dataset, dataset.name)
    if fails:
        for failure in fails:
            print(f"FAIL: {failure}")
        print(f"{len(fails)} data finding(s)")
        return 1
    print(f"data check: ok ({len(datasets)} datasets, manifest, serialisation and identity columns verified)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
