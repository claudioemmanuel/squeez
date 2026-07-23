//! Vendored pato-buddy companion (assets/buddy/).
//!
//! The duck's Node sources are embedded in the binary and materialized into
//! `<data_dir>/buddy/` at install, so every install method (curl, npm
//! pre-built binary, cargo) ships the buddy without an extra download.
//! XP grows with squeez net savings via `lib/squeez.js` (100 tokens = 1 XP).
//! Duck state lives in `~/.claude-buddy/` and is never touched here.

use std::path::{Path, PathBuf};

const LIB_STATE: &str = include_str!("../../assets/buddy/lib/state.js");
const LIB_ENGINE: &str = include_str!("../../assets/buddy/lib/engine.js");
const LIB_ART: &str = include_str!("../../assets/buddy/lib/art.js");
const LIB_SQUEEZ: &str = include_str!("../../assets/buddy/lib/squeez.js");
const HOOK_SESSION_START: &str = include_str!("../../assets/buddy/hooks/session-start.js");
const HOOK_POST_TOOL_USE: &str = include_str!("../../assets/buddy/hooks/post-tool-use.js");
const HOOK_STOP: &str = include_str!("../../assets/buddy/hooks/stop.js");
const STATUSLINE: &str = include_str!("../../assets/buddy/statusline/statusline.js");
const BUDDY_CLI: &str = include_str!("../../assets/buddy/buddy-cli.js");

/// Shell shim template: exits silently when `buddy = false` in config.ini or
/// `node` is missing, so a broken/disabled buddy can never break the host.
/// `{target}` is the node script path relative to the buddy root.
fn shim(target: &str) -> String {
    format!(
        "#!/bin/sh\n\
         # squeez buddy shim — silencioso quando buddy=false ou node ausente\n\
         root=\"$(cd \"$(dirname \"$0\")/..\" && pwd)\"\n\
         grep -Eq '^[[:space:]]*buddy[[:space:]]*=[[:space:]]*(false|off|0)[[:space:]]*$' \"$root/../config.ini\" 2>/dev/null && exit 0\n\
         command -v node >/dev/null 2>&1 || exit 0\n\
         exec node \"$root/{target}\"\n"
    )
}

/// Embedded buddy sources keyed by path relative to `<data_dir>/buddy/`.
pub fn buddy_manifest() -> [(&'static str, &'static str); 9] {
    [
        ("lib/state.js", LIB_STATE),
        ("lib/engine.js", LIB_ENGINE),
        ("lib/art.js", LIB_ART),
        ("lib/squeez.js", LIB_SQUEEZ),
        ("hooks/session-start.js", HOOK_SESSION_START),
        ("hooks/post-tool-use.js", HOOK_POST_TOOL_USE),
        ("hooks/stop.js", HOOK_STOP),
        ("statusline/statusline.js", STATUSLINE),
        ("buddy-cli.js", BUDDY_CLI),
    ]
}

const SHIMS: [(&str, &str); 3] = [
    ("shims/session-start.sh", "hooks/session-start.js"),
    ("shims/post-tool-use.sh", "hooks/post-tool-use.js"),
    ("shims/stop.sh", "hooks/stop.js"),
];

/// Statusline shim: chains the adopted third-party HUD (command stored in
/// `<buddy>/hud-command` at setup) with the duck head appended to the end of
/// the HUD's first line. With `buddy = false` (or no node) it degrades to the
/// HUD alone — never to a blank statusline.
const STATUSLINE_SHIM: &str = "#!/bin/sh\n\
# squeez buddy statusline — HUD original (se houver) + cabeça do pato na ponta\n\
root=\"$(cd \"$(dirname \"$0\")/..\" && pwd)\"\n\
input=$(cat)\n\
hud=\"\"\n\
if [ -f \"$root/hud-command\" ]; then\n\
  hud=$(printf '%s' \"$input\" | sh -c \"$(cat \"$root/hud-command\")\" 2>/dev/null)\n\
fi\n\
duck=\"\"\n\
if ! grep -Eq '^[[:space:]]*buddy[[:space:]]*=[[:space:]]*(false|off|0)[[:space:]]*$' \"$root/../config.ini\" 2>/dev/null; then\n\
  if command -v node >/dev/null 2>&1; then\n\
    duck=$(printf '%s' \"$input\" | node \"$root/statusline/statusline.js\" --compact 2>/dev/null)\n\
  fi\n\
fi\n\
if [ -n \"$hud\" ]; then\n\
  first=$(printf '%s\\n' \"$hud\" | head -n1)\n\
  rest=$(printf '%s\\n' \"$hud\" | tail -n +2)\n\
  if [ -n \"$duck\" ]; then printf '%s %s\\n' \"$first\" \"$duck\"; else printf '%s\\n' \"$first\"; fi\n\
  [ -n \"$rest\" ] && printf '%s\\n' \"$rest\"\n\
else\n\
  [ -n \"$duck\" ] && printf '%s\\n' \"$duck\"\n\
fi\n\
exit 0\n";

/// Writes the buddy tree into `<data_dir>/buddy/`, overwriting stale copies
/// (VERSION stamp records the writing binary). Returns the buddy root.
pub fn materialize(data_dir: &Path) -> std::io::Result<PathBuf> {
    let root = data_dir.join("buddy");
    for (rel, body) in buddy_manifest() {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, body)?;
    }
    for (rel, body) in SHIMS
        .iter()
        .map(|(rel, target)| (*rel, shim(target)))
        .chain(std::iter::once(("shims/statusline.sh", STATUSLINE_SHIM.to_string())))
    {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, body)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
    }
    std::fs::write(root.join("VERSION"), env!("CARGO_PKG_VERSION"))?;
    Ok(root)
}
