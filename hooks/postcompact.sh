#!/usr/bin/env bash
# squeez PostCompact hook — re-arms squeez after context compaction.
#
# PostCompact fires after Claude Code compacts the context window. The model's
# context is now shorter; session memory injected at SessionStart may have
# been trimmed. We log the event and emit a brief re-arm reminder so the
# model knows compression is still active for the remainder of the session.
set -euo pipefail

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

"$SQUEEZ" track PostCompact 0 2>/dev/null || true

# Emit a terse re-arm note. Claude Code may surface this to the model
# as a system-level context injection depending on hook output handling.
printf '[squeez] context compacted — compression still active\n'
