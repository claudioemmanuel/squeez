#!/usr/bin/env bash
set -euo pipefail

SQUEEZ="$HOME/.claude/squeez/bin/squeez"
if [ ! -x "$SQUEEZ" ]; then
    _sq=$(command -v squeez 2>/dev/null || true)
    [ -n "$_sq" ] && SQUEEZ="$_sq"
fi
[ ! -x "$SQUEEZ" ] && exit 0

SQUEEZ_BIN="$SQUEEZ" python3 -c "
import sys, json, os, shlex, subprocess

data = sys.stdin.read()
if not data.strip():
    sys.exit(0)

try:
    d = json.loads(data)
except json.JSONDecodeError:
    sys.exit(0)

tool = d.get('tool_name', '')
squeez = os.environ['SQUEEZ_BIN']

# ── Bash tool: wrap command with squeez ────────────────────────────────
if tool == 'Bash':
    cmd = d.get('tool_input', {}).get('command')
    if cmd is None:
        sys.exit(0)

    if cmd.startswith(squeez):
        sys.exit(0)

    if cmd.startswith('--no-squeez '):
        d['tool_input']['command'] = cmd[len('--no-squeez '):]
        print(json.dumps({'hookSpecificOutput': {'permissionDecision': 'allow', 'updatedInput': d['tool_input']}}))
        sys.exit(0)

    d['tool_input']['command'] = squeez + ' wrap ' + shlex.quote(cmd)
    print(json.dumps({'hookSpecificOutput': {'permissionDecision': 'allow', 'updatedInput': d['tool_input']}}))
    sys.exit(0)

# ── Read/Grep/Glob: inject budget limits ──────────────────────────────
if tool in ('Read', 'Grep', 'Glob'):
    try:
        result = subprocess.run(
            [squeez, 'budget-params', tool],
            capture_output=True, text=True, timeout=2,
        )
        out = result.stdout.strip()
        if out:
            patch = json.loads(out)
            inp = d.get('tool_input', {})
            changed = False
            for k, v in patch.items():
                if k not in inp:  # don't override explicit user values
                    inp[k] = v
                    changed = True
            if changed:
                d['tool_input'] = inp
                print(json.dumps({'hookSpecificOutput': {'permissionDecision': 'allow', 'updatedInput': d['tool_input']}}))
                sys.exit(0)
    except Exception:
        pass  # budget enforcement is best-effort
    sys.exit(0)

# ── All other tools: pass through ─────────────────────────────────────
sys.exit(0)
"
