# For agents and contributors

This repository generates synthetic banking, non-life insurance and life
underwriting datasets. It is standalone: nothing here depends on a gateway, and
the output is useful as SQL or as CSV on its own.

## What matters when changing the generator

- Every table is built once as a `Table` with explicit column types, then both SQL
  and CSV are derived from the same cells. Do not introduce a second path that
  writes either format separately.
- Column types are declared per column. Guessing a type from a name is how a text
  reference like `ID-000001` ends up declared as a number.
- Foreign keys are declared in the schema and checked before writing. If you add a
  table with a reference, add the `ForeignKey` and the test that proves it resolves.
- No column name may be a Postgres reserved word. `limit` was one; the schema
  failed to load. There is a test for this.
- Money is integer cents, rates are integer micro-units. No floats in the output.
- Determinism is a contract. Run two generations with the same flags and `cmp` the
  files before claiming a change is safe.

## Ground each field set in something real

Before adding fields for an industry or a line of business, write the reasoning
down in `Research_and_Development/YYYY-MM-DD_<topic>.md` with the source URL,
authors, publication date, fetch date and method. UK regulation is the baseline:
Solvency II Annex I for non-life lines, Annex II for life classes, FCA and PRA
expectations for banking records.

Do not emit model outputs (best estimate liability, development triangles, loss
ratios). Emit inputs. A projection without the model that produced it is noise.

## Licence and dependency policy

Apache-2.0. Standard library only. `deny.toml` enforces a permissive-only licence
allowlist so a dependency added later cannot bring a restrictive licence in
silently. Never add `psycopg`, `psycopg2` or any GPL/AGPL/LGPL crate.

## Nothing internal is tracked

`target/`, `out/`, `out-a/`, `out-b/`, `*.log` and `Research_and_Development/` stay
out of the repository. Check `git status --short` before committing.
