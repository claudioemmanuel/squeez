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

"$SQUEEZ" track PostCompact 0 >/dev/null 2>&1 || true
