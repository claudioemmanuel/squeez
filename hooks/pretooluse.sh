#!/usr/bin/env bash
set -euo pipefail

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
[ ! -x "$SQUEEZ" ] && exit 0

python3 -c "
import sys, json, os

data = sys.stdin.read()
d = json.loads(data)
if d.get('tool_name') != 'Bash':
    sys.exit(0)

cmd = d['tool_input']['command']
squeez = os.path.expanduser('~/.claude/squeez/bin/squeez')

if cmd.startswith(squeez):
    sys.exit(0)

if cmd.startswith('--no-squeez '):
    d['tool_input']['command'] = cmd[len('--no-squeez '):]
    print(json.dumps({'hookSpecificOutput': {'permissionDecision': 'allow', 'updatedInput': d['tool_input']}}))
    sys.exit(0)

d['tool_input']['command'] = squeez + ' wrap ' + cmd
print(json.dumps({'hookSpecificOutput': {'permissionDecision': 'allow', 'updatedInput': d['tool_input']}}))
"
