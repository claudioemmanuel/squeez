"""Hermes plugin adapter for squeez output compression.

When installed via `squeez setup --host=hermes`, this plugin is dropped into
$HERMES_HOME/plugins/squeez-fallback/__init__.py, defaulting to ~/.hermes when
that variable is unset. Hermes discovers plugins on session start, but
directory plugins are opt-in: enable it once with
`hermes plugins enable squeez-fallback`.

The plugin hooks transform_terminal_output and passes the output of every
terminal command through `squeez filter`. The command itself is not
rewritten, so piped and chained commands are compressed too. The full
output is saved, redacted, and the compressed text ends with its path. Set
SQUEEZ_OUTPUT_DISABLE=1 to leave output as is. Commands another plugin has
already rewritten are skipped so the first wrapper keeps priority -- set
SQUEEZ_HERMES_NEVER_WRAP to a comma-separated list of prefixes to declare
those, e.g. SQUEEZ_HERMES_NEVER_WRAP="foo ,bar ".

Requires: squeez binary in PATH (cargo install squeez or npm install -g squeez)
or at $SQUEEZ_BIN
"""

from __future__ import annotations

import hashlib
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

_MIN_SAVING = 0.20
_TIMEOUT_S = 10
_KEEP_RAW_S = 24 * 3600

_BYPASS = ": --no-squeez; "

# Commands whose output is never compressed (shell builtins such as cd and
# export needed skipping only while commands were wrapped)
_NEVER_WRAP = (
    "squeez ",       # already squeez-handled (prevents double-wrap)
    "--no-squeez ",  # explicit bypass prefix
    _BYPASS,         # bypass prefix after _pre_tool_call rewrote it
    "hermes ",       # Hermes CLI commands — avoid recursion
)


def _never_wrap_prefixes():
    """_NEVER_WRAP plus any prefixes declared in SQUEEZ_HERMES_NEVER_WRAP.

    Another Hermes plugin may rewrite a command before this hook runs;
    compressing its output again would double-compress it. Which prefixes those
    are depends on what else the user has installed, so they are declared
    rather than guessed.
    """
    extra = os.environ.get("SQUEEZ_HERMES_NEVER_WRAP", "")
    declared = tuple(p for p in (part.strip() for part in extra.split(",")) if p)
    return _NEVER_WRAP + declared


def register(ctx):
    """Register the Hermes output callback."""
    if not _squeez_bin():
        _warn("squeez binary not found; output is left as is until it is installed")
    ctx.register_hook("pre_tool_call", _pre_tool_call)
    ctx.register_hook("transform_terminal_output", _transform_terminal_output)


def _pre_tool_call(tool_name=None, args=None, **_kwargs):
    """Turn `--no-squeez cmd` into a command the shell can run."""
    if tool_name != "terminal" or not isinstance(args, dict):
        return None
    command = args.get("command")
    if not isinstance(command, str) or not command.lstrip().startswith("--no-squeez "):
        return None
    rest = command.lstrip()[len("--no-squeez "):]
    return {"action": "modify", "args": {"command": _BYPASS + rest}}


def _hermes_home() -> Path:
    try:
        from hermes_constants import get_hermes_home
        return Path(get_hermes_home())
    except Exception:
        return Path(os.environ.get("HERMES_HOME") or Path.home() / ".hermes")


def _squeez_bin() -> str | None:
    """Return the squeez binary path, or None when it is missing."""
    for cand in (os.environ.get("SQUEEZ_BIN"), shutil.which("squeez"),
                 str(_hermes_home() / "squeez" / "bin" / "squeez"),
                 str(Path.home() / ".claude" / "squeez" / "bin" / "squeez")):
        if cand and os.path.isfile(cand) and os.access(cand, os.X_OK):
            return cand
    return None


def _min_chars() -> int:
    try:
        return int(os.environ.get("SQUEEZ_OUTPUT_MIN_CHARS", "4000"))
    except ValueError:
        return 4000


def _raw_dir() -> Path:
    d = _hermes_home() / "cache" / "squeez-raw"
    d.mkdir(parents=True, exist_ok=True)
    try:
        os.chmod(d, 0o700)
    except OSError:
        pass
    return d


def _prune(d: Path) -> None:
    cutoff = time.time() - _KEEP_RAW_S
    for p in d.glob("*.txt"):
        try:
            if p.stat().st_mtime < cutoff:
                p.unlink()
        except OSError:
            pass


def _save_raw(command: str, output: str) -> Path:
    # The hook runs before Hermes redacts, so only a redacted copy may reach disk.
    try:
        from agent.redact import redact_terminal_output
        from tools.ansi_strip import strip_ansi
        text = redact_terminal_output(strip_ansi(output), command)
    except Exception:
        raise RuntimeError("redactor unavailable")
    d = _raw_dir()
    _prune(d)
    key = hashlib.sha256((command + "\0" + output).encode("utf-8", "replace")).hexdigest()[:16]
    path = d / f"{key}.txt"
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | getattr(os, "O_NOFOLLOW", 0), 0o600)
    with os.fdopen(fd, "w", encoding="utf-8", errors="replace") as f:
        f.write(text)
    return path


def compress(command: object, output: object) -> str | None:
    if os.environ.get("SQUEEZ_OUTPUT_DISABLE") == "1":
        return None
    if not isinstance(output, str) or len(output) < _min_chars():
        return None
    # Skip if already wrapped or shouldn't be wrapped
    if not isinstance(command, str) or command.strip().startswith(_never_wrap_prefixes()):
        return None
    exe = _squeez_bin()
    if not exe:
        return None
    env = dict(os.environ)
    env.setdefault("SQUEEZ_DIR", str(_hermes_home() / "squeez"))
    # Filter with squeez — squeez filter picks the handler for the command
    # (a user filter rule first, else the built-in one) and compresses stdin
    proc = subprocess.run([exe, "filter", command], input=output, capture_output=True,
                          text=True, errors="replace", timeout=_TIMEOUT_S, env=env)
    small = proc.stdout.rstrip("\n")
    if proc.returncode != 0 or not small.strip():
        return None
    if len(small) > len(output) * (1 - _MIN_SAVING):
        return None
    path = _save_raw(command, output)
    return (f"{small}\n[squeez: output compressed {len(output)}→{len(small)} chars. "
            f"Full output: {path} (read_file it if something you need is missing)]")


def _transform_terminal_output(command=None, output=None, **_kw):
    """Compress a terminal command's output with squeez, or leave it as is."""
    try:
        return compress(command, output)
    except Exception as exc:  # fail open: the model gets the raw output
        _warn(str(exc))
        return None


def _warn(message):
    print(f"squeez: hermes plugin warning: {message}", file=sys.stderr)
