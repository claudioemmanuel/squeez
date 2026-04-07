#!/usr/bin/env bash
# bench/run_context.sh — exercises the context engine end-to-end (PR1)
#
# Unlike bench/run.sh (which uses `squeez filter`), this runs `squeez wrap`
# on synthetic commands so the wrap.rs pre/post-pass code path is reached.
# Each test uses an isolated SQUEEZ_DIR so state never leaks.

set -euo pipefail

if [ -x "$(dirname "$0")/../target/release/squeez" ]; then
    SQUEEZ="$(cd "$(dirname "$0")/.." && pwd)/target/release/squeez"
elif [ -x "$HOME/.claude/squeez/bin/squeez" ]; then
    SQUEEZ="$HOME/.claude/squeez/bin/squeez"
else
    echo "ERROR: squeez binary not found. Run 'cargo build --release' first." >&2
    exit 1
fi

FIXTURES="$(dirname "$0")/fixtures"
FAIL=0
TOTAL=0
REPORT="$(dirname "$0")/report_context.md"
: > "$REPORT"

bench_dir() {
    local d
    d="$(mktemp -d -t squeez_bench_ctx.XXXXXX)"
    mkdir -p "$d/sessions" "$d/memory"
    echo "$d"
}

assert_contains() {
    local label=$1 haystack=$2 needle=$3
    if printf '%s' "$haystack" | grep -Fq -- "$needle"; then
        echo "  ✅ $label" | tee -a "$REPORT"
    else
        echo "  ❌ $label — expected to contain: $needle" | tee -a "$REPORT"
        FAIL=$((FAIL + 1))
    fi
}

assert_not_contains() {
    local label=$1 haystack=$2 needle=$3
    if printf '%s' "$haystack" | grep -Fq -- "$needle"; then
        echo "  ❌ $label — should not contain: $needle" | tee -a "$REPORT"
        FAIL=$((FAIL + 1))
    else
        echo "  ✅ $label" | tee -a "$REPORT"
    fi
}

# ── Test 1: summarize fallback on huge output ─────────────────────────────
echo "## summarize_huge — wrap output >500 lines triggers summary" | tee -a "$REPORT"
TOTAL=$((TOTAL + 1))
DIR=$(bench_dir)
OUT=$(SQUEEZ_DIR="$DIR" "$SQUEEZ" wrap "cat $FIXTURES/summarize_huge.txt" 2>&1 || true)
LINES=$(printf '%s\n' "$OUT" | wc -l | tr -d ' ')
assert_contains "summary header present" "$OUT" "squeez:summary"
assert_contains "total_lines emitted" "$OUT" "total_lines="
if [ "$LINES" -le 60 ]; then
    echo "  ✅ output ≤60 lines (got $LINES)" | tee -a "$REPORT"
else
    echo "  ❌ output too large: $LINES lines" | tee -a "$REPORT"
    FAIL=$((FAIL + 1))
fi
rm -rf "$DIR"
echo | tee -a "$REPORT"

# ── Test 2: intensity scaling at >80% budget ─────────────────────────────
echo "## intensity_budget80 — pre-seed total_tokens to force Ultra" | tee -a "$REPORT"
TOTAL=$((TOTAL + 1))
DIR=$(bench_dir)
# Seed current.json with usage = 90% of default budget (160000 * 5/4 = 200000)
SESSION_FILE="$(date -u +%Y-%m-%d-%H).jsonl"
cat > "$DIR/sessions/current.json" <<JSON
{"session_file":"$SESSION_FILE","total_tokens":180000,"compact_warned":false,"start_ts":0}
JSON
OUT=$(SQUEEZ_DIR="$DIR" "$SQUEEZ" wrap "cat $FIXTURES/intensity_budget80.txt" 2>&1 || true)
assert_contains "header shows adaptive: Ultra" "$OUT" "[adaptive: Ultra]"
rm -rf "$DIR"
echo | tee -a "$REPORT"

# ── Test 3: cross-call redundancy (3 identical runs) ─────────────────────
echo "## context_crosscall — same output 3x triggers redundancy" | tee -a "$REPORT"
TOTAL=$((TOTAL + 1))
DIR=$(bench_dir)
OUT1=$(SQUEEZ_DIR="$DIR" "$SQUEEZ" wrap "cat $FIXTURES/context_crosscall_1.txt" 2>&1 || true)
OUT2=$(SQUEEZ_DIR="$DIR" "$SQUEEZ" wrap "cat $FIXTURES/context_crosscall_2.txt" 2>&1 || true)
OUT3=$(SQUEEZ_DIR="$DIR" "$SQUEEZ" wrap "cat $FIXTURES/context_crosscall_3.txt" 2>&1 || true)
assert_not_contains "first call has full output" "$OUT1" "[squeez: identical to"
assert_contains "second call shows redundancy reference" "$OUT2" "[squeez: identical to"
assert_contains "third call shows redundancy reference" "$OUT3" "[squeez: identical to"
rm -rf "$DIR"
echo | tee -a "$REPORT"

# ── Summary ───────────────────────────────────────────────────────────────
echo | tee -a "$REPORT"
PASSED=$((TOTAL - FAIL))
echo "PASS: $PASSED/$TOTAL  FAIL: $FAIL/$TOTAL" | tee -a "$REPORT"
[ "$FAIL" -eq 0 ]
