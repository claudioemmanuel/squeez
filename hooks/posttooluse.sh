#!/usr/bin/env bash
# squeez PostToolUse hook — tracks token usage and rewrites tool output when
# content is redundant or oversized (Claude Code v2.1.119+ updatedToolOutput).
# Bash compression is handled at PreToolUse via `squeez wrap`; this hook covers
# Read / Grep / Glob / MCP and the other tools whose output bypasses it.
#
# The logic lives in `squeez hook post-tool-use` (src/commands/hook/mod.rs):
# track → compress-output → track-result, in that order, in one process. It
# used to be two Python runs plus three squeez processes, ~100ms per call.
SQUEEZ="${SQUEEZ_BIN:-$HOME/.claude/squeez/bin/squeez}"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

SQUEEZ_BIN="$SQUEEZ" "$SQUEEZ" hook post-tool-use 2>/dev/null || true
