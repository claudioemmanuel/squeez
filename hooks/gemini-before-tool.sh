#!/usr/bin/env bash
# squeez Gemini CLI BeforeTool hook — wraps bash commands with `squeez wrap`
# and prints an informational note before Read/Grep/Glob tools.
#
# Gemini's BeforeTool rewrite schema is not yet fully documented upstream
# (tracked in google-gemini/gemini-cli#14449), so Read/Grep/Glob
# enforcement here is deliberately soft: we rely on the GEMINI.md block
# written by `squeez init` to nudge the model to cap output.
#
# Registered in ~/.gemini/settings.json under hooks.BeforeTool.
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

SQUEEZ_BIN="$SQUEEZ" "$SQUEEZ_PY" -c "
import json, os, shlex, subprocess, sys

data = sys.stdin.read()
if not data.strip():
    sys.exit(0)

try:
    d = json.loads(data)
except json.JSONDecodeError:
    sys.exit(0)

# Gemini events name the tool under tool_name (Claude Code convention).
tool = d.get('tool_name') or d.get('tool') or ''
squeez = os.environ['SQUEEZ_BIN']

if tool in ('bash', 'Bash', 'run_shell_command'):
    inp = d.get('tool_input') or d.get('input') or {}
    cmd = inp.get('command') or inp.get('cmd')
    if not cmd or not isinstance(cmd, str):
        sys.exit(0)
    if cmd.startswith(squeez) or 'squeez wrap' in cmd:
        sys.exit(0)
    if cmd.startswith('--no-squeez'):
        inp['command'] = cmd[len('--no-squeez'):].lstrip()
        print(json.dumps({'decision': 'allow', 'updatedInput': inp}))
        sys.exit(0)
    # Security gate (#150): leave risky/bypassed/wrap-disabled commands untouched
    # so the host's native permission flow applies to the original command.
    try:
        if subprocess.run([squeez, 'should-wrap', cmd], timeout=2).returncode != 0:
            sys.exit(0)
    except Exception:
        sys.exit(0)
    inp['command'] = squeez + ' wrap ' + shlex.quote(cmd)
    print(json.dumps({'decision': 'allow', 'updatedInput': inp}))
    sys.exit(0)

# Read/Grep/Glob: pass-through (soft-budget via GEMINI.md).
sys.exit(0)
"
