// squeez OpenCode plugin — full-parity integration, one file for both hosts.
//
// OpenCode 1.x: conforms to the @opencode-ai/plugin SDK `PluginModule`
// contract: a default export object with `id` + async `server(input, options)`
// that returns a map of hook-name → handler. The server return value MUST be
// an object — a bare return (or `return undefined`) causes OpenCode to crash
// on internal property access (see squeez issue #69, reproduced on opencode
// 1.4.11 + @opencode-ai/plugin 1.4.10).
//
// OpenCode 2.x: `setup(ctx)` registers the same work on typed domains (read
// from the 2.0.22 source, packages/plugin/src/promise/adapter.ts). Each host
// ignores the other's entry point. What differs on 2.x:
//   - the shell is its own domain: `ctx.shell.hook("create.before")` rewrites
//     the command, and a finished command is only announced on the event bus
//     as `shell.exited`. It never reaches the tool hooks.
//   - `ctx.tool.hook("execute.before")` exposes the arguments as `input`.
//   - sessions announce themselves with `session.execution.started`, once per
//     turn, so init is deduplicated per session.
//
// Handlers (1.x names):
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
// which no quoting layer on the way can reinterpret. The progress stream is
// silenced first: with its output captured, PowerShell serialises the
// "Preparing modules for first use" record as a `#< CLIXML` block, about 400
// characters in front of every command's real output.
function wrapCommand(command, configShell) {
  const name = shellName(configShell);
  const posixQuote = (text) => "'" + text.replace(/'/g, "'\\''") + "'";
  const quoted = "'" + command.replace(/'/g, "'\\''") + "'";
  if (!IS_WINDOWS) return `${SQUEEZ_BIN} wrap ${quoted}`;
  if (POSIX_SHELLS.includes(name)) return `${posixQuote(SQUEEZ_CMD)} wrap ${quoted}`;
  const exe = name === "pwsh" ? "pwsh.exe" : "powershell.exe";
  const script = `$ProgressPreference = 'SilentlyContinue'; ${command}`;
  const encoded = Buffer.from(script, "utf16le").toString("base64");
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

// Fire-and-forget — don't block the tool pipeline.
//
// `squeez track-result` reads stdin and returns at once when it is empty, so
// a call without `payload` records nothing (squeez issue #247). With one, the
// JSON is piped and the stream closed. The tool comes from the argument; of
// the payload the observer reads only what it knows (`file_path`, `pattern`,
// `path`, content), so shell metadata does no more than make it count the
// call and stamp the activity time.
function trackResult(tool, payload) {
  try {
    const json = payload === undefined ? undefined : JSON.stringify(payload);
    const child = spawn(SQUEEZ_BIN, ["track-result", tool], {
      stdio: json === undefined ? "ignore" : ["pipe", "ignore", "ignore"],
      detached: true,
      windowsHide: true,
    });
    // A missing binary or a closed pipe must never reach the host.
    child.on("error", () => {});
    if (child.stdin) {
      child.stdin.on("error", () => {});
      child.stdin.end(json);
    }
    child.unref();
  } catch {
    // best-effort
  }
}

// What the observer keeps of a result is capped at 256 KiB, so no more than
// that is piped to it.
const MAX_TRACKED_CHARS = 256 * 1024;

// The payload `squeez track-result` reads, in the shape Claude Code's
// PostToolUse hook sends it. `args` is the tool input as either host names
// it: 1.x reads `filePath`, 2.x reads `path`; grep and glob take `pattern`
// and an optional `path` on both.
function toolPayload(tool, args, text) {
  const input = args && typeof args === "object" ? args : {};
  const str = (value) => (typeof value === "string" && value ? value : undefined);
  const isRead = tool === "read";
  return {
    tool_name: tool,
    tool_input: {
      file_path: isRead ? str(input.filePath) || str(input.path) : undefined,
      pattern: str(input.pattern),
      path: isRead ? undefined : str(input.path),
    },
    tool_result: { content: str(text) && text.slice(0, MAX_TRACKED_CHARS) },
  };
}

// The text of an OpenCode 2.x `execute.after` event: what the model was shown
// (`content`, a string or a list of blocks), or the error message.
function resultText(e) {
  if (e.status === "error") return e.error && e.error.message;
  const content = e.result && e.result.content;
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return undefined;
  return content
    .filter((block) => block && block.type === "text" && typeof block.text === "string")
    .map((block) => block.text)
    .join("\n");
}

const SHELL_END_STATUSES = ["exited", "timeout", "killed"];

// OpenCode 2.x event bus. Event names can carry a numeric suffix
// (`shell.exited.1`), hence the patterns.
async function watchEvents(events) {
  const initialized = new Set();
  try {
    for await (const event of events) {
      const type = (event && event.type) || "";
      const data = (event && event.data) || {};
      if (type === "session.created") {
        runInit();
      } else if (/^session\.execution\.started(\.\d+)?$/.test(type)) {
        // Fired once per turn: run init only the first time a session shows up.
        const sessionID = data.sessionID || "";
        if (initialized.has(sessionID)) continue;
        initialized.add(sessionID);
        runInit();
      } else if (/^shell\.exited(\.\d+)?$/.test(type)) {
        // The event carries { id, exit?, status } and nothing else.
        if (typeof data.id !== "string") continue;
        if (!SHELL_END_STATUSES.includes(data.status)) continue;
        trackResult("bash", {
          tool_name: "Bash",
          shell_id: data.id,
          shell_status: data.status,
          ...(typeof data.exit === "number" ? { exit_code: data.exit } : {}),
        });
      }
    }
  } catch {
    // event stream closed or host shutting down — harmless
  }
}

export default {
  id: "squeez",

  // OpenCode 2.x entry point. Every domain is optional: a release that lacks
  // one loses that hook instead of failing the plugin.
  setup(ctx) {
    if (!ctx || !squeezExists()) return;

    if (ctx.event && typeof ctx.event.subscribe === "function") {
      watchEvents(ctx.event.subscribe());
    }

    if (ctx.shell && typeof ctx.shell.hook === "function") {
      ctx.shell.hook("create.before", (e) => {
        if (!e || !e.command || typeof e.command !== "string") return;
        if (isAlreadyWrapped(e.command)) return;
        if (e.command.startsWith("--no-squeez")) return;
        // The event names the shell that will run the command.
        e.command = wrapCommand(e.command, e.shell);
      });
    }

    if (ctx.tool && typeof ctx.tool.hook === "function") {
      // The host awaits this callback before it reads `input` back.
      ctx.tool.hook("execute.before", async (e) => {
        if (!e || !e.input || typeof e.input !== "object") return;
        const patch = await budgetPatch(e.tool);
        if (!patch) return;
        for (const [k, v] of Object.entries(patch)) {
          // Do not override fields the user (or agent) already set explicitly.
          if (e.input[k] === undefined) {
            e.input[k] = v;
          }
        }
      });

      // No `bash` here: shell completions arrive as `shell.exited` above.
      ctx.tool.hook("execute.after", (e) => {
        if (!e || !e.tool) return;
        if (["read", "grep", "glob"].includes(e.tool)) {
          trackResult(e.tool, toolPayload(e.tool, e.input, resultText(e)));
        }
      });
    }
  },

  // OpenCode 1.x entry point.
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

      "tool.execute.after": async (input, output) => {
        if (!input || !input.tool) return;
        // Only track tools we know about — keeps the noise down.
        if (["read", "grep", "glob"].includes(input.tool)) {
          trackResult(input.tool, toolPayload(input.tool, input.args, output && output.output));
        } else if (input.tool === "bash" || input.tool === "shell") {
          // `squeez wrap` already read this output; the observer only has to
          // see that a command finished, as on 2.x.
          trackResult("bash", { tool_name: "Bash" });
        }
      },
    };
  },
};
