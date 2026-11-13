#!/usr/bin/env bash
# squeez SubagentStop hook — feeds sub-agent final output into SessionContext.
#
# SubagentStop fires when a sub-agent spawned via Agent/Task completes. The
# logic lives in `squeez hook subagent-stop` (src/commands/hook/lifecycle.rs):
# record the sub-agent's real cost from its transcript, release one slot of
# the spawn ceiling, feed last_assistant_message into SessionContext so the
# parent can dedup against what the sub-agent already saw, and advise when the
# return was oversized.
SQUEEZ="${SQUEEZ_BIN:-$HOME/.claude/squeez/bin/squeez}"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

"$SQUEEZ" hook subagent-stop 2>/dev/null || true
