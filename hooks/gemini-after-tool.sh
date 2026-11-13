#!/usr/bin/env bash
# squeez Gemini CLI AfterTool hook — records results into SessionContext.
# The logic lives in `squeez hook record-result`.
#
# Registered in ~/.gemini/settings.json under hooks.AfterTool.
SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

export SQUEEZ_DIR="$HOME/.gemini/squeez"

"$SQUEEZ" hook record-result 2>/dev/null || true
