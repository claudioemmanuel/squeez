#!/usr/bin/env bash
# squeez Codex CLI PreToolUse hook — rewrites bash/shell commands with
# `squeez wrap` before execution.
#
# Hook surface as of Codex 0.123.0: apply_patch now emits PreToolUse/PostToolUse
# (openai/codex#18391), but updatedInput is explicitly rejected by the runtime
# and read_file/grep still have no hook surface (openai/codex#18491).
# Soft budget for read_file/grep is communicated via ~/.codex/AGENTS.md,
# written by `squeez init --host=codex`.
#
# Registered in ~/.codex/hooks.json under hooks.PreToolUse.
set -euo pipefail

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

SQUEEZ_BIN="$SQUEEZ" python3 -c "
import json, os, shlex, sys

data = sys.stdin.read()
if not data.strip():
    sys.exit(0)
try:
    d = json.loads(data)
except json.JSONDecodeError:
    sys.exit(0)

tool = d.get('tool_name') or d.get('tool') or ''
if tool not in ('bash', 'Bash', 'shell', 'run_shell_command'):
    sys.exit(0)

inp = d.get('tool_input') or {}
cmd = inp.get('command')
if not cmd or not isinstance(cmd, str):
    sys.exit(0)

squeez = os.environ['SQUEEZ_BIN']
if cmd.startswith(squeez) or 'squeez wrap' in cmd or cmd.startswith('--no-squeez'):
    sys.exit(0)

inp['command'] = squeez + ' wrap ' + shlex.quote(cmd)
# Codex runtime explicitly rejects updatedInput for non-Bash PreToolUse
# (openai/codex#18491). For Bash/shell it still applies the rewrite.
print(json.dumps({'decision': 'allow', 'updatedInput': inp}))
"
