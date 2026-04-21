#!/usr/bin/env bash
set -euo pipefail
REPO="$(cd "$(dirname "$0")" && pwd)"

# Install rustup if needed
if ! command -v cargo &>/dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

cd "$REPO"
cargo build --release
BINARY="$REPO/target/release/squeez"

INSTALL_DIR="$HOME/.claude/squeez"
mkdir -p "$INSTALL_DIR/bin" "$INSTALL_DIR/hooks" "$INSTALL_DIR/sessions" "$INSTALL_DIR/memory"
cp "$BINARY" "$INSTALL_DIR/bin/squeez" && chmod +x "$INSTALL_DIR/bin/squeez"
cp "$REPO/hooks/"*.sh "$INSTALL_DIR/hooks/" && chmod +x "$INSTALL_DIR/hooks/"*.sh
cp "$REPO/hooks/statusline.sh" "$INSTALL_DIR/bin/statusline.sh" && chmod +x "$INSTALL_DIR/bin/statusline.sh"

# Commit binary to repo
mkdir -p "$REPO/bin"
cp "$BINARY" "$REPO/bin/squeez"

# Register hooks + statusline
python3 - <<'EOF'
import json, os, shutil, sys

path = os.path.expanduser("~/.claude/settings.json")
settings = {}
file_existed = os.path.exists(path)
if file_existed:
    try:
        with open(path, "r", encoding="utf-8-sig") as f:
            settings = json.load(f)
    except Exception as e:
        sys.stderr.write(
            "squeez: refusing to overwrite " + path + ": could not parse existing JSON (" + str(e) + ").\n"
            "squeez: fix or remove the file, then re-run the installer.\n"
        )
        sys.exit(2)
if not isinstance(settings, dict):
    sys.stderr.write("squeez: refusing to overwrite " + path + ": top-level value is not a JSON object.\n")
    sys.exit(2)

def ensure_list(key):
    if not isinstance(settings.get(key), list):
        settings[key] = []

ensure_list("PreToolUse")
pre = {"matcher": "Bash", "hooks": [{"type": "command", "command": "bash ~/.claude/squeez/hooks/pretooluse.sh"}]}
if not any("squeez" in str(h) for h in settings["PreToolUse"]):
    settings["PreToolUse"].append(pre)

ensure_list("SessionStart")
start = {"hooks": [{"type": "command", "command": "bash ~/.claude/squeez/hooks/session-start.sh"}]}
if not any("squeez" in str(h) for h in settings["SessionStart"]):
    settings["SessionStart"].append(start)

ensure_list("PostToolUse")
post = {"hooks": [{"type": "command", "command": "bash ~/.claude/squeez/hooks/posttooluse.sh"}]}
if not any("squeez" in str(h) for h in settings["PostToolUse"]):
    settings["PostToolUse"].append(post)

existing_status = settings.get("statusLine", {})
existing_cmd = existing_status.get("command", "") if isinstance(existing_status, dict) else ""
squeez_cmd = "bash ~/.claude/squeez/bin/statusline.sh"
if "squeez" not in existing_cmd:
    if existing_cmd:
        new_cmd = "bash -c 'input=$(cat); echo \"$input\" | { " + existing_cmd.rstrip() + "; } 2>/dev/null; echo \"$input\" | " + squeez_cmd + "'"
        settings["statusLine"] = {"type": "command", "command": new_cmd}
    else:
        settings["statusLine"] = {"type": "command", "command": squeez_cmd}

os.makedirs(os.path.dirname(path), exist_ok=True)
if file_existed:
    try:
        shutil.copy2(path, path + ".bak")
    except Exception:
        pass
tmp = path + ".tmp"
with open(tmp, "w", encoding="utf-8") as f:
    json.dump(settings, f, indent=2)
os.replace(tmp, path)
print("hooks registered in ~/.claude/settings.json")
EOF

# Auto-calibrate: run benchmark analysis to generate optimized config
echo "  Running calibration..."
"$INSTALL_DIR/bin/squeez" calibrate --force-aggressive 2>/dev/null || true

echo "✅ squeez $($INSTALL_DIR/bin/squeez --version) installed ($(du -sh $INSTALL_DIR/bin/squeez | cut -f1))"
echo "   Restart Claude Code to activate."
