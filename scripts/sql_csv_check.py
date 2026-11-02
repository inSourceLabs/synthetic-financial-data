#!/usr/bin/env python3
"""Compare the generated SQL with the generated CSV, cell by cell.

Both files are produced from the same in-memory table, so this is a check on the
serialisation, not on the data: quoting, escaping, ordering and column names must
all agree.

Usage:
    ./scripts/sql_csv_check.py                 # every dataset under out/
    ./scripts/sql_csv_check.py out/banking     # one dataset

Standard library only. No network access.
"""

from __future__ import annotations

import csv
import re
import sys
from pathlib import Path


def split_values(text: str) -> list[list[str]]:
    """Return the tuples in a VALUES list as lists of unquoted cell strings."""
    cells: list[list[str]] = []
    row: list[str] = []
    buf: list[str] = []
    in_tuple = False
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
        if not in_tuple:
            # Outside a tuple: only an opening bracket starts one.
            if ch == "(":
                in_tuple = True
                row = []
                buf = []
            i += 1
            continue
        if ch == "'":
            i += 1
            while i < n:
                if text[i] == "'":
                    if i + 1 < n and text[i + 1] == "'":
                        buf.append("'")
                        i += 2
                        continue
                    i += 1
                    break
                buf.append(text[i])
                i += 1
            continue
        if ch == ")":
            row.append("".join(buf))
            buf = []
            cells.append(row)
            in_tuple = False
            i += 1
            continue
        if ch == ",":
            row.append("".join(buf))
            buf = []
            i += 1
            continue
        if ch in " \n\t;":
            i += 1
            continue
        buf.append(ch)
        i += 1
    return cells


def unquote_sql(value: str) -> str:
    if value.startswith("'") and value.endswith("'"):
        return value[1:-1]
    return value


def parse_seed(seed: str) -> dict[str, tuple[list[str], list[list[str]]]]:
    tables: dict[str, tuple[list[str], list[list[str]]]] = {}
    for match in re.finditer(
        r"INSERT INTO (\w+) \(([^)]*)\) VALUES\n(.*?);\n",
        seed,
        re.DOTALL,
    ):
        name = match.group(1)
        columns = [column.strip() for column in match.group(2).split(",")]
        rows = split_values(match.group(3))
        if name in tables:
            # The generator emits one INSERT per 500 rows, so a table can appear
            # more than once. The header must be the same every time.
            if tables[name][0] != columns:
                raise ValueError(f"{name}: two INSERT statements with different columns")
            tables[name][1].extend(rows)
        else:
            tables[name] = (columns, rows)
    return tables


def main(argv: list[str]) -> int:
    root = Path(argv[1]) if len(argv) > 1 else Path("out")
    if root.name in ("banking", "insurance", "life"):
        datasets = [root]
    else:
        datasets = [root / name for name in ("banking", "insurance", "life") if (root / name).is_dir()]
    compared_tables = 0
    compared_cells = 0
    for dataset in datasets:
        seed_path = dataset / "seed.sql"
        csv_dir = dataset / "csv"
        if not seed_path.is_file() or not csv_dir.is_dir():
            print(f"skip {dataset}: no seed.sql or csv/ directory")
            continue
        seed = seed_path.read_text()
        tables = parse_seed(seed)
        if not tables:
            print(f"FAIL {dataset}: no INSERT statements found in seed.sql")
            return 1
        for name, (columns, rows) in tables.items():
            csv_path = csv_dir / f"{name}.csv"
            if not csv_path.is_file():
                print(f"FAIL {name}: {csv_path} is missing")
                return 1
            with csv_path.open(newline="") as handle:
                reader = csv.reader(handle)
                header = next(reader)
                csv_rows = list(reader)
            if header != columns:
                print(f"FAIL {name}: header mismatch {header} vs {columns}")
                return 1
            if len(csv_rows) != len(rows):
                print(f"FAIL {name}: {len(csv_rows)} CSV rows vs {len(rows)} SQL rows")
                return 1
            for index, (sql_row, csv_row) in enumerate(zip(rows, csv_rows), start=1):
                if len(sql_row) != len(columns) or len(csv_row) != len(columns):
                    print(f"FAIL {name} row {index}: width mismatch")
                    return 1
                for column, sql_cell, csv_cell in zip(columns, sql_row, csv_row):
                    if unquote_sql(sql_cell) != csv_cell:
                        print(
                            f"FAIL {name} row {index} column {column}: SQL {sql_cell!r} vs CSV {csv_cell!r}"
                        )
                        return 1
                    compared_cells += 1
            compared_tables += 1
            print(f"ok {name}: {len(rows)} rows x {len(columns)} columns match")
    print(f"compared {compared_tables} tables and {compared_cells} cells")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
