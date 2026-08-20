#!/usr/bin/env bash
# squeez Codex CLI SessionStart hook — finalizes previous session and
# refreshes ~/.codex/AGENTS.md, which Codex CLI auto-loads at session start.
#
# Registered in ~/.codex/hooks.json under hooks.SessionStart.
set -euo pipefail

# Resolve a working Python interpreter.
#
# Hardcoding python3 is not safe on Windows: python.org installs ship
# python.exe with no python3.exe, so the name resolves to the Microsoft Store
# App Execution Alias under %LOCALAPPDATA%\Microsoft\WindowsApps. That stub is
# on PATH and passes `command -v`, but exits non-zero when run — which left
# every squeez hook silently dead (issue #209). Probe by EXECUTING each
# candidate, not by locating it.
SQUEEZ_PY=""
for _c in python3 python py; do
    if command -v "$_c" >/dev/null 2>&1 && "$_c" -c "" >/dev/null 2>&1; then
        SQUEEZ_PY="$_c"
        break
    fi
done
# No interpreter, nothing this hook can parse — leave quietly rather than
# failing the host's session-start under `set -e`.
[ -z "$SQUEEZ_PY" ] && exit 0

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

export SQUEEZ_DIR="$HOME/.codex/squeez"

# `init` finalizes the previous session and prints the banner + memory block.
# Codex expects SessionStart context wrapped under hookSpecificOutput with
# `additionalContext`; raw stdout is accepted today but not the documented shape.
SQUEEZ_CONTEXT="$("$SQUEEZ" init --host=codex 2>/dev/null || true)"

if [ -n "$SQUEEZ_CONTEXT" ]; then
    SQUEEZ_CONTEXT="$SQUEEZ_CONTEXT" "$SQUEEZ_PY" -c '
import json, os
print(json.dumps({
    "hookSpecificOutput": {
        "hookEventName": "SessionStart",
        "additionalContext": os.environ["SQUEEZ_CONTEXT"],
    }
}))
'
fi
