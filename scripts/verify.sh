#!/usr/bin/env bash
# verify.sh - exercise a generated dataset through a running pg-guardrails
# gateway. Table names and expected row counts come from the dataset manifest,
# so the checks follow the generator instead of a hardcoded list.
#
#   ./target/debug/pg-guardrails http --config sources.json --port 8413
#   MCP_URL=http://127.0.0.1:8413/mcp SOURCE=synth_retail_banking DATASET=retail_banking ./scripts/verify.sh
#
# Requires curl and jq. Everything is loopback; nothing leaves the machine.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"

URL="${MCP_URL:-http://127.0.0.1:8411/mcp}"
SOURCE="${SOURCE:-synth}"
DATASET="${DATASET:-retail_banking}"
case "$DATASET" in
  banking) DATASET="retail_banking" ;;
  insurance) DATASET="non_life" ;;
esac
MANIFEST="$ROOT/out/$DATASET/manifest.csv"

if [ ! -f "$MANIFEST" ]; then
  echo "no manifest at $MANIFEST. Run ./scripts/generate.sh --dataset $DATASET first."
  exit 1
fi

SID=$(curl -s -D - -o /dev/null -X POST "$URL" \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"verify","version":"1"}}}' \
  | grep -i 'mcp-session-id' | tr -d '\r' | awk '{print $2}')

if [ -z "$SID" ]; then
  echo "gateway at $URL did not return a session id"
  exit 1
fi

call() {
  curl -s -X POST "$URL" \
    -H 'Content-Type: application/json' \
    -H 'Accept: application/json, text/event-stream' \
    -H "Mcp-Session-Id: $SID" \
    -d "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"tools/call\",\"params\":{\"name\":\"$1\",\"arguments\":$2}}" \
    | sed -n 's/^data: //p'
}

failures=0
check() {
  label="$1"
  got="$2"
  want="$3"
  if [ "$got" = "$want" ]; then
    printf "  ok   %-28s %s\n" "$label" "$got"
  else
    printf "  FAIL %-28s got %s, expected %s\n" "$label" "$got" "$want"
    failures=$((failures + 1))
  fi
}

echo "== tools =="
call list_sources "{}" >/dev/null
tools=$(call list_sources "{\"source\":\"$SOURCE\"}" \
  | jq -r '.result.structuredContent.sources[] | select(.id == "'"$SOURCE"'") | .id')
check "source registered" "$tools" "$SOURCE"

echo "== tables =="
listed=$(call schema "{\"source\":\"$SOURCE\"}" \
  | jq -r '.result.structuredContent.tables | join(", ")')
echo "  $listed"

echo "== row counts against the manifest =="
while read -r table expected; do
  got=$(call query "{\"source\":\"$SOURCE\",\"sql\":\"SELECT count(*) AS n FROM $table\"}" \
    | jq -r '.result.structuredContent.rows[0][0]')
  check "rows in $table" "$got" "$expected"
done < <(awk -F, 'NR>1 {print $2, $3}' "$MANIFEST")

echo "== derived metric (join plus aggregate) =="
if [ "$DATASET" = "non_life" ]; then
  derived=$(call query "{\"source\":\"$SOURCE\",\"sql\":\"SELECT p.line_of_business, count(*) AS claims, SUM(c.paid_amount) AS paid, ROUND(100.0 * SUM(c.paid_amount) / SUM(p.premium), 2) AS loss_ratio_pct FROM claims c JOIN policies p ON p.policy_id = c.policy_id GROUP BY p.line_of_business ORDER BY loss_ratio_pct DESC LIMIT 1\"}" \
    | jq -r '.result.structuredContent.rows[0][3]')
  if echo "$derived" | grep -qE '^[0-9]+\.[0-9]{2}$'; then
    check "loss ratio per line of business" "$derived" "$derived"
  else
    check "loss ratio per line of business" "$derived" "a number with two decimals"
  fi
elif [ "$DATASET" = "life" ]; then
  derived=$(call query "{\"source\":\"$SOURCE\",\"sql\":\"SELECT p.product, count(*) AS policies, ROUND(AVG(p.annual_premium), 2) AS average_premium FROM policies p GROUP BY p.product ORDER BY average_premium DESC LIMIT 1\"}" \
    | jq -r '.result.structuredContent.rows[0][2]')
  if echo "$derived" | grep -qE '^[0-9]+\.[0-9]{2}$'; then
    check "average premium by product" "$derived" "$derived"
  else
    check "average premium by product" "$derived" "a number with two decimals"
  fi
else
  derived=$(call query "{\"source\":\"$SOURCE\",\"sql\":\"SELECT c.region, count(*) AS txns, SUM(t.amount) AS total FROM transactions t JOIN accounts a ON a.account_id = t.account_id JOIN customers c ON c.customer_id = a.customer_id GROUP BY c.region ORDER BY total DESC LIMIT 1\"}" \
    | jq -r '.result.structuredContent.rows[0][1]')
  if echo "$derived" | grep -qE '^[0-9]+$'; then
    check "transactions per region" "$derived" "$derived"
  else
    check "transactions per region" "$derived" "a whole number"
  fi
fi

echo "== ledger reconciliation =="
if [ "$DATASET" = "banking" ]; then
  mismatch=$(call query "{\"source\":\"$SOURCE\",\"sql\":\"SELECT count(*) FROM (SELECT a.account_id FROM accounts a LEFT JOIN transactions t ON t.account_id = a.account_id AND t.status = 'posted' GROUP BY a.account_id, a.balance, a.opening_balance HAVING a.balance <> a.opening_balance + COALESCE(SUM(CASE WHEN t.direction = 'credit' THEN t.amount ELSE -t.amount END), 0)) mismatched\"}" \
    | jq -r '.result.structuredContent.rows[0][0]')
  check "accounts out of balance with the ledger" "$mismatch" "0"
else
  echo "  skip: only the banking dataset has an account ledger"
fi

echo "== masking =="
if [ "$DATASET" = "retail_banking" ]; then
  masked=$(call query "{\"source\":\"$SOURCE\",\"sql\":\"SELECT customer_id, full_name, phone, national_id FROM customers LIMIT 1\"}" \
    | jq -r '.result.structuredContent | (.columns|join(",")) + " | hidden " + (.columns_hidden|join(","))')
  check "masked columns" "$masked" "customer_id,full_name | hidden phone,national_id"
else
  echo "  skip: the non-life dataset has no personal identifier columns to mask"
fi

echo "== row cap =="
cap_table=$(awk -F, 'NR>1 {print $2}' "$MANIFEST" | head -n 5 | tail -n 1)
capped=$(call query "{\"source\":\"$SOURCE\",\"sql\":\"SELECT * FROM $cap_table\",\"limit\":10}" \
  | jq -r '.result.structuredContent | "returned " + (.row_count|tostring) + " truncated " + (.truncated|tostring)')
check "row cap at 10" "$capped" "returned 10 truncated true"

echo "== refusals =="
delete_ok=$(call query "{\"source\":\"$SOURCE\",\"sql\":\"DELETE FROM customers WHERE customer_id = 1\"}" \
  | jq -r '.result.isError')
check "DELETE refused" "$delete_ok" "true"
stack_ok=$(call query "{\"source\":\"$SOURCE\",\"sql\":\"SELECT 1; SELECT 2\"}" \
  | jq -r '.result.isError')
check "stacked statements refused" "$stack_ok" "true"

echo "== audit trail =="
audit=$(call audit "{\"limit\":5}" | jq -r '.result.structuredContent.records | length')
check "audit records returned" "$audit" "5"

echo
if [ "$failures" -eq 0 ]; then
  echo "all gateway checks passed against source '$SOURCE'"
else
  echo "$failures check(s) failed"
  exit 1
fi
