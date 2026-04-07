#!/usr/bin/env bash
set -euo pipefail

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

SQUEEZ_BIN="$SQUEEZ" python3 -c "
import sys, json, os, shlex

data = sys.stdin.read()
if not data.strip():
    sys.exit(0)

try:
    d = json.loads(data)
except json.JSONDecodeError:
    sys.exit(0)

if d.get('tool_name') != 'Bash':
    sys.exit(0)

cmd = d.get('tool_input', {}).get('command')
if cmd is None:
    sys.exit(0)

squeez = os.environ['SQUEEZ_BIN']

if cmd.startswith(squeez):
    sys.exit(0)

if cmd.startswith('--no-squeez '):
    d['tool_input']['command'] = cmd[len('--no-squeez '):]
    print(json.dumps({'hookSpecificOutput': {'permissionDecision': 'allow', 'updatedInput': d['tool_input']}}))
    sys.exit(0)

d['tool_input']['command'] = squeez + ' wrap ' + shlex.quote(cmd)
print(json.dumps({'hookSpecificOutput': {'permissionDecision': 'allow', 'updatedInput': d['tool_input']}}))
"
