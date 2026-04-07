#!/usr/bin/env bash
# squeez status line for Claude Code
# Format: ✓ Squeez | Ctx 40% | ↓94% | 200 Calls | 668.4K Tk Saved | 6 Deduped

SQUEEZ_DIR="${SQUEEZ_DIR:-$HOME/.claude/squeez}"
STATUS_INPUT=$(cat)  # forward input from statusLine chain

python3 - "$SQUEEZ_DIR" << 'PYEOF'
import json, os, sys, glob

squeez_dir = sys.argv[1]
sessions_dir = f'{squeez_dir}/sessions'
curr_path    = f'{sessions_dir}/current.json'

# ── ANSI colours ───────────────────────────────────────────────────
GREEN  = '\033[32m'
DIM    = '\033[2m'
RESET  = '\033[0m'
CHECK  = f'{GREEN}✓{RESET}'
WARN   = '\033[33m⚠{RESET}'

# ── helpers ────────────────────────────────────────────────────────
def read_session(path):
    calls = total_in = total_out = redundant = 0
    try:
        for line in open(path):
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
    return f'{n/1000:.1f}K' if n >= 1000 else str(int(n))

def get_context_pct():
    """Read last assistant turn from the most-recently-modified Claude session JSONL."""
    project_dir = os.path.expanduser('~/.claude/projects/')
    latest = None; latest_mtime = 0
    try:
        for proj in os.listdir(project_dir):
            pp = os.path.join(project_dir, proj)
            if not os.path.isdir(pp):
                continue
            for f in glob.glob(f'{pp}/*.jsonl'):
                mtime = os.path.getmtime(f)
                if mtime > latest_mtime:
                    latest_mtime = mtime; latest = f
    except:
        pass
    if not latest:
        return None, None
    last_usage = None
    try:
        with open(latest) as f:
            for line in f:
                try:
                    ev = json.loads(line)
                    if ev.get('type') == 'assistant':
                        u = ev.get('message', {}).get('usage', {})
                        if u:
                            last_usage = u
                except:
                    pass
    except:
        pass
    if last_usage:
        total = (last_usage.get('input_tokens', 0)
                 + last_usage.get('cache_creation_input_tokens', 0)
                 + last_usage.get('cache_read_input_tokens', 0))
        pct = int(total * 100 / 200_000)      # claude-sonnet-4-6 = 200K
        return total, min(pct, 100)
    return None, None

# ── hooks status ──────────────────────────────────────────────────
hooks_ok = False
try:
    s = json.load(open(os.path.expanduser('~/.claude/settings.json')))
    hooks_ok = bool(s.get('PreToolUse'))
except:
    pass

# ── squeez session data ───────────────────────────────────────────
try:
    curr = json.load(open(curr_path))
    session_file = curr.get('session_file', '')

    cur_calls = cur_in = cur_out = cur_red = 0
    if session_file:
        p = f'{sessions_dir}/{session_file}'
        if os.path.exists(p):
            cur_calls, cur_in, cur_out, cur_red = read_session(p)

    all_calls = all_in = all_out = all_red = 0
    for f in glob.glob(f'{sessions_dir}/*.jsonl'):
        c, i, o, r = read_session(f)
        all_calls += c; all_in += i; all_out += o; all_red += r

    all_saved = max(0, all_in - all_out)
    all_pct   = int(all_saved * 100 / all_in) if all_in > 0 else 0

    # ── context window (real-time from latest session JSONL) ──────
    ctx_total, ctx_pct = get_context_pct()
    ctx_str = f'Ctx {ctx_pct}%' if ctx_pct is not None else None

    # ── build output line ─────────────────────────────────────────
    parts = []

    if cur_calls > 0:
        cur_saved = max(0, cur_in - cur_out)
        cur_pct   = int(cur_saved * 100 / cur_in) if cur_in > 0 else 0
        parts.append(f'{CHECK} Squeez ↓{cur_pct}%')
        if ctx_str:
            parts.append(ctx_str)
        parts.append(f'{cur_calls} Calls')
        parts.append(f'{fmt_k(cur_saved)} Tk Saved')
        if cur_red:
            parts.append(f'{cur_red} Deduped')
        if all_calls > cur_calls:
            parts.append(f'All-time: {fmt_k(all_saved)} Saved')
    elif all_calls > 0:
        parts.append(f'{CHECK} Squeez')
        if ctx_str:
            parts.append(ctx_str)
        parts.append(f'All-time ↓{all_pct}%')
        parts.append(f'{all_calls} Calls')
        parts.append(f'{fmt_k(all_saved)} Tk Saved')
        if all_red:
            parts.append(f'{all_red} Deduped')
    else:
        label = 'Active' if hooks_ok else f'{WARN} Restart to Activate'
        parts.append(f'{CHECK} Squeez {label}')
        if ctx_str:
            parts.append(ctx_str)

    print(' | '.join(parts))

except:
    label = 'Active' if hooks_ok else f'\033[33m⚠\033[0m Restart to Activate'
    ctx_total, ctx_pct = get_context_pct()
    ctx_str = f'Ctx {ctx_pct}%' if ctx_pct is not None else ''
    suffix = f' | {ctx_str}' if ctx_str else ''
    print(f'{CHECK} Squeez {label}{suffix}')
PYEOF
