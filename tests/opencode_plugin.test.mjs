// Behavioural tests for opencode-plugin/squeez.js, run by `node --test`
// (driven from tests/test_hosts_opencode.rs). The plugin is loaded the way
// OpenCode loads it: default export, then `server()` and its hooks (1.x) or
// `setup(ctx)` (2.x).

import assert from "node:assert/strict";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

const PLUGIN = join(fileURLToPath(new URL(".", import.meta.url)), "..", "opencode-plugin", "squeez.js");
const REAL_PLATFORM = process.platform;
let loads = 0;

// The plugin reads the platform, HOME and SHELL once at import, so each case
// imports its own copy.
async function loadPlugin({ platform, home, shell }) {
  Object.defineProperty(process, "platform", { value: platform });
  process.env.HOME = home;
  delete process.env.USERPROFILE;
  if (shell === undefined) delete process.env.SHELL;
  else process.env.SHELL = shell;
  try {
    const mod = await import(`${pathToFileURL(PLUGIN).href}?load=${++loads}`);
    return mod.default;
  } finally {
    Object.defineProperty(process, "platform", { value: REAL_PLATFORM });
  }
}

async function loadHooks(options) {
  return (await loadPlugin(options)).server({}, {});
}

// Runs `setup()` against a stand-in for the OpenCode 2.x context and returns
// the callbacks it registered, keyed by "<domain>.<hook>".
async function loadSetup(options, events = []) {
  const plugin = await loadPlugin(options);
  const hooks = {};
  const domain = (name) => ({ hook: (hook, callback) => (hooks[`${name}.${hook}`] = callback) });
  const subscribe = async function* () {
    yield* events;
  };
  plugin.setup({ event: { subscribe }, shell: domain("shell"), tool: domain("tool") });
  return hooks;
}

// A home whose squeez is a script: it answers `budget-params` and appends
// every call, with whatever arrived on stdin, to `calls.log` beside it.
function recordingHome() {
  const home = mkdtempSync(join(tmpdir(), "squeez-plugin-"));
  const bin = join(home, ".claude", "squeez", "bin");
  mkdirSync(bin, { recursive: true });
  writeFileSync(
    join(bin, "squeez"),
    [
      "#!/bin/sh",
      `if [ "$1" = "budget-params" ]; then printf '{"limit":200}'; fi`,
      // Only track-result reads stdin; the other calls leave theirs open.
      `if [ "$1" = "track-result" ]; then input="$(cat)"; fi`,
      `printf '%s|%s\\n' "$*" "$input" >> "$(dirname "$0")/calls.log"`,
      "",
    ].join("\n"),
  );
  chmodSync(join(bin, "squeez"), 0o755);
  return { home, log: join(bin, "calls.log") };
}

// The plugin does not wait for its children, so the log is polled.
async function calls(log, count) {
  let lines = [];
  for (let waited = 0; waited < 5000; waited += 25) {
    lines = existsSync(log) ? readFileSync(log, "utf8").split("\n").filter(Boolean) : [];
    if (lines.length >= count) break;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  return lines.sort();
}

// The recording squeez is a shell script, which Windows cannot execute.
const NEEDS_SH = { skip: REAL_PLATFORM === "win32" };

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
  // The progress stream is silenced, or every command's output starts with a
  // `#< CLIXML` block.
  assert.equal(
    Buffer.from(m[2], "base64").toString("utf16le"),
    `$ProgressPreference = 'SilentlyContinue'; ${TRICKY}`,
  );
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

test("2.x setup wraps through the shell the event names", async () => {
  const home = fakeHome();
  const linux = await loadSetup({ platform: "linux", home, shell: "/bin/zsh" });
  const e = { command: "echo 'a b'", shell: "/bin/zsh" };
  await linux["shell.create.before"](e);
  assert.equal(e.command, `${home}/.claude/squeez/bin/squeez wrap 'echo '\\''a b'\\'''`);

  const windows = await loadSetup({ platform: "win32", home: fakeHome() });
  const pwsh = { command: "Get-Date", shell: "C:\\Program Files\\PowerShell\\7\\pwsh.exe" };
  await windows["shell.create.before"](pwsh);
  assert.match(pwsh.command, / wrap 'pwsh\.exe -NoLogo /);
  const wrapped = pwsh.command;
  await windows["shell.create.before"](pwsh);
  assert.equal(pwsh.command, wrapped);
});

test("2.x setup registers nothing without squeez and survives a bare context", async () => {
  const empty = mkdtempSync(join(tmpdir(), "squeez-plugin-"));
  assert.deepEqual(await loadSetup({ platform: "linux", home: empty }), {});
  const plugin = await loadPlugin({ platform: "linux", home: fakeHome() });
  plugin.setup({});
  plugin.setup(undefined);
});

test("2.x execute.before waits for the budget and keeps explicit values", NEEDS_SH, async () => {
  const { home } = recordingHome();
  const hooks = await loadSetup({ platform: "linux", home });
  const bare = { tool: "read", input: { filePath: "a.txt" } };
  await hooks["tool.execute.before"](bare);
  assert.equal(bare.input.limit, 200);

  const explicit = { tool: "read", input: { limit: 5 } };
  await hooks["tool.execute.before"](explicit);
  assert.equal(explicit.input.limit, 5);

  const other = { tool: "write", input: {} };
  await hooks["tool.execute.before"](other);
  assert.deepEqual(other.input, {});
});

test("2.x events track finished shells and init each session once", NEEDS_SH, async () => {
  const { home, log } = recordingHome();
  const started = (type, sessionID) => ({ type, data: { sessionID } });
  const hooks = await loadSetup({ platform: "linux", home }, [
    started("session.execution.started", "s1"),
    started("session.execution.started", "s1"),
    started("session.execution.started.1", "s2"),
    { type: "shell.exited", data: { id: "sh1", status: "exited", exit: 0 } },
    { type: "shell.exited.1", data: { id: "sh2", status: "killed" } },
    { type: "shell.exited", data: { id: "sh3", status: "running" } },
    { type: "shell.exited", data: { status: "exited" } },
    { type: "shell.started", data: { id: "sh4", status: "exited" } },
  ]);
  // A tool hook never reports the shell.
  await hooks["tool.execute.after"]({ tool: "bash" });
  await hooks["tool.execute.after"]({
    tool: "read",
    input: { path: "/tmp/a.rs" },
    status: "completed",
    result: { content: [{ type: "text", text: "fn main" }, { type: "file", uri: "file:///tmp/a.rs" }] },
  });

  assert.deepEqual(await calls(log, 5), [
    "init --host=opencode|",
    "init --host=opencode|",
    'track-result bash|{"tool_name":"Bash","shell_id":"sh1","shell_status":"exited","exit_code":0}',
    'track-result bash|{"tool_name":"Bash","shell_id":"sh2","shell_status":"killed"}',
    'track-result read|{"tool_name":"read","tool_input":{"file_path":"/tmp/a.rs"},"tool_result":{"content":"fn main"}}',
  ]);
});

test("2.x execute.after sends the search target, and the message of a failed call", NEEDS_SH, async () => {
  const { home, log } = recordingHome();
  const hooks = await loadSetup({ platform: "linux", home });
  await hooks["tool.execute.after"]({
    tool: "grep",
    input: { pattern: "fn main", path: "src" },
    status: "completed",
    result: { content: "src/main.rs:1:fn main" },
  });
  await hooks["tool.execute.after"]({
    tool: "glob",
    input: { pattern: "*.rs" },
    status: "error",
    error: { message: "error: no such directory" },
  });
  await hooks["tool.execute.after"]({ tool: "write", input: { path: "a" }, status: "completed", result: {} });

  assert.deepEqual(await calls(log, 2), [
    'track-result glob|{"tool_name":"glob","tool_input":{"pattern":"*.rs"},"tool_result":{"content":"error: no such directory"}}',
    'track-result grep|{"tool_name":"grep","tool_input":{"pattern":"fn main","path":"src"},"tool_result":{"content":"src/main.rs:1:fn main"}}',
  ]);
});

test("1.x tool.execute.after sends what the tool read", NEEDS_SH, async () => {
  const { home, log } = recordingHome();
  const hooks = await loadHooks({ platform: "linux", home });
  await hooks["tool.execute.after"](
    { tool: "read", args: { filePath: "/tmp/a.rs", limit: 10 } },
    { title: "a.rs", output: "fn main", metadata: {} },
  );
  await hooks["tool.execute.after"]({ tool: "grep", args: { pattern: "x", path: "src" } }, undefined);
  await hooks["tool.execute.after"]({ tool: "bash", args: { command: "ls" } }, { output: "a.rs" });
  await hooks["tool.execute.after"]({ tool: "write", args: { filePath: "b" } }, { output: "" });

  assert.deepEqual(await calls(log, 3), [
    'track-result bash|{"tool_name":"Bash"}',
    'track-result grep|{"tool_name":"grep","tool_input":{"pattern":"x","path":"src"},"tool_result":{}}',
    'track-result read|{"tool_name":"read","tool_input":{"file_path":"/tmp/a.rs"},"tool_result":{"content":"fn main"}}',
  ]);
});
