# Security Policy

## Reporting a vulnerability

Please report security issues privately via **GitHub Security Advisories**
(the "Report a vulnerability" button under the repository's *Security* tab),
not in public issues. We aim to acknowledge reports within a few days.

## Bash command wrapping & host permission rules

squeez compresses Bash output by rewriting a proposed command to
`squeez wrap '<original command>'` in the host's PreToolUse hook. Two properties
of that mechanism are security-relevant, so they're documented here explicitly.

### What squeez does

For an ordinary command, the PreToolUse hook rewrites it to `squeez wrap '…'`
and returns `permissionDecision: "allow"`, so the wrapped command runs and its
output is compressed.

### The risk

Because the host matches permission rules (`allow`/`ask`/`deny`) against the
string that actually executes, a rule written against the original command
(e.g. `Bash(git push *)`) does not match the rewritten `squeez wrap '…'`
string, and the hardcoded `allow` skips the host's default confirmation prompt.
(Reported in #150.)

### Mitigations (shipped)

squeez does **not** wrap a command when any of these hold — instead it leaves
the command untouched (no rewrite, no `permissionDecision`), so the host
evaluates your native `deny`/`ask` rules and default prompt against the
**original** command:

- **Risky commands.** Commands matching `bash_risk_patterns` (default: `rm -rf`,
  `git push --force`, `git reset --hard`, `npm/yarn/pnpm publish`, `dd if=`,
  `mkfs`, `> /dev/sd`, …) run unwrapped. Extend the list to match the rules you
  rely on:
  ```
  squeez config set bash_risk_patterns "rm -rf,git push --force,terraform apply,…"
  ```
- **Bypassed commands.** Anything in `bypass` (default: `ssh`, `psql`, `mysql`,
  `docker exec`) runs unwrapped.
- **Wrapping disabled.** Set `squeez config set wrap_bash false` to disable Bash
  wrapping entirely — no command is ever rewritten and your host's permission
  flow is fully intact (you lose Bash output compression in exchange).

The check is **fail-safe**: if it can't run, the command is left unwrapped.

### Residual

For commands that are *not* risky/bypassed and with `wrap_bash` on, squeez still
rewrites + allows, so a `deny`/`ask` rule on such a command won't fire while
squeez is active. If you depend on host permission rules for the full command
surface, set `wrap_bash false`. A deeper fix (compressing Bash output in
PostToolUse so the original command runs under native permission evaluation)
is tracked as a follow-up; it has a capture-before-host-truncation trade-off
still being evaluated.

## Sensitive-content guard (blobs & session state)

`retrieve::store()` (the `squeez_retrieve` blob cache) and the error-snippet
cache in `context.json` both persist tool output to disk. Before either write,
`context::sensitive::is_sensitive()` scans for credential shapes (PEM private
keys, AWS access/secret keys, GitHub/Slack/OpenAI-style tokens, `Authorization:
Bearer` headers, bulk dotenv-style secrets) and refuses the write if any hit —
so a `cat .env` or a leaked key is never stashed verbatim. This guard is
default-on and not currently configurable.
