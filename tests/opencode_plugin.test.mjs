// Behavioural tests for opencode-plugin/squeez.js, run by `node --test`
// (driven from tests/test_hosts_opencode.rs). The plugin is loaded the way
// OpenCode 1.x loads it: default export, `server()`, then the hooks.

import assert from "node:assert/strict";
import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

const PLUGIN = join(fileURLToPath(new URL(".", import.meta.url)), "..", "opencode-plugin", "squeez.js");
const REAL_PLATFORM = process.platform;
let loads = 0;

// The plugin reads the platform, HOME and SHELL once at import, so each case
// imports its own copy.
async function loadHooks({ platform, home, shell }) {
  Object.defineProperty(process, "platform", { value: platform });
  process.env.HOME = home;
  delete process.env.USERPROFILE;
  if (shell === undefined) delete process.env.SHELL;
  else process.env.SHELL = shell;
  try {
    const mod = await import(`${pathToFileURL(PLUGIN).href}?load=${++loads}`);
    return await mod.default.server({}, {});
  } finally {
    Object.defineProperty(process, "platform", { value: REAL_PLATFORM });
  }
}

function fakeHome(sub = "") {
  const home = join(mkdtempSync(join(tmpdir(), "squeez-plugin-")), sub);
  const bin = join(home, ".claude", "squeez", "bin");
  mkdirSync(bin, { recursive: true });
  for (const name of ["squeez", "squeez.exe"]) {
    writeFileSync(join(bin, name), "");
    chmodSync(join(bin, name), 0o755);
  }
  return home;
}

async function wrap(hooks, command, tool = "bash") {
  const output = { args: { command } };
  await hooks["tool.execute.before"]({ tool }, output);
  return output.args.command;
}

const slashed = (p) => p.replace(/\\/g, "/");
const TRICKY = `Write-Output "it's"; Get-ChildItem |\n  Select-Object -First 1`;

test("non-Windows keeps the POSIX wrapper byte for byte", async () => {
  const home = fakeHome();
  const hooks = await loadHooks({ platform: "linux", home, shell: "/bin/zsh" });
  assert.equal(
    await wrap(hooks, "echo 'a b'"),
    `${home}/.claude/squeez/bin/squeez wrap 'echo '\\''a b'\\'''`,
  );
});

test("Windows defaults to a PowerShell wrapper with an encoded command", async () => {
  const home = fakeHome();
  const hooks = await loadHooks({ platform: "win32", home });
  const wrapped = await wrap(hooks, TRICKY);
  const m = wrapped.match(
    /^& '(.+)' wrap 'powershell\.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand ([A-Za-z0-9+/=]+)'$/,
  );
  assert.ok(m, wrapped);
  assert.equal(m[1], `${slashed(home)}/.claude/squeez/bin/squeez.exe`);
  assert.equal(Buffer.from(m[2], "base64").toString("utf16le"), TRICKY);
});

test("Windows with shell=pwsh encodes for pwsh.exe", async () => {
  const hooks = await loadHooks({ platform: "win32", home: fakeHome() });
  await hooks.config({ shell: "C:\\Program Files\\PowerShell\\7\\pwsh.exe" });
  assert.match(await wrap(hooks, "Get-Date"), / wrap 'pwsh\.exe -NoLogo /);
});

test("Windows with shell=bash gets a quoted forward-slash POSIX wrapper", async () => {
  const home = fakeHome();
  const hooks = await loadHooks({ platform: "win32", home });
  await hooks.config({ shell: "bash" });
  assert.equal(
    await wrap(hooks, "echo 'a b'"),
    `'${slashed(home)}/.claude/squeez/bin/squeez.exe' wrap 'echo '\\''a b'\\'''`,
  );
});

test("Windows falls back to $SHELL when the config names no shell", async () => {
  const hooks = await loadHooks({ platform: "win32", home: fakeHome(), shell: "/usr/bin/bash" });
  await hooks.config({});
  assert.match(await wrap(hooks, "ls"), /^'.+squeez\.exe' wrap 'ls'$/);
});

test("Windows home with backslashes is written with forward slashes", async () => {
  const home = fakeHome("Users\\me");
  const hooks = await loadHooks({ platform: "win32", home });
  await hooks.config({ shell: "bash" });
  const wrapped = await wrap(hooks, "ls");
  assert.ok(!wrapped.includes("\\"), wrapped);
  assert.ok(wrapped.includes("Users/me/.claude/squeez/bin/squeez.exe"), wrapped);
});

test("the `shell` tool name is wrapped like `bash`", async () => {
  const hooks = await loadHooks({ platform: "linux", home: fakeHome() });
  assert.match(await wrap(hooks, "ls", "shell"), / wrap 'ls'$/);
});

test("an already wrapped command is left alone in both forms", async () => {
  const home = fakeHome();
  const hooks = await loadHooks({ platform: "win32", home });
  const powershell = await wrap(hooks, "Get-Date");
  assert.equal(await wrap(hooks, powershell), powershell);
  await hooks.config({ shell: "bash" });
  const posix = await wrap(hooks, "ls");
  assert.equal(await wrap(hooks, posix), posix);
});
