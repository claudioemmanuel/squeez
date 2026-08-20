#!/usr/bin/env bash
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
        print(json.dumps({'hookSpecificOutput': {'hookEventName': 'PreToolUse', 'permissionDecision': 'allow', 'updatedInput': d['tool_input']}}))
        sys.exit(0)

    # Security gate (#150): only rewrite to 'squeez wrap …' when squeez deems
    # it safe. Risky (rm -rf, git push --force, …), bypassed, or wrap-disabled
    # commands are left UNTOUCHED — we emit no updatedInput and no
    # permissionDecision, so the host evaluates the user's native deny/ask
    # rules against the ORIGINAL command instead of the wrapper. Fail-safe: any
    # error in the check means we do not wrap (and do not silently allow).
    try:
        if subprocess.run([squeez, 'should-wrap', cmd], timeout=2).returncode != 0:
            sys.exit(0)
    except Exception:
        sys.exit(0)

    d['tool_input']['command'] = squeez + ' wrap ' + shlex.quote(cmd)
    print(json.dumps({'hookSpecificOutput': {'hookEventName': 'PreToolUse', 'permissionDecision': 'allow', 'updatedInput': d['tool_input']}}))
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
                print(json.dumps({'hookSpecificOutput': {'hookEventName': 'PreToolUse', 'permissionDecision': 'allow', 'updatedInput': d['tool_input']}}))
                sys.exit(0)
    except Exception:
        pass  # budget enforcement is best-effort
    sys.exit(0)

# ── Agent/Task: compress prompt ───────────────────────────────────────
if tool in ('Agent', 'Task'):
    prompt = d.get('tool_input', {}).get('prompt')
    if isinstance(prompt, str) and prompt:
        try:
            result = subprocess.run(
                [squeez, 'compress-prompt'],
                input=prompt, capture_output=True, text=True, timeout=5,
            )
            compressed = result.stdout
            # Only replace when compression actually shrinks the prompt.
            if compressed and len(compressed) < len(prompt):
                d['tool_input']['prompt'] = compressed
                print(json.dumps({'hookSpecificOutput': {'hookEventName': 'PreToolUse', 'permissionDecision': 'allow', 'updatedInput': d['tool_input']}}))
                sys.exit(0)
        except Exception:
            pass
    sys.exit(0)

# ── All other tools: pass through ─────────────────────────────────────
sys.exit(0)
"
