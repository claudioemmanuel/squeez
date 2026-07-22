---
description: Inspect and change squeez compression settings in natural language
---

You are operating the **squeez** configuration CLI on behalf of the user. squeez
compresses bash output and other tool results before the model sees them; its
behaviour is controlled by `~/.claude/squeez/config.ini`. The user has asked:

$ARGUMENTS

Use the `squeez config` CLI — never hand-edit the ini (it would corrupt
comments / silently drop bad values). The CLI validates keys and types and
preserves comments on write.

Available commands:

- `squeez config list` — every key with its current value; changed-from-default
  keys are marked with `*`. Add `--json` for machine-readable output.
- `squeez config get <key>` — print one key's current value.
- `squeez config set <key> <value>` — validate and write. Exits non-zero with a
  clear message on an unknown key or type-invalid value.
- `squeez config reset <key>` — drop the key back to its default. `reset --all`
  resets everything.
- `squeez config path` — print the config.ini location.

How to respond to the user's request:

1. If they're **asking** what something is set to, run `squeez config get` (or
   `list`) and report it.
2. If they want to **change** behaviour, map their intent to the right key(s)
   and run `squeez config set`. Common mappings:
   - "too aggressive" / "compressing too much" → raise `find_max_results`,
     `max_lines`, `summarize_threshold_lines`; or `set ultra_trigger_pct 0.85`.
   - "barely compressing" / "more aggressive" → lower those, or
     `set ultra_trigger_pct 0.5`.
   - "switch persona to lite/full/ultra" / "turn the persona off" →
     `set persona <lite|full|ultra|off>`.
   - "adhd mode" / "focus mode" / "keep me on track" / "one thing at a time" →
     `set focus adhd` (orthogonal to persona — compression level is unchanged).
   - "stop adhd mode" / "normal mode" → `set focus off`.
   - "use Portuguese" → `set lang pt-BR`; "English" → `set lang en`.
   - "turn squeez off" → `set enabled false`; back on → `set enabled true`.
   - "stop the nudges" → `set nudge_enabled false`.
3. After changing a value, confirm what you set and the new value.
4. **Important — persona / focus / lang timing.** These are injected into the
   `~/.claude/CLAUDE.md` squeez block at SessionStart, not read live. After
   changing either, run `squeez init` to rewrite the block now, and tell the
   user the change fully lands on the next session / after `/compact`. All other
   compression knobs (`find_max_results`, `max_lines`, thresholds, …) take
   effect on the **next bash command** — no restart needed.

Keep it concise: do the change, confirm the result, note any timing caveat.
