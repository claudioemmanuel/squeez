#!/usr/bin/env bash
# squeez PostCompact hook — re-injects session state after context compaction.
#
# PostCompact fires after Claude Code compacts the context window. Compaction
# can drop concrete state (files touched, errors hit, git refs). squeez already
# tracks that, so we re-inject it as `additionalContext` — the documented,
# reliable way to add context that survives into the compacted session — plus
# pointers to any squeez_retrieve blobs holding dropped output.
set -euo pipefail

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

"$SQUEEZ" track PostCompact 0 2>/dev/null || true

# Emit the PostCompact hookSpecificOutput JSON (or nothing if there's no state
# worth restoring). This is the reliable injection path; a plain echo is not
# guaranteed to reach the model's context.
"$SQUEEZ" compact-summary 2>/dev/null || true
