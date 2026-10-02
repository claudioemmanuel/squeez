// squeez OpenCode plugin — full-parity integration, dual-host export.
//
// v1 (OpenCode 1.x): `server(input, options)` returns a map of hook-name →
//   handler. The return value MUST be an object — a bare return (or
//   `return undefined`) crashes OpenCode on internal property access (see
//   squeez issue #69, opencode 1.4.11 + @opencode-ai/plugin 1.4.10).
//
// v2 (OpenCode 2.x, verified against sst/opencode v2.0.18 source,
//   packages/plugin/src/promise/adapter.ts): `setup(ctx)` registers hooks on
//   domains. Contracts that matter here:
//   - ctx.shell.hook("create.before", cb) — cb receives a MUTABLE
//     { command, cwd, timeout, shell, env } (packages/plugin/src/effect/shell.ts);
//     core reads invocation.command after trigger (packages/core/src/shell.ts),
//     so in-place mutation is the contract.
//   - ctx.tool.hook("execute.before"|"execute.after", cb) — mutable `input`
//     field (packages/plugin/src/effect/tool.ts). v2 renamed the bash tool to
//     "shell"; we map it back to "bash" for squeez track-result.
//   - ctx.event.subscribe() returns an AsyncIterable of host events
//     (session.created lives there).
//   Registration shape differences across v2 point releases are tolerated via
//   optional chaining: a missing domain simply disables that hook.
//
// Handlers:
//   - session.created → finalize previous session and refresh AGENTS.md via
//     `squeez init --host=opencode`.
//   - bash/shell before-exec → rewrite command to `squeez wrap <cmd>`.
//   - read/grep before-exec → inject budget limits so Read and Grep respect
//     the squeez config.
//   - after-exec (any known tool) → fire-and-forget `squeez track-result`.
//
// MCP tools: sst/opencode#2319 ("MCP tool calls don't trigger plugin hooks")
// was fixed by sst/opencode#2320 (merged 2025-08-30), and v2 routes MCP tools
// through the same Tool.execute that fires execute.before/after
// (packages/core/src/tool.ts). So these hooks DO see MCP calls on current
// hosts;
// squeez ignores them anyway (budget slugs cover read/grep only).


import { execSync, spawn } from "child_process";

// Every child below passes `windowsHide: true`. On Windows, execSync goes
// through cmd.exe and a detached spawn gets its own console, so without it
// each tool call flashed a console window (squeez issue #231).

const HOME = process.env.HOME || process.env.USERPROFILE || "";
const SQUEEZ_BIN = `${HOME}/.claude/squeez/bin/squeez`;

// Map OpenCode's lowercase tool names to the capitalized slugs the squeez
// budget-params subcommand expects (Read / Grep).
const BUDGET_TOOL_SLUG = {
  read: "Read",
  grep: "Grep",
};

function squeezExists() {
  try {
    execSync(`test -x "${SQUEEZ_BIN}"`, { timeout: 500, windowsHide: true });
    return true;
  } catch {
    return false;
  }
}

function runInit() {
  try {
    execSync(`"${SQUEEZ_BIN}" init --host=opencode`, {
      timeout: 5000,
      windowsHide: true,
    });
  } catch {
    // best-effort — don't break the session if squeez init fails
  }
}

function budgetPatch(tool) {
  const slug = BUDGET_TOOL_SLUG[tool];
  if (!slug) return null;
  try {
    const out = execSync(`"${SQUEEZ_BIN}" budget-params ${slug}`, {
      timeout: 2000,
      encoding: "utf8",
      windowsHide: true,
    }).trim();
    if (!out) return null;
    return JSON.parse(out);
  } catch {
    return null;
  }
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

  // OpenCode 2.x entry point. v1 ignores this key (its PluginModule type is
  // { id?, server, tui? }); v2 prefers setup() when present.
  setup(ctx) {
    if (!ctx) return;
    // Returning nothing (not even a cleanup) keeps the v2 loader happy when
    // squeez isn't on the machine. Hooks are simply absent.
    if (!squeezExists()) return;

    // session.created → squeez init (event bus is an AsyncIterable).
    if (ctx.event && typeof ctx.event.subscribe === "function") {
      (async () => {
        try {
          for await (const event of ctx.event.subscribe()) {
            if (event && event.type === "session.created") runInit();
          }
        } catch {
          // event stream closed or host shutting down — harmless
        }
      })();
    }

    // bash (v2: shell) → wrap command. Same guards as the v1 handler.
    if (ctx.shell && typeof ctx.shell.hook === "function") {
      ctx.shell.hook("create.before", (e) => {
        if (!e || typeof e.command !== "string") return;
        const command = e.command;
        if (!command) return;
        if (command.startsWith(SQUEEZ_BIN)) return;
        if (command.includes("squeez wrap")) return;
        if (command.startsWith("--no-squeez")) return;
        // See the v1 handler for why the command is shell-quoted first.
        const quoted = "'" + command.replace(/'/g, "'\\''") + "'";
        e.command = `${SQUEEZ_BIN} wrap ${quoted}`;
      });
    }

    if (ctx.tool && typeof ctx.tool.hook === "function") {
      // read/grep budget injection — v2's mutable field is `input`.
      ctx.tool.hook("execute.before", (e) => {
        if (!e || typeof e.tool !== "string") return;
        const patch = budgetPatch(e.tool);
        if (!patch) return;
        const input = e.input;
        if (!input || typeof input !== "object") return;
        for (const [k, v] of Object.entries(patch)) {
          // Do not override fields the user (or agent) already set explicitly.
          if (input[k] === undefined) {
            input[k] = v;
          }
        }
      });

      // Post-execution tracking — v2 renamed bash → shell; map it back.
      ctx.tool.hook("execute.after", (e) => {
        if (!e || !e.tool) return;
        const tool = e.tool === "shell" ? "bash" : e.tool;
        if (["bash", "read", "grep", "glob"].includes(tool)) {
          trackResult(tool);
        }
      });
    }
  },

  // OpenCode 1.x entry point — byte-identical behavior to pre-v2 squeez.
server: async (_input, _options) => {
    // Returning `{}` (not `undefined`) keeps the plugin loader happy when
    // squeez isn't on the machine. Hooks are simply absent so OpenCode runs
    // as if the plugin were not installed.
    if (!squeezExists()) return {};

    return {
      event: async ({ event }) => {
        if (event && event.type === "session.created") {
          runInit();
        }
      },

      "tool.execute.before": async (input, output) => {
        if (!input || !output || !output.args) return;

        if (input.tool === "bash") {
          const command = output.args.command;
          if (!command || typeof command !== "string") return;
          if (command.startsWith(SQUEEZ_BIN)) return;
          if (command.includes("squeez wrap")) return;
          if (command.startsWith("--no-squeez")) return;
          // Shell-quote the command before prepending `squeez wrap`. Without
          // this, multi-line `python3 -c "..."`, `bash -c '...'`, and quoted
          // `git commit -m "msg with spaces"` are split into separate argv
          // tokens by the host shell and end up with `-c` getting no argument,
          // pathspec errors on commit messages, etc. Matches what the
          // claude-code Python hook does with `shlex.quote(cmd)`.
          const quoted = "'" + command.replace(/'/g, "'\\''") + "'";
          output.args.command = `${SQUEEZ_BIN} wrap ${quoted}`;
          return;
        }

        const patch = budgetPatch(input.tool);
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
