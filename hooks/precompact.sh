#!/usr/bin/env bash
# squeez PreCompact hook — logs compaction events for session efficiency metrics.
#
# PreCompact fires before Claude Code compacts the context window.
# A hook can block compaction by exiting 2 or returning {"decision":"block"}.
# squeez does not block — compaction is healthy. We only record the event
# so session stats can show how often compaction occurred and at what call depth.
set -euo pipefail

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

"$SQUEEZ" track PreCompact 0 2>/dev/null || true
