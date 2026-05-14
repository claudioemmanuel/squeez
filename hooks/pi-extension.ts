// squeez extension for Pi coding agent (https://pi.dev)
// Capabilities: BASH_WRAP + SESSION_MEM + BUDGET_HARD
//
// Events wired:
//   session_start          → squeez init --host=pi
//   tool_call              → mutates bash command → squeez wrap <cmd>
//   tool_result            → pipes text through squeez filter, returns compressed
//   session_before_compact → squeez track PreCompact <size>
import { spawnSync } from "child_process";
import { accessSync, constants } from "fs";
import { homedir } from "os";
import { join } from "path";

const SQUEEZ_DIR = join(homedir(), ".pi", "agent", "squeez");

function squeezBin(): string {
  const home = homedir();
  for (const candidate of [
    join(home, ".claude", "squeez", "bin", "squeez"),
    join(home, ".pi", "agent", "squeez", "bin", "squeez"),
  ]) {
    try {
      accessSync(candidate, constants.X_OK);
      return candidate;
    } catch {}
  }
  return "squeez";
}

function shellQuote(s: string): string {
  return "'" + s.replace(/'/g, "'\\''") + "'";
}

export default function (pi: any) {
  pi.on("session_start", async (_event: any, _ctx: any) => {
    const sq = squeezBin();
    spawnSync(sq, ["init", "--host=pi"], {
      env: { ...process.env, SQUEEZ_DIR },
      timeout: 5000,
    });
  });

  pi.on("tool_call", async (event: any, _ctx: any) => {
    const tool: string = event.toolName ?? "";
    if (!["bash", "Bash", "run_shell_command", "shell"].includes(tool)) return;
    const cmd: unknown = event.input?.command;
    if (!cmd || typeof cmd !== "string") return;
    const sq = squeezBin();
    if (cmd.includes("squeez wrap") || cmd.startsWith("--no-squeez")) return;
    event.input.command = `${sq} wrap ${shellQuote(cmd)}`;
  });

  pi.on("tool_result", async (event: any, _ctx: any) => {
    const content: unknown = event.content;
    if (!Array.isArray(content) || content.length === 0) return;
    const text = (content as any[])
      .filter((b) => b?.type === "text")
      .map((b) => (b.text as string) ?? "")
      .join("\n");
    if (!text.trim()) return;
    const sq = squeezBin();
    const tool: string = event.toolName ?? "unknown";
    const result = spawnSync(sq, ["filter", tool], {
      input: text,
      encoding: "utf8",
      env: { ...process.env, SQUEEZ_DIR },
      timeout: 3000,
    });
    if (result.status === 0 && result.stdout) {
      return { content: [{ type: "text", text: result.stdout as string }] };
    }
  });

  pi.on("session_before_compact", async (_event: any, _ctx: any) => {
    const sq = squeezBin();
    spawnSync(sq, ["track", "PreCompact", "0"], {
      env: { ...process.env, SQUEEZ_DIR },
      timeout: 3000,
    });
  });
}
