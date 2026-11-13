#!/usr/bin/env bash
# squeez Copilot CLI PostToolUse hook — tracks token usage per tool call and
# feeds the result into SessionContext for cross-call dedup. The logic lives
# in `squeez hook copilot-post-tool-use` (src/commands/hook/shell.rs).
SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

export SQUEEZ_DIR="$HOME/.copilot/squeez"

"$SQUEEZ" hook copilot-post-tool-use 2>/dev/null || true
