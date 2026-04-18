// squeez OpenCode plugin — full-parity integration.
//
// Capabilities:
//   - session.created → finalize previous session and refresh AGENTS.md via
//     `squeez init --host=opencode`.
//   - tool.execute.before (bash) → rewrite command to `squeez wrap <cmd>`.
//   - tool.execute.before (read/grep) → inject budget limits so Read and
//     Grep respect the squeez config.
//   - tool.execute.after (any) → fire-and-forget `squeez track-result` for
//     post-execution context tracking.
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

export const SqueezPlugin = async (ctx) => {
  if (!squeezExists()) {
    // squeez not installed locally — plugin becomes a no-op rather than
    // throwing on every hook.
    return;
  }

  ctx.hook?.("session.created", async () => {
    runInit();
  });

  ctx.hook?.("tool.execute.before", async (input, output) => {
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
  });

  ctx.hook?.("tool.execute.after", async (input) => {
    if (!input || !input.tool) return;
    // Only track tools we know about — keeps the noise down.
    if (["bash", "read", "grep", "glob"].includes(input.tool)) {
      trackResult(input.tool);
    }
  });
};
