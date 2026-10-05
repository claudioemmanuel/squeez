// squeez OpenCode plugin — full-parity integration.
//
// Conforms to the @opencode-ai/plugin SDK `PluginModule` contract: a default
// export object with `id` + async `server(input, options)` that returns a map
// of hook-name → handler. The server return value MUST be an object — a bare
// return (or `return undefined`) causes OpenCode to crash on internal
// property access (see squeez issue #69, reproduced on opencode 1.4.11 +
// @opencode-ai/plugin 1.4.10).
//
// Handlers:
//   - event (session.created) → finalize previous session and refresh
//     AGENTS.md via `squeez init --host=opencode`.
//   - config → remember which shell OpenCode runs commands with.
//   - tool.execute.before (bash) → rewrite command to `squeez wrap <cmd>`,
//     in the syntax of that shell.
//   - tool.execute.before (read/grep) → inject budget limits so Read and
//     Grep respect the squeez config.
//   - tool.execute.after (any known tool) → fire-and-forget
//     `squeez track-result` for post-execution context tracking.
//
// Caveat (upstream sst/opencode#2319): MCP tool calls do NOT trigger these
// hooks. That's a host limitation, not something this plugin can work around.

import { execFile, spawn } from "child_process";
import { accessSync, constants } from "fs";

// Every child below passes `windowsHide: true`. On Windows a child without
// it gets its own console, so each tool call flashed a console window
// (squeez issue #231).
//
// No synchronous child processes (squeez issue #245). Inside the OpenCode
// server on Windows the first execSync of a hook invocation fails at once
// with a false ETIMEDOUT (8-10 ms into a 2000 ms timeout; the same call
// succeeds when repeated). The plugin made one execSync per read/grep, so
// the budget was never applied, and the same failure in the load-time
// check would have dropped every hook. So the check is a file test, and
// squeez runs through async execFile, without a shell, with one retry.

const HOME = process.env.HOME || process.env.USERPROFILE || "";
const SQUEEZ_BIN = `${HOME}/.claude/squeez/bin/squeez`;

// Map OpenCode's lowercase tool names to the capitalized slugs the squeez
// budget-params subcommand expects (Read / Grep).
const BUDGET_TOOL_SLUG = {
  read: "Read",
  grep: "Grep",
};

// What the rewritten command line calls. On Windows it is written with
// forward slashes, which both PowerShell and Git Bash accept, and it names the
// `.exe` so neither shell has to guess the extension.
const IS_WINDOWS = process.platform === "win32";
const SQUEEZ_CMD = IS_WINDOWS
  ? `${SQUEEZ_BIN.replace(/\\/g, "/")}.exe`
  : SQUEEZ_BIN;

const POSIX_SHELLS = ["bash", "sh", "zsh", "dash", "ksh"];

// The shell OpenCode hands the command to, as a bare lowercase name: its
// `shell` config key when set, else $SHELL. Empty when neither names one.
function shellName(configShell) {
  const shell = (typeof configShell === "string" && configShell) || process.env.SHELL || "";
  return shell.split(/[\\/]/).pop().toLowerCase().replace(/\.exe$/, "");
}

function isAlreadyWrapped(command) {
  return (
    command.startsWith(SQUEEZ_BIN) ||
    command.includes("squeez wrap") ||
    /squeez(\.exe)?['"]?\s+wrap\s/.test(command)
  );
}

// The command line that runs `command` through `squeez wrap`, written for the
// shell that will parse it (squeez issue #244). The wrapper has to follow the
// OpenCode shell, not the OS: on Windows that shell is PowerShell unless the
// user configured bash, and each one misreads the other's quoting.
//
// POSIX shells: single-quote the command. Without that, multi-line
// `python3 -c "..."`, `bash -c '...'`, and quoted
// `git commit -m "msg with spaces"` are split into separate argv tokens by
// the host shell and end up with `-c` getting no argument, pathspec errors on
// commit messages, etc. Matches what the claude-code Python hook does with
// `shlex.quote(cmd)`.
//
// PowerShell: `squeez wrap` re-runs its argument with bash when Git Bash is
// installed, so a PowerShell command passed as text would be run by the wrong
// shell. It goes through `-EncodedCommand` (base64 of UTF-16LE) instead,
// which no quoting layer on the way can reinterpret.
function wrapCommand(command, configShell) {
  const name = shellName(configShell);
  const posixQuote = (text) => "'" + text.replace(/'/g, "'\\''") + "'";
  const quoted = "'" + command.replace(/'/g, "'\\''") + "'";
  if (!IS_WINDOWS) return `${SQUEEZ_BIN} wrap ${quoted}`;
  if (POSIX_SHELLS.includes(name)) return `${posixQuote(SQUEEZ_CMD)} wrap ${quoted}`;
  const exe = name === "pwsh" ? "pwsh.exe" : "powershell.exe";
  const encoded = Buffer.from(command, "utf16le").toString("base64");
  const bin = SQUEEZ_CMD.replace(/'/g, "''");
  return `& '${bin}' wrap '${exe} -NoLogo -NoProfile -NonInteractive -EncodedCommand ${encoded}'`;
}

function isExecutable(path) {
  try {
    accessSync(path, constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

function squeezExists() {
  return (
    isExecutable(SQUEEZ_BIN) ||
    (process.platform === "win32" && isExecutable(`${SQUEEZ_BIN}.exe`))
  );
}

function runSqueez(args, timeoutMs) {
  return new Promise((resolve, reject) => {
    execFile(
      SQUEEZ_BIN,
      args,
      { timeout: timeoutMs, encoding: "utf8", windowsHide: true },
      (error, stdout) => (error ? reject(error) : resolve(String(stdout))),
    );
  });
}

function runInit() {
  // best-effort and not awaited — don't break the session if squeez init fails
  runSqueez(["init", "--host=opencode"], 5000).catch(() => {});
}

// The budget comes from squeez's config, which rarely changes: ask once per
// tool and keep the answer for a minute. A failed lookup is not kept.
const BUDGET_TTL_MS = 60000;
const budgetCache = new Map();

async function budgetPatch(tool) {
  const slug = BUDGET_TOOL_SLUG[tool];
  if (!slug) return null;
  const hit = budgetCache.get(slug);
  if (hit && Date.now() - hit.at < BUDGET_TTL_MS) return hit.patch;
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      const out = (await runSqueez(["budget-params", slug], 2000)).trim();
      const patch = out ? JSON.parse(out) : null;
      budgetCache.set(slug, { at: Date.now(), patch });
      return patch;
    } catch {
      // try once more, then leave the call untouched
    }
  }
  return null;
}

function trackResult(tool) {
  // Fire-and-forget — don't block the tool pipeline.
  try {
    spawn(SQUEEZ_BIN, ["track-result", tool], {
      stdio: "ignore",
      detached: true,
      windowsHide: true,
    }).unref();
  } catch {
    // best-effort
  }
}

export default {
  id: "squeez",
  server: async (_input, _options) => {
    // Returning `{}` (not `undefined`) keeps the plugin loader happy when
    // squeez isn't on the machine. Hooks are simply absent so OpenCode runs
    // as if the plugin were not installed.
    if (!squeezExists()) return {};

    let configShell;

    return {
      // OpenCode hands every plugin the resolved config once at load.
      config: async (cfg) => {
        configShell = cfg && cfg.shell;
      },

      event: async ({ event }) => {
        if (event && event.type === "session.created") {
          runInit();
        }
      },

      "tool.execute.before": async (input, output) => {
        if (!input || !output || !output.args) return;

        // Some OpenCode builds name the shell tool `shell`.
        if (input.tool === "bash" || input.tool === "shell") {
          const command = output.args.command;
          if (!command || typeof command !== "string") return;
          if (isAlreadyWrapped(command)) return;
          if (command.startsWith("--no-squeez")) return;
          output.args.command = wrapCommand(command, configShell);
          return;
        }

        const patch = await budgetPatch(input.tool);
        if (!patch) return;
        for (const [k, v] of Object.entries(patch)) {
          // Do not override fields the user (or agent) already set explicitly.
          if (output.args[k] === undefined) {
            output.args[k] = v;
          }
        }
      },

      "tool.execute.after": async (input) => {
        if (!input || !input.tool) return;
        // Only track tools we know about — keeps the noise down.
        if (["bash", "read", "grep", "glob"].includes(input.tool)) {
          trackResult(input.tool);
        }
      },
    };
  },
};
