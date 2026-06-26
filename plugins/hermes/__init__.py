"""Hermes plugin adapter for squeez fallback compression.

When installed via `squeez setup --host=hermes`, this plugin is dropped into
~/.hermes/plugins/squeez-fallback/__init__.py. Hermes auto-discovers plugins
on session start — no config file patching needed.

The plugin hooks pre_tool_call and wraps unrecognized terminal commands
with `squeez wrap`. Commands already rewritten by rtk (prefix "rtk ") are
skipped, so rtk's specialized handlers always take priority.

Requires: squeez binary in PATH (cargo install squeez or npm install -g squeez)
"""

import shutil
import subprocess
import sys

_squeez_available = None
_squeez_missing_warned = False

# Commands that should never be wrapped (would break functionality)
_NEVER_WRAP = (
    "rtk ",          # already rtk-handled — rtk's specialized filters are better
    "squeez ",       # already squeez-handled (prevents double-wrap)
    "--no-squeez ",  # explicit bypass prefix
    "cd ",           # wrapping breaks cwd persistence in Hermes terminal
    "export ",       # env var setting — must run in parent shell
    "source ",       # sourcing scripts — must run in parent shell
    "eval ",         # eval — must run in parent shell
    ". ",            # POSIX source — must run in parent shell
    "alias ",        # alias — must run in parent shell
    "hermes ",       # Hermes CLI commands — avoid recursion
)

# Shell metacharacters that make wrapping unsafe (squeez wrap handles
# single commands; compound/piped commands should pass through raw)
_UNSAFE_CHARS = ("&&", "||", "|", ";", ">", "<", "$(", "`", "\n")


def register(ctx):
    """Register the Hermes pre-tool callback."""
    if not _check_squeez():
        return
    ctx.register_hook("pre_tool_call", _pre_tool_call)


def _check_squeez():
    """Return whether the squeez binary is in PATH, warning once when missing."""
    global _squeez_available, _squeez_missing_warned

    if _squeez_available is None:
        _squeez_available = shutil.which("squeez") is not None

    if not _squeez_available and not _squeez_missing_warned:
        _warn("squeez binary not found in PATH; Hermes hook not registered")
        _squeez_missing_warned = True

    return _squeez_available


def _pre_tool_call(tool_name=None, args=None, **_kwargs):
    """Wrap terminal commands with squeez when rtk hasn't already handled them.

    Flow:
      1. rtk-rewrite plugin fires first (alphabetical: rtk < squeez)
      2. If rtk knows the command → "rtk <cmd>" → we skip (starts with "rtk ")
      3. If rtk doesn't know it → original command → we wrap with "squeez wrap"
    """
    try:
        if tool_name != "terminal" or not isinstance(args, dict):
            return

        command = args.get("command")
        if not isinstance(command, str) or not command.strip():
            return

        command = command.strip()

        # Skip if already wrapped or shouldn't be wrapped
        if command.startswith(_NEVER_WRAP):
            return

        # Skip compound/piped commands (squeez wrap is for single commands)
        if any(char in command for char in _UNSAFE_CHARS):
            return

        # Wrap with squeez — squeez wrap executes the command, compresses
        # output through 4-stage pipeline, and returns the original exit code
        args["command"] = f"squeez wrap {command}"
    except Exception as e:
        _warn(str(e))
        return


def _warn(message):
    print(f"squeez: hermes plugin warning: {message}", file=sys.stderr)
