#!/usr/bin/env bash
# squeez status line for Claude Code
# Shows current-session stats, falls back to all-time aggregate.

SQUEEZ_DIR="${SQUEEZ_DIR:-$HOME/.claude/squeez}"

python3 - "$SQUEEZ_DIR" << 'PYEOF'
import json, os, sys, glob

squeez_dir = sys.argv[1]
sessions_dir = f'{squeez_dir}/sessions'
curr_path = f'{sessions_dir}/current.json'

hooks_ok = False
try:
    s = json.load(open(os.path.expanduser('~/.claude/settings.json')))
    hooks_ok = bool(s.get('PreToolUse'))
except:
    pass

def read_session(jsonl_path):
    """Return (calls, total_in, total_out, redundant) from a session JSONL."""
    calls = total_in = total_out = redundant = 0
    try:
        for line in open(jsonl_path):
            try:
                d = json.loads(line)
                if d.get('type') == 'bash' and d.get('in_tk', 0) > 0:
                    total_in  += d['in_tk']
                    total_out += d.get('out_tk', 0)
                    calls     += 1
                    if d.get('out_tk', 0) < d['in_tk'] * 0.05:
                        redundant += 1
            except:
                pass
    except:
        pass
    return calls, total_in, total_out, redundant

def fmt_k(n):
    return f'{n/1000:.1f}K' if n >= 1000 else str(n)

try:
    curr = json.load(open(curr_path))
    session_file = curr.get('session_file', '')

    # --- Current session ---
    cur_calls = cur_in = cur_out = cur_red = 0
    if session_file:
        jsonl = f'{sessions_dir}/{session_file}'
        if os.path.exists(jsonl):
            cur_calls, cur_in, cur_out, cur_red = read_session(jsonl)

    # --- All-time aggregate (all *.jsonl files) ---
    all_calls = all_in = all_out = all_red = 0
    for f in glob.glob(f'{sessions_dir}/*.jsonl'):
        c, i, o, r = read_session(f)
        all_calls += c; all_in += i; all_out += o; all_red += r

    all_saved = max(0, all_in - all_out)
    all_pct   = int(all_saved * 100 / all_in) if all_in > 0 else 0

    if cur_calls > 0:
        # Show current session stats + all-time context
        cur_saved = max(0, cur_in - cur_out)
        cur_pct   = int(cur_saved * 100 / cur_in) if cur_in > 0 else 0
        parts = [f'squeez ↓{cur_pct}%', f'{cur_calls} calls', f'{fmt_k(cur_saved)} tk saved']
        if cur_red:
            parts.append(f'{cur_red} deduped')
        if all_calls > cur_calls:
            parts.append(f'all-time: {fmt_k(all_saved)} saved')
        print(' | '.join(parts))
    elif all_calls > 0:
        # No current-session data — show all-time stats with label
        parts = [f'squeez ✓', f'all-time: ↓{all_pct}%', f'{all_calls} calls', f'{fmt_k(all_saved)} tk saved']
        if all_red:
            parts.append(f'{all_red} deduped')
        print(' | '.join(parts))
    else:
        status = '✓ active' if hooks_ok else '⚠ restart to activate'
        print(f'squeez {status}')

except:
    status = '✓ active' if hooks_ok else '⚠ restart to activate'
    print(f'squeez {status}')
PYEOF
