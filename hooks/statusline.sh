#!/usr/bin/env bash
# squeez statusline — emits a compact status line for Claude Code statusLine hook.
# Reads session data from ~/.claude/squeez/ and outputs a single line.

SQUEEZ_DIR="${SQUEEZ_DIR:-$HOME/.claude/squeez}"
SESSION_FILE="$SQUEEZ_DIR/sessions/current.json"
CONTEXT_FILE="$SQUEEZ_DIR/sessions/context.json"
SUMMARIES_FILE="$SQUEEZ_DIR/memory/summaries.jsonl"

# Bail silently if no session data
if [[ ! -f "$SESSION_FILE" ]]; then
    exit 0
fi

# Read session fields
saved_tokens=0
total_calls=0
if command -v python3 &>/dev/null; then
    read -r saved_tokens total_calls < <(python3 -c "
import json
try:
    with open('$SESSION_FILE') as f:
        d = json.load(f)
    print(d.get('tokens_saved', 0), d.get('total_calls', 0))
except Exception:
    print(0, 0)
")
fi

# Read agent_spawns and total_in from context.json
agent_spawns=0
total_in=0
if [[ -f "$CONTEXT_FILE" ]] && command -v python3 &>/dev/null; then
    read -r agent_spawns total_in < <(python3 -c "
import json
try:
    with open('$CONTEXT_FILE') as f:
        d = json.load(f)
    spawns = d.get('agent_spawns', 0)
    tin = (d.get('tokens_bash', 0) + d.get('tokens_read', 0)
           + d.get('tokens_grep', 0) + d.get('tokens_other', 0))
    print(spawns, tin)
except Exception:
    print(0, 0)
")
fi

# Read efficiency_overall_bp from last line of summaries.jsonl
efficiency_bp=""
if [[ -f "$SUMMARIES_FILE" ]] && command -v python3 &>/dev/null; then
    efficiency_bp=$(python3 -c "
import json
try:
    with open('$SUMMARIES_FILE') as f:
        lines = f.readlines()
    if lines:
        last = json.loads(lines[-1].strip())
        val = last.get('efficiency_overall_bp')
        if val is not None:
            print(val)
except Exception:
    pass
")
fi

# Build output line
parts=("squeez")

if [[ "$saved_tokens" -gt 0 ]]; then
    if [[ "$total_in" -gt 0 ]]; then
        ratio=$(python3 -c "print(f'{$saved_tokens*100/$total_in:.0f}')" 2>/dev/null || echo "")
        parts+=("| -${saved_tokens}tk ${ratio}%")
    else
        parts+=("| -${saved_tokens}tk")
    fi
fi

if [[ "$total_calls" -gt 0 ]]; then
    parts+=("| ${total_calls} calls")
fi

if [[ "$agent_spawns" -gt 0 ]]; then
    parts+=("| ${agent_spawns} agents")
fi

if [[ -n "$efficiency_bp" ]] && [[ "$efficiency_bp" -gt 0 ]]; then
    eff_pct=$(python3 -c "print(f'{$efficiency_bp/100:.1f}')" 2>/dev/null || echo "")
    if [[ -n "$eff_pct" ]]; then
        parts+=("| Eff: ${eff_pct}%")
    fi
fi

echo "${parts[*]}"
