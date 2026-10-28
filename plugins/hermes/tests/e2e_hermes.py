"""Run from a hermes-agent checkout: .venv/bin/python <this file>."""
import importlib.util
import json
import os
import re
import shlex
import sys
from pathlib import Path

sys.path.insert(0, os.getcwd())
from hermes_cli.plugins import _dispatch_pre_tool_call_hooks, discover_plugins, get_plugin_manager
from tools.terminal_tool import terminal_tool

PLUGIN = Path(__file__).resolve().parent.parent / "__init__.py"
spec = importlib.util.spec_from_file_location("squeez_output", PLUGIN)
assert spec and spec.loader
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

BIG = f"cd {shlex.quote(os.getcwd())} && git log --stat -30 | head -3000"
FAKE = "ghp_" + "A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8"
SECRET = f"for i in $(seq 1 300); do echo line $i GITHUB_TOKEN={FAKE}; done"


def run(cmd):
    r = terminal_tool(command=cmd, timeout=60)
    r = json.loads(r) if isinstance(r, str) else r
    return r.get("output", "")


fails = []


def check(name, cond, detail=""):
    print(("PASS " if cond else "FAIL ") + name, detail)
    if not cond:
        fails.append(name)


discover_plugins()
hooks = get_plugin_manager()._hooks.setdefault("transform_terminal_output", [])
hooks.clear()

raw = run(BIG)
check("control: no plugin -> raw output", "[squeez:" not in raw, f"len={len(raw)}")

hooks.append(mod._transform_terminal_output)
out = run(BIG)
m = re.search(r"Full output: (\S+)", out)
check("plugin: output shrinks", len(out) < len(raw) * 0.8, f"{len(raw)} -> {len(out)}")
check("plugin: footer with path", bool(m))
if m:
    p = Path(m.group(1))
    check("plugin: saved file exists, mode 600",
          p.exists() and (p.stat().st_mode & 0o777) == 0o600)
    check("plugin: saved file holds full output", len(p.read_text()) >= len(raw) * 0.9,
          f"saved={len(p.read_text())} raw={len(raw)}")

small = run("echo hello")
check("small: unchanged", small.strip() == "hello", repr(small[:40]))

os.environ["SQUEEZ_OUTPUT_DISABLE"] = "1"
d = run(BIG)
check("disable: unchanged", "[squeez:" not in d and len(d) >= len(raw) * 0.9, f"len={len(d)}")
del os.environ["SQUEEZ_OUTPUT_DISABLE"]

real_bin, mod._squeez_bin = mod._squeez_bin, lambda: None
nb = run(BIG)
check("no binary: unchanged", "[squeez:" not in nb and len(nb) >= len(raw) * 0.9, f"len={len(nb)}")
mod._squeez_bin = real_bin

os.environ["SQUEEZ_HERMES_NEVER_WRAP"] = "cd "
nw = run(BIG)
check("declared prefix: unchanged", "[squeez:" not in nw and len(nw) >= len(raw) * 0.9, f"len={len(nw)}")
del os.environ["SQUEEZ_HERMES_NEVER_WRAP"]

pre = get_plugin_manager()._hooks.setdefault("pre_tool_call", [])
pre[:] = [mod._pre_tool_call]
_, new = _dispatch_pre_tool_call_hooks("terminal", {"command": "--no-squeez " + BIG})
bp = run((new or {}).get("command", ""))
check("bypass: --no-squeez runs and is unchanged",
      "[squeez:" not in bp and len(bp) >= len(raw) * 0.9, f"len={len(bp)}")
pre.clear()

s = run(SECRET)
m = re.search(r"Full output: (\S+)", s)
if m:
    body = Path(m.group(1)).read_text()
    check("secret: saved file redacted", FAKE not in body, f"lines={body.count(chr(10))}")
else:
    print("INFO secret case not compressed (squeez saved < 20%); saved-file check skipped")
check("secret: model-visible output redacted", FAKE not in s)

print("RESULT", "FAIL " + ",".join(fails) if fails else "ALL PASS")
sys.exit(1 if fails else 0)
