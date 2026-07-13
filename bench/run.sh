#!/usr/bin/env bash
set -euo pipefail
# Use local dev build if available, otherwise fall back to installed binary
if [ -x "$(dirname "$0")/../target/release/squeez" ]; then
    SQUEEZ="$(cd "$(dirname "$0")/.." && pwd)/target/release/squeez"
elif [ -x "$HOME/.claude/squeez/bin/squeez" ]; then
    SQUEEZ="$HOME/.claude/squeez/bin/squeez"
else
    echo "ERROR: squeez binary not found. Run 'cargo build --release' first." >&2
    exit 1
fi
FIXTURES="$(dirname "$0")/fixtures"
REPORT="$(dirname "$0")/report.md"
FAIL=0; TOTAL=0

printf "%-35s %8s %8s %10s %8s %6s\n" FIXTURE BEFORE AFTER REDUCTION LATENCY STATUS > "$REPORT"
printf '%.0s─' {1..78} >> "$REPORT"; echo >> "$REPORT"

for f in "$FIXTURES"/*.txt; do
    name=$(basename "$f")
    # context_crosscall_* fixtures exercise wrap.rs cross-call dedup; they
    # are run by bench/run_context.sh, not by filter-mode bench.
    # Synthetic fixtures (cargo_build, tsc_errors, verbose_log, repetitive,
    # kubectl_pods) are exercised by `squeez benchmark`, not this script —
    # their expected reduction ratios and latency bounds differ.
    case "$name" in
        context_crosscall_*) continue ;;
        cargo_build.txt)     continue ;;
        tsc_errors.txt)      continue ;;
        verbose_log.txt)     continue ;;
        repetitive.txt)      continue ;;
        kubectl_pods.txt)    continue ;;
    esac
    input=$(cat "$f")
    before=$(( ${#input} / 4 ))
    [ "$before" -eq 0 ] && continue

    # Derive handler hint from fixture name: "git_log_200.txt" → hint="git"
    hint="${name%%_*}"

    # mdcompress_* fixtures use the markdown compressor instead of filter.
    # Prose is locale-specific: a *ptbr* fixture must be compressed with the
    # pt-BR word list, else the EN compressor barely touches it.
    if [ "$hint" = "mdcompress" ]; then
        lang_flag=""
        case "$name" in *ptbr*|*pt_br*|*pt-br*) lang_flag="--lang pt-BR" ;; esac
        t0=$(date +%s%N)
        compressed=$("$SQUEEZ" compress-md --dry-run --ultra --quiet $lang_flag "$f" 2>/dev/null || cat "$f")
        t1=$(date +%s%N)
    else
        t0=$(date +%s%N)
        compressed=$(echo "$input" | "$SQUEEZ" filter "$hint" 2>/dev/null || echo "$input")
        t1=$(date +%s%N)
    fi
    ms=$(( (t1 - t0) / 1000000 ))

    after=$(( ${#compressed} / 4 ))
    pct=$(( 100 - (after * 100 / before) ))
    # mdcompress fixtures: prose compression is naturally lighter (~10-30%).
    # Floor was 15 until E6 (bench/substitutions_snapshot.json): EN
    # letter/word substitutions (with->w/, function->fn, etc.) were pruned
    # from ultra_subs because they measured COSTS/NEUTRAL against real
    # tokenizers (o200k_base/cl100k_base) -- but they DID shrink raw char
    # count, which is all this script's chars/4 proxy sees. Removing them
    # correctly dropped mdcompress_en_prose.txt's proxy-measured ratio from
    # ~18% to 14% even though real token savings are unaffected or better.
    # 10 keeps a real floor (catches genuine no-ops) without re-penalizing
    # that honest, tokenizer-validated fix.
    threshold=30
    if [ "$hint" = "mdcompress" ]; then threshold=10; fi
    status="✅"; [ "$pct" -lt "$threshold" ] && { status="❌"; FAIL=$((FAIL+1)); }
    [ "$ms" -gt 100 ] && { status="❌ slow"; FAIL=$((FAIL+1)); }
    TOTAL=$((TOTAL+1))

    printf "%-35s %7stk %7stk %9s%% %7sms  %s\n" "$name" "$before" "$after" "$pct" "$ms" "$status" >> "$REPORT"
done

echo >> "$REPORT"
echo "PASS: $((TOTAL-FAIL))/$TOTAL  FAIL: $FAIL/$TOTAL" >> "$REPORT"
cat "$REPORT"
[ "$FAIL" -eq 0 ]
