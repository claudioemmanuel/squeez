#!/usr/bin/env bash
# squeez Copilot CLI session-start integration
# Initialises the session and injects memory into ~/.copilot/copilot-instructions.md
# Run this once per session: add to shell RC or invoke manually.
SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

export SQUEEZ_DIR="$HOME/.copilot/squeez"
mkdir -p "$SQUEEZ_DIR/sessions" "$SQUEEZ_DIR/memory"
chmod 700 "$SQUEEZ_DIR" "$SQUEEZ_DIR/sessions" "$SQUEEZ_DIR/memory" 2>/dev/null || true

"$SQUEEZ" init --copilot
