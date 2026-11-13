#!/usr/bin/env bash
# squeez SessionStart hook — runs squeez init, prints memory banner to session context

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0
# The SessionStart payload on stdin has two readers, so capture it once.
# Only when stdin is not a terminal: run by hand, there is nothing to read.
_payload=""
if [ ! -t 0 ]; then
    _payload=$(cat 2>/dev/null || true)
fi
# After /compact (source=compact on the stdin payload), restore squeez's
# session state first: it must read the pre-compaction session before init
# rolls it over, and SessionStart stdout is what reaches the model — PostCompact
# output does not (#225). Prints nothing on any other start.
printf '%s' "$_payload" | "$SQUEEZ" compact-summary --session-start 2>/dev/null || true
# `--stdin` records the payload's session_id and cwd on the new squeez
# session (#286), so it can be tied back to the chat that started it.
printf '%s' "$_payload" | "$SQUEEZ" init --stdin

# Hook health check: warn if squeez hooks were removed from settings.json
# (e.g. by another tool like OMC that overwrites the file on setup).
"$SQUEEZ" hook settings-check 2>/dev/null || true

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
        printf "\n[squeez] Update available: %s\nRun: squeez update\n" "$_uc_out" >&2
    fi
fi
