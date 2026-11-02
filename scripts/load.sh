#!/usr/bin/env bash
# load.sh - create a database per dataset, apply the schema, load the seed and
# grant one role SELECT access only. Idempotent: re-running reloads the tables.
#
#   ./scripts/generate.sh --dataset all
#   ./scripts/load.sh                       # all three datasets
#   ./scripts/load.sh --dataset retail_banking   # out/retail_banking into synthetic_retail_bank
#
# Env: PORT (5433), PGUSER (demo), PGBIN, OUT.
# Databases: synthetic_retail_bank, synthetic_non_life, synthetic_life.
# Override with --bank-db, --non-life-db and --life-db. The role is agent_ro
# (no password, local trust auth only).

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
OUT="${OUT:-$ROOT/out}"
PORT="${PORT:-5433}"
PGUSER="${PGUSER:-demo}"
PGBIN="${PGBIN:-}"
DATASET="both"
BANK_DB="synthetic_retail_bank"
NON_LIFE_DB="synthetic_non_life"
LIFE_DB="synthetic_life"

while [ $# -gt 0 ]; do
  case "$1" in
    --dataset) DATASET="$2"; shift 2 ;;
    --bank-db) BANK_DB="$2"; shift 2 ;;
    --non-life-db) NON_LIFE_DB="$2"; shift 2 ;;
    --life-db) LIFE_DB="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    *) echo "error: unknown argument '$1'"; exit 1 ;;
  esac
done

if [ -z "$PGBIN" ]; then
  for c in /usr/lib/postgresql/17/bin /usr/lib/postgresql/16/bin /usr/local/bin; do
    if [ -x "$c/psql" ]; then PGBIN="$c"; break; fi
  done
fi

if [ -z "$PGBIN" ]; then
  echo "psql not found. Install PostgreSQL 16+ or point PGBIN at its bin directory."
  exit 1
fi

# Canonical labels are retail_banking, non_life and life. The older shorthand is
# still accepted so an existing command keeps working.
case "$DATASET" in
  banking|retail_banking) DATASET="retail_banking" ;;
  insurance|non_life) DATASET="non_life" ;;
  life) DATASET="life" ;;
  both|all) ;;
  *)
    echo "error: --dataset must be retail_banking, non_life, life, both or all"
    exit 1
esac

PSQL=("$PGBIN/psql" -h 127.0.0.1 -p "$PORT" -U "$PGUSER" -v ON_ERROR_STOP=1)

load_one() {
  label="$1"
  db="$2"
  dir="$OUT/$label"
  if [ ! -f "$dir/schema.sql" ] || [ ! -f "$dir/seed.sql" ]; then
    echo "no generated files in $dir. Run ./scripts/generate.sh --dataset $label first."
    exit 1
  fi

  "${PSQL[@]}" -d postgres -q -c "CREATE DATABASE $db" 2>/dev/null \
    || echo "database $db already exists"

  "${PSQL[@]}" -d "$db" -q -f "$dir/schema.sql"
  "${PSQL[@]}" -d "$db" -q -f "$dir/seed.sql"

  # A role with no password and SELECT only: the gateway connects as this role,
  # so a compromised gateway cannot write.
  "${PSQL[@]}" -d "$db" -q <<SQL
DO \$\$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'agent_ro') THEN
    CREATE ROLE agent_ro LOGIN;
  END IF;
END
\$\$;
ALTER ROLE agent_ro SET default_transaction_read_only = on;
REVOKE ALL ON ALL TABLES IN SCHEMA public FROM agent_ro;
REVOKE ALL ON ALL TABLES IN SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA public TO agent_ro;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO agent_ro;
SQL

  tables=$("${PSQL[@]}" -d "$db" -q -t -c "SELECT count(*) FROM pg_tables WHERE schemaname = 'public'")
  fk=$("${PSQL[@]}" -d "$db" -q -t -c "SELECT count(*) FROM pg_constraint WHERE conname LIKE '%_fkey'")
  echo "loaded $db on 127.0.0.1:$PORT: $tables tables, $fk foreign keys, read-only role agent_ro"
}

if [ "$DATASET" = "retail_banking" ] || [ "$DATASET" = "both" ] || [ "$DATASET" = "all" ]; then
  load_one retail_banking "$BANK_DB"
fi
if [ "$DATASET" = "non_life" ] || [ "$DATASET" = "both" ] || [ "$DATASET" = "all" ]; then
  load_one non_life "$NON_LIFE_DB"
fi
if [ "$DATASET" = "life" ] || [ "$DATASET" = "all" ]; then
  load_one life "$LIFE_DB"
fi

echo
echo "connection strings for the gateway registry:"
if [ "$DATASET" = "retail_banking" ] || [ "$DATASET" = "both" ]; then
  echo "  host=127.0.0.1 port=$PORT dbname=$BANK_DB user=agent_ro"
fi
if [ "$DATASET" = "non_life" ] || [ "$DATASET" = "both" ] || [ "$DATASET" = "all" ]; then
  echo "  host=127.0.0.1 port=$PORT dbname=$NON_LIFE_DB user=agent_ro"
fi
if [ "$DATASET" = "life" ] || [ "$DATASET" = "all" ]; then
  echo "  host=127.0.0.1 port=$PORT dbname=$LIFE_DB user=agent_ro"
fi
