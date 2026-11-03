# synthetic-financial-data

A deterministic generator for enterprise-shaped financial data: retail banking,
non-life insurance and life underwriting inputs. Rust standard library only, no
real institutions, no personal data.

The datasets are large enough to exercise row caps, column masking, refusals and
joins. Every table has a primary key and real foreign keys.

## Requirements

Tested on Linux x86_64 (Rust 1.99.1, PostgreSQL 17, GNU bash 5.2). macOS and Windows are
untested: the last two columns say what to expect, not what has been verified.

| Component | Needed for | Linux (tested) | macOS (not tested) | Windows (not tested) |
| --- | --- | --- | --- | --- |
| Rust 1.99, pinned in [`rust-toolchain.toml`](rust-toolchain.toml) | the generator | yes | expected, rustup supports it | expected, rustup supports it |
| bash | [`generate.sh`](scripts/generate.sh), [`ci.sh`](scripts/ci.sh), [`load.sh`](scripts/load.sh), [`verify.sh`](scripts/verify.sh) | GNU bash 5.2 | bash 3.2 is shipped and zsh is the default shell; invoke with `bash` | Git Bash or WSL |
| python3, standard library only | [`sql_csv_check.py`](scripts/sql_csv_check.py), [`data_check.py`](scripts/data_check.py), [`hygiene.py`](scripts/hygiene.py) | yes | expected | expected |
| `psql` | [`load.sh`](scripts/load.sh) only | yes, PostgreSQL 17 | expected | expected, set `PGBIN` |
| `curl` and `jq` | [`verify.sh`](scripts/verify.sh) only | yes | expected | curl ships with Windows 10+, jq must be installed |

The generator is the portable part: Rust standard library only, no C bindings. The helper
scripts are the part that assumes a Unix shell.

### Windows and macOS

Neither platform has been tested. What to expect:

- `cargo build --release` should work on both; there are no dependencies to link.
- Output is plain text with LF line endings. `.gitattributes` pins LF so a Windows clone does
  not convert it to CRLF.
- The four helper scripts are bash. Without one, run the generator directly:
  `cargo run --release -- --out out --dataset all --seed 20260101`
- `load.sh` searches `/usr/lib/postgresql/17/bin`, `/usr/lib/postgresql/16/bin` and
  `/usr/local/bin` for `psql`. Set `PGBIN` where those paths do not apply.
- Determinism is byte level. Compare with `cmp` on Unix-like systems, or
  `certutil -hashfile` / `fc /b` on Windows.

## Three datasets

| Dataset | Tables | What it models |
| --- | --- | --- |
| `retail_banking` | `branches`, `customers`, `accounts`, `transactions`, `loans`, `kyc_reviews` | a retail bank: accounts per customer, transaction ledger, loan book, AML review history |
| `non_life` | `offices`, `policyholders`, `policies`, `coverages`, `exclusions`, `claims`, `claim_adjustments`, `endorsements` | a general non-life carrier, lines of business per Solvency II Annex I |
| `life` | `offices`, `policyholders`, `policies`, `mortality_assumptions`, `morbidity_assumptions`, `lapse_assumptions`, `expense_assumptions`, `discount_curve` | a life insurance carrier's underwriting inputs: policy issue data plus the assumption tables a model consumes |

`retail_banking`, `non_life` and `life` are the canonical labels. `banking`,
`insurance` and `both` are accepted as shorthand so an older command still runs.

The life dataset has no calculated projections (no best estimate liability, no
development triangles, no loss ratios). Those are model outputs; emitting them
without the model that produced them would be noise in a data file.

The retail banking dataset is internally consistent: `accounts.balance` is
`accounts.opening_balance` plus the posted entries in `transactions`. Pending and
reversed entries do not move a balance, and no debit is generated that would take
an account below zero. The generator refuses to write a dataset where a balance
does not match its ledger, and `scripts/verify.sh` re-runs the reconciliation
through the gateway.

Counts are flags, not hardcoded:

```sh
./scripts/generate.sh --dataset all --seed 20260101 \
  --customers 5000 --transactions 200000 --loans 1200 \
  --policyholders 4000 --policies 8000 --claims 25000 \
  --life-policyholders 3000 --life-policies 6000
```

Output layout, per dataset:

```
out/retail_banking/schema.sql  # DDL with primary keys and foreign keys
out/retail_banking/seed.sql    # INSERT statements, 500 rows per statement
out/retail_banking/manifest.csv # table, row count, columns
out/retail_banking/csv/<table>.csv  # one CSV per table
```

Amounts are `NUMERIC(12,2)` with no currency. Currency is declared per column in
the gateway registry.

## Use it in a spreadsheet

No infrastructure needed:

```sh
./scripts/generate.sh --dataset non_life --policies 2000 --claims 5000
head out/non_life/csv/claims.csv
```

CSV has no type information, so a spreadsheet will infer what it likes. If you
need types and constraints, use the SQL.

## Check the SQL against the CSV

SQL and CSV are written from the same in-memory table, so this checks the
serialisation (quoting, escaping, ordering, column names), not the data:

```sh
./scripts/sql_csv_check.py            # every dataset under out/
./scripts/sql_csv_check.py out/life   # one dataset
```

It parses every `INSERT` in `seed.sql`, compares it cell by cell with the matching
CSV, and exits non-zero on the first mismatch.

[`scripts/data_check.py`](scripts/data_check.py) checks the other direction: the row
counts in `manifest.csv` against the CSV and the SQL, and every identity column
against the placeholder pattern, so no plausible name, phone number or email can
appear in a dataset.

## Load it into Postgres

[`load.sh`](scripts/load.sh) creates one database per dataset, applies the schema, loads the seed, and
grants one role SELECT access only. It is idempotent: re-running reloads the tables.

```sh
./scripts/load.sh --dataset all      # synthetic_retail_bank, synthetic_non_life, synthetic_life
./scripts/load.sh --dataset retail_banking   # just the bank
```

Defaults are `127.0.0.1:5433`, role `demo`, read-only role `agent_ro` (no
password; local trust auth). Override with `PORT`, `PGUSER`, `PGBIN`, `OUT`,
`--bank-db`, `--non-life-db`, `--life-db`.

## Query it through a gateway

[`pg-guardrails`](https://github.com/inSourceLabs/pg-guardrails) is a read-only MCP gateway
for SQL: statement policy, row caps, column masking, audit trail. It is optional; nothing
here requires it. That repository is not populated yet, so the gateway commands below work
once it is.

Register each dataset as its own source so a cross-source join fails by design:

```sh
printf 'host=127.0.0.1 port=5433 dbname=synthetic_retail_bank user=agent_ro' > bank.dsn
pg-guardrails source add --config sources.json --id synth_retail_banking \
  --dsn-file ./bank.dsn --deny phone,national_id \
  --description "Synthetic retail banking dataset"
rm bank.dsn
pg-guardrails source test --config sources.json --id synth_retail_banking
pg-guardrails http --config sources.json --port 8413
```

[`examples/gateway-sources.json`](examples/gateway-sources.json) is the same registry in full. It declares no
`column_formats`: amounts print as stored.

Then:

```sh
MCP_URL=http://127.0.0.1:8413/mcp SOURCE=synth_retail_banking DATASET=retail_banking ./scripts/verify.sh
```

[`verify.sh`](scripts/verify.sh) reads the table list and expected row counts from the manifest, then
checks the gateway: tables present, counts correct, a join plus aggregate returns a
real number, masking hides `phone` and `national_id`, the row cap applies, a
`DELETE` and a stacked statement are refused, and the audit trail has a record for
every call.

## Gate set

Every gate runs locally. There is no build service and nothing to configure on a
hosting platform.

```sh
./scripts/ci.sh              # rust gates, licence gate, determinism, data checks, hygiene
./scripts/ci.sh fast         # rust gates and hygiene; nothing is generated
./scripts/ci.sh full         # also loads the datasets into Postgres and runs verify.sh
```

`ci.sh` runs the generator twice with the same seed and compares the output byte for
byte, then checks the SQL against the CSV and the manifests, then audits the tracked
files (no machine paths, no credentials, no CRLF, no broken links).

## Determinism and what is not real

- Same seed and counts produce byte-identical files. Each dataset draws from its
  own stream derived from the seed, so generating one does not shift another:
  `cargo run --quiet -- --out out-a` then `--out out-b`, then `cmp out-a/retail_banking/seed.sql out-b/retail_banking/seed.sql`.
- Names are `Customer 1`, `Branch 12`, `Office 3`. No institution, person or place.
- Phone and national id columns exist so masking can be demonstrated. They are
  sequential placeholders and are denied by default in the example registry.
- Dates are ISO `YYYY-MM-DD` from a fixed base day. There is no time of day and no
  timezone.
- Money is integer cents formatted with two decimals, so no float rounding reaches
  the files. Rates are integer micro-units for the same reason.
- Assumption tables (mortality, morbidity, lapse, expense, discount curve) are
  invented by formula, not copied from a published mortality table. Swap them for
  the tables your actuary or regulator specifies; the shape stays the same.
- The generator checks its own foreign keys before writing. If a child row points
  at a parent that does not exist, it exits 1 rather than emitting the file.

## Releases

Each release attaches the generated datasets as archives, so the data can be used without
building anything. Every archive contains `schema.sql`, `seed.sql`, `manifest.csv`, `csv/`
and a `MANIFEST.txt` recording the commit, seed, row counts and per-file checksums.

| Archive | Rows | Compressed size |
| --- | --- | --- |
| `retail_banking-0.1.0.tar.gz` | 228,792 | 8.6 MB |
| `non_life-0.1.0.tar.gz` | 88,070 | 3.0 MB |
| `life-0.1.0.tar.gz` | 9,685 | 0.5 MB |
| `all-datasets-0.1.0.tar.gz` | 326,547 | 12.1 MB |

### Verify an archive

```sh
gpg --import keys/inSource-public.asc
gpg --verify retail_banking-0.1.0.tar.gz.asc retail_banking-0.1.0.tar.gz
```

Signing key `DC74A1A3EC37F13DC7E85ABE13465C6AFB9369DA`; the public key is in the repo at
[`keys/inSource-public.asc`](keys/inSource-public.asc). The sha256 of every archive is listed
in the release notes.

### Reproduce an archive

```sh
git checkout v0.1.0
cargo build --release
cargo run --release -- --out out --dataset all --seed 20260101   # defaults, no extra flags
tar --format=gnu --owner=0 --group=0 --mtime='2026-11-03T00:00:00Z' \
    -czf retail_banking-0.1.0.tar.gz -C out/retail_banking .
```

The generator is deterministic and the packaging flags are fixed, so the archive checksums are
reproducible. `.gitattributes` keeps LF line endings so a Windows clone does not change the
bytes.

## Why Rust

No third-party crates, one binary, and exact arithmetic: money and rates are built
from integers, so a value that should be 63.45 is 63.45 in the SQL and in the CSV,
byte for byte. Determinism is a property of the generator, not of the machine.

## Licence

Apache-2.0. See [`LICENSE`](LICENSE).
