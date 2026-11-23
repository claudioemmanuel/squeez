#!/usr/bin/env bash
# squeez PostCompact hook — logs the compaction.
#
# Session state is restored from session-start.sh (source=compact), not here:
# Claude Code rejects hookSpecificOutput for PostCompact and only shows its
# stdout as a UI notice, so nothing emitted here can reach the model (#225).
# Keep stdout empty.
set -euo pipefail

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

_payload=""
[ ! -t 0 ] && _payload=$(cat 2>/dev/null || true)
# Bind to this chat's per-session context file (src/context/cache.rs).
_sid=$(printf '%s' "$_payload" | sed -n 's/.*"session_id"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
export SQUEEZ_SESSION="$_sid"
"$SQUEEZ" track PostCompact 0 >/dev/null 2>&1 || true
