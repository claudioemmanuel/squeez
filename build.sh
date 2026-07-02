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
mkdir -p "$INSTALL_DIR/bin" "$INSTALL_DIR/sessions" "$INSTALL_DIR/memory"
# rm before cp: overwriting an existing Mach-O in place reuses its inode, so
# the macOS kernel's per-inode signature cache goes stale and the next exec
# is SIGKILLed ("Killed: 9"). A fresh inode avoids that; re-sign ad-hoc as
# belt-and-braces.
rm -f "$INSTALL_DIR/bin/squeez"
cp "$BINARY" "$INSTALL_DIR/bin/squeez" && chmod +x "$INSTALL_DIR/bin/squeez"
if command -v codesign &>/dev/null; then
    codesign -s - -f "$INSTALL_DIR/bin/squeez" 2>/dev/null || true
fi

# Commit binary to repo
mkdir -p "$REPO/bin"
rm -f "$REPO/bin/squeez"
cp "$BINARY" "$REPO/bin/squeez"

# Delegate hook installation + settings.json patching to the binary itself.
# This routes through the same host adapter (`src/hosts/claude_code.rs`) that
# `squeez setup` uses, so the inline build.sh registration cannot drift from
# the Rust source of truth. The adapter writes hooks under
# `settings.hooks.<event>` (the schema Claude Code v2.x reads from) — earlier
# versions of this script wrote to top-level `settings.<event>`, which silently
# disabled compression because the harness never read those entries.
"$INSTALL_DIR/bin/squeez" setup

# Auto-calibrate: run benchmark analysis to generate optimized config
echo "  Running calibration..."
"$INSTALL_DIR/bin/squeez" calibrate --force-aggressive 2>/dev/null || true

echo "✅ squeez $($INSTALL_DIR/bin/squeez --version) installed ($(du -sh $INSTALL_DIR/bin/squeez | cut -f1))"
echo "   Restart Claude Code to activate."
