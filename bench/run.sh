#!/usr/bin/env bash
set -euo pipefail
SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    echo "ERROR: squeez binary not found at $SQUEEZ" >&2
    exit 1
fi
FIXTURES="$(dirname "$0")/fixtures"
REPORT="$(dirname "$0")/report.md"
FAIL=0; TOTAL=0

printf "%-35s %8s %8s %10s %8s %6s\n" FIXTURE BEFORE AFTER REDUCTION LATENCY STATUS > "$REPORT"
printf '%.0s─' {1..78} >> "$REPORT"; echo >> "$REPORT"

for f in "$FIXTURES"/*.txt; do
    name=$(basename "$f")
    input=$(cat "$f")
    before=$(( ${#input} / 4 ))
    [ "$before" -eq 0 ] && continue

    # Derive handler hint from fixture name: "git_log_200.txt" → hint="git"
    hint="${name%%_*}"

    t0=$(date +%s%N)
    compressed=$(echo "$input" | "$SQUEEZ" filter "$hint" 2>/dev/null || echo "$input")
    t1=$(date +%s%N)
    ms=$(( (t1 - t0) / 1000000 ))

    after=$(( ${#compressed} / 4 ))
    pct=$(( 100 - (after * 100 / before) ))
    status="✅"; [ "$pct" -lt 30 ] && { status="❌"; FAIL=$((FAIL+1)); }
    [ "$ms" -gt 25 ] && { status="❌ slow"; FAIL=$((FAIL+1)); }
    TOTAL=$((TOTAL+1))

    printf "%-35s %7stk %7stk %9s%% %7sms  %s\n" "$name" "$before" "$after" "$pct" "$ms" "$status" >> "$REPORT"
done

echo >> "$REPORT"
echo "PASS: $((TOTAL-FAIL))/$TOTAL  FAIL: $FAIL/$TOTAL" >> "$REPORT"
cat "$REPORT"
[ "$FAIL" -eq 0 ]
