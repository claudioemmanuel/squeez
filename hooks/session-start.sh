#!/usr/bin/env bash
# squeez SessionStart hook — runs squeez init, prints memory banner to session context
SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0
"$SQUEEZ" init

# Rate-limited update check (at most once per day).
# Outputs a notification when a new squeez version is available.
_uc_ts="$HOME/.claude/squeez/.update-check-ts"
_now=$(date +%s 2>/dev/null || echo 0)
_last=0
[ -f "$_uc_ts" ] && _last=$(cat "$_uc_ts" 2>/dev/null || echo 0)
_diff=$(( _now - _last )) 2>/dev/null || _diff=99999
if [ "$_diff" -gt 86400 ]; then
    echo "$_now" > "$_uc_ts" 2>/dev/null || true
    _uc_out=$("$SQUEEZ" update --check 2>/dev/null || true)
    if echo "$_uc_out" | grep -q "→"; then
        printf "\n[squeez] Update available: %s\nRun: squeez update\n" "$_uc_out"
    fi
fi
