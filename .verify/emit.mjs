// Emits the command lines the OpenCode plugin builds on this machine, one per
// shell family, for the workflow to execute for real.
import { mkdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { pathToFileURL } from "node:url";

process.env.HOME = homedir();
delete process.env.SHELL;
const plugin = (await import(pathToFileURL("opencode-plugin/squeez.js").href)).default;
mkdirSync("out", { recursive: true });

async function emit(file, shell, command) {
  const hooks = await plugin.server({}, {});
  if (!hooks["tool.execute.before"]) throw new Error("plugin returned no hooks: squeez not found");
  await hooks.config(shell ? { shell } : {});
  const output = { args: { command } };
  await hooks["tool.execute.before"]({ tool: "bash" }, output);
  writeFileSync(file, output.args.command);
  console.log(`${file}: ${output.args.command}`);
}

await emit("out/ps.txt", undefined, `Write-Output "it's ok"; Write-Output ("year=" + (Get-Date -Format yyyy)); exit 7`);
await emit("out/bash.txt", "bash", `printf '%s\\n' "it's ok" "sum=$((40 + 2))"; exit 7`);

// Same wrapper, with the progress stream silenced inside the encoded command.
{
  const { readFileSync } = await import("node:fs");
  const base = readFileSync("out/ps.txt", "utf8");
  const inner = `$ProgressPreference = 'SilentlyContinue'; Write-Output "it's ok"; Write-Output ("year=" + (Get-Date -Format yyyy)); exit 7`;
  const encoded = Buffer.from(inner, "utf16le").toString("base64");
  writeFileSync("out/ps2.txt", base.replace(/-EncodedCommand \S+'$/, `-EncodedCommand ${encoded}'`));
}
