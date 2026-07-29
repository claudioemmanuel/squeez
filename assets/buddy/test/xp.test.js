'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const HOOK = path.join(__dirname, '..', 'hooks', 'post-tool-use.js');

/** Roda o hook num HOME isolado e devolve o state resultante. */
function runHook(home, payload) {
  const result = spawnSync(process.execPath, [HOOK], {
    input: JSON.stringify(payload),
    env: { ...process.env, CLAUDE_BUDDY_HOME: home, SQUEEZ_DIR: path.join(home, 'no-squeez') },
    encoding: 'utf8',
  });
  assert.strictEqual(result.status, 0, `hook falhou: ${result.stderr}`);
  return JSON.parse(fs.readFileSync(path.join(home, 'state.json'), 'utf8'));
}

function freshHome(t) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'buddy-xp-'));
  t.after(() => fs.rmSync(home, { recursive: true, force: true }));
  return home;
}

const bash = (command, response, session = 's1') => ({
  session_id: session,
  tool_name: 'Bash',
  tool_input: { command },
  tool_response: response,
});

test('REGRESSÃO: "error" na saída seguido de comando limpo não paga XP', (t) => {
  // Este é o vazamento que produziu ~700 XP/dia em produção: ERROR_PATTERN era
  // testado contra comando+saída, então um `grep error` armava o gatilho e o
  // Bash seguinte, limpo, pagava 10 XP. Numa sessão de agente o par se repetia
  // indefinidamente.
  const home = freshHome(t);
  runHook(home, bash('grep -rn error src/', 'src/a.rs:10: // error path'));
  const state = runHook(home, bash('ls', 'a.rs'));
  assert.strictEqual(state.xp, 0);
});

test('REGRESSÃO: comando com "test" no nome não vira suíte verde', (t) => {
  // `grep -rn passed tests/` casa TEST_CONTEXT_PATTERN e TEST_PASS_PATTERN no
  // texto do comando. Só a saída pode contar.
  const home = freshHome(t);
  const state = runHook(home, bash('grep -rn passed tests/', 'tests/a.rs:3: // nada aqui'));
  assert.strictEqual(state.xp, 0);
});

test('teste verde de verdade paga, e o exit code manda sobre a regex', (t) => {
  const home = freshHome(t);
  const state = runHook(home, bash('cargo test', { exit_code: 0, stdout: 'test result: ok' }));
  assert.strictEqual(state.xp, 2);
});

test('suíte vermelha que fica verde paga mais, e só uma vez por transição', (t) => {
  const home = freshHome(t);
  let state = runHook(home, bash('cargo test', { exit_code: 1, stdout: 'FAILED' }));
  assert.strictEqual(state.xp, 0, 'suíte vermelha não paga');
  assert.strictEqual(state.lastTestRun, 'fail');

  state = runHook(home, bash('cargo test', { exit_code: 0, stdout: 'ok' }));
  assert.strictEqual(state.xp, 3, 'fail -> pass vale SUITE_RECOVERED');

  state = runHook(home, bash('cargo test', { exit_code: 0, stdout: 'ok' }));
  assert.strictEqual(state.xp, 5, 'segunda verde seguida volta a valer TEST_PASS');
});

test('edits param no teto de 5 por sessão', (t) => {
  const home = freshHome(t);
  let state;
  for (let i = 0; i < 20; i++) {
    state = runHook(home, { session_id: 's1', tool_name: 'Edit', tool_input: {} });
  }
  assert.strictEqual(state.xp, 5);
  assert.strictEqual(state.sessionEditXp, 5);
});

test('teto duro de 8 XP de ação por sessão', (t) => {
  const home = freshHome(t);
  let state;
  for (let i = 0; i < 10; i++) {
    state = runHook(home, { session_id: 's1', tool_name: 'Write', tool_input: {} });
    state = runHook(home, bash('git commit -m x', { exit_code: 0, stdout: '' }));
  }
  assert.strictEqual(state.sessionXp, 8);
  assert.strictEqual(state.xp, 8);
});

test('sessão nova reabre o teto', (t) => {
  const home = freshHome(t);
  let state;
  for (let i = 0; i < 10; i++) {
    state = runHook(home, bash('git commit -m x', { exit_code: 0, stdout: '' }, 's1'));
  }
  assert.strictEqual(state.xp, 8);

  state = runHook(home, bash('git commit -m y', { exit_code: 0, stdout: '' }, 's2'));
  assert.strictEqual(state.sessionXp, 2, 'contador zerou na sessão nova');
  assert.strictEqual(state.xp, 10);
});

test('commit que falhou não paga', (t) => {
  const home = freshHome(t);
  const state = runHook(home, bash('git commit -m x', { exit_code: 1, stdout: 'nothing to commit' }));
  assert.strictEqual(state.xp, 0);
});

test('migração v1 -> v2 zera o XP uma vez e preserva a identidade', (t) => {
  const home = freshHome(t);
  const legacy = {
    identity: 'host:user',
    name: 'Pato',
    stats: { SNARK: 83, DEBUGGING: 6, WISDOM: 34, CHAOS: 47, PATIENCE: 34 },
    peakName: 'SNARK',
    dumpName: 'DEBUGGING',
    archetype: 'Reclamão',
    intensity: 'intenso',
    xp: 3563,
    shiny: false,
    createdAt: '2026-07-23',
    lastSessionDate: '2026-07-28',
    hasRecentError: true,
    lastComment: null,
    squeez: { lastStartTs: 1, lastNetSaved: 0, lifetimeNetSaved: 137290, xpGranted: 0, rate: 1000000 },
  };
  fs.writeFileSync(path.join(home, 'state.json'), JSON.stringify(legacy));

  // lib/state resolve STATE_DIR no require, então a migração tem que rodar no
  // processo filho pra respeitar o HOME isolado.
  const state = runHook(home, { session_id: 's1', tool_name: 'Read', tool_input: {} });

  assert.strictEqual(state.xp, 0, 'placar zerado');
  assert.strictEqual(state.schemaVersion, 2);
  assert.strictEqual(state.xpResetReason, 'migration');
  assert.strictEqual(state.name, 'Pato', 'identidade preservada');
  assert.strictEqual(state.archetype, 'Reclamão');
  assert.strictEqual(state.createdAt, '2026-07-23');
  assert.strictEqual(state.squeez.lifetimeNetSaved, 137290, 'histórico de economia preservado');
  assert.strictEqual(state.hasRecentError, undefined, 'campo do gatilho removido');

  // Idempotente: uma segunda passada não zera de novo.
  const bumped = JSON.parse(fs.readFileSync(path.join(home, 'state.json'), 'utf8'));
  bumped.xp = 137;
  fs.writeFileSync(path.join(home, 'state.json'), JSON.stringify(bumped));
  const again = runHook(home, { session_id: 's1', tool_name: 'Read', tool_input: {} });
  assert.strictEqual(again.xp, 137);
});
