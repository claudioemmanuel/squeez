#!/usr/bin/env bash
# squeez Gemini CLI AfterTool hook — records tool results into the squeez
# SessionContext (file-path, error, git-event tracking).
#
# Registered in ~/.gemini/settings.json under hooks.AfterTool.
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
[ -z "$SQUEEZ_PY" ] && exit 0

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

export SQUEEZ_DIR="$HOME/.gemini/squeez"

"$SQUEEZ_PY" -c "
import json, sys, subprocess, os

data = sys.stdin.read()
if not data.strip():
    sys.exit(0)
try:
    d = json.loads(data)
except json.JSONDecodeError:
    sys.exit(0)

tool = d.get('tool_name') or d.get('tool') or 'unknown'
try:
    subprocess.run(
        [os.environ.get('SQUEEZ') or '$SQUEEZ', 'track-result', tool],
        input=data,
        timeout=3,
        check=False,
    )
except Exception:
    pass
"
