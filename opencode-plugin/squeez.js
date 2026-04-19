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
//   - tool.execute.before (bash) → rewrite command to `squeez wrap <cmd>`.
//   - tool.execute.before (read/grep) → inject budget limits so Read and
//     Grep respect the squeez config.
//   - tool.execute.after (any known tool) → fire-and-forget
//     `squeez track-result` for post-execution context tracking.
//
// Caveat (upstream sst/opencode#2319): MCP tool calls do NOT trigger these
// hooks. That's a host limitation, not something this plugin can work around.

import { execSync, spawn } from "child_process";

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
    execSync(`test -x "${SQUEEZ_BIN}"`, { timeout: 500 });
    return true;
  } catch {
    return false;
  }
}

function runInit() {
  try {
    execSync(`"${SQUEEZ_BIN}" init --host=opencode`, { timeout: 5000 });
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
          output.args.command = `${SQUEEZ_BIN} wrap ${command}`;
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
