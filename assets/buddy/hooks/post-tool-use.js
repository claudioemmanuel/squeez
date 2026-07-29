#!/usr/bin/env node
'use strict';

// Ver nota de implementação em session-start.js sobre os nomes de campo do
// stdin -- confira com `claude --debug` antes de confiar cegamente.

const { readState, writeState, addXp, sessionKeyFrom } = require('../lib/state');
const { applySqueezSavings } = require('../lib/squeez');

// XP de ação é a fonte SECUNDÁRIA e é limitada por SESSION_ACTION_CAP. A fonte
// primária é a economia de tokens do squeez (lib/squeez.js), que não tem teto
// porque não é gamificável: ou o squeez economizou tokens de verdade, ou não.
//
// O gatilho antigo BUG_RESOLVED (10 XP) foi REMOVIDO, não reajustado. Ele pagava
// sempre que a palavra "error" aparecia em comando+saída e o Bash seguinte vinha
// limpo -- ou seja, um `grep error`, um caminho com "failed" ou uma linha de log
// armavam o gatilho, e numa sessão de agente o par arma/dispara se repetia
// indefinidamente. Medido em produção: ~700 XP/dia contra os ~70 que o design
// assumia. Não dá pra distinguir "bug resolvido" de "a palavra error apareceu na
// tela" por regex; o substituto (SUITE_RECOVERED) exige uma transição real
// falha -> sucesso de uma execução de teste.
const XP = {
  EDIT_OR_WRITE: 1,
  GIT_COMMIT: 2,
  TEST_PASS: 2,
  SUITE_RECOVERED: 3,
};

const SESSION_ACTION_CAP = 8; // teto duro de XP de ação por sessão
const SESSION_EDIT_CAP = 5; // dentro do teto acima, quanto pode vir de edits

// Estes padrões são aplicados SÓ À SAÍDA, nunca ao texto do comando -- o comando
// é justamente o que gerava os falsos positivos (`grep error`, `cargo test`,
// caminhos contendo "failed").
const ERROR_PATTERN = /\b(error|exception|traceback|failed|failure)\b/i;
const TEST_PASS_PATTERN = /\b(passed|passing|0 failing|all tests pass|test result: ok)\b|✓/i;
const TEST_COMMAND_PATTERN = /\b(test|spec|jest|vitest|pytest|mocha)\b/i;
const COMMIT_PATTERN = /\bgit\s+commit\b/i;

function readStdin() {
  try {
    const raw = require('node:fs').readFileSync(0, 'utf8');
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

/** Exit code do payload quando o host manda; null quando não dá pra saber. */
function exitCodeOf(response) {
  if (!response || typeof response !== 'object') return null;
  for (const key of ['exit_code', 'exitCode', 'returnCode', 'status']) {
    const value = response[key];
    if (typeof value === 'number') return value;
  }
  return null;
}

/**
 * Resultado de uma execução de teste: 'pass', 'fail' ou null (não era teste).
 * Prefere o exit code; a regex só entra quando o host não expõe o código.
 */
function testOutcome(command, output, exitCode) {
  if (!TEST_COMMAND_PATTERN.test(command)) return null;
  if (exitCode !== null) return exitCode === 0 ? 'pass' : 'fail';
  if (ERROR_PATTERN.test(output)) return 'fail';
  if (TEST_PASS_PATTERN.test(output)) return 'pass';
  return null;
}

function main() {
  const input = readStdin();
  const toolName = input.tool_name || input.toolName || '';
  const toolInput = input.tool_input || input.toolInput || {};
  const toolResponse = input.tool_response || input.toolResponse || input.output || '';
  const command = typeof toolInput.command === 'string' ? toolInput.command : '';
  const output =
    typeof toolResponse === 'string' ? toolResponse : JSON.stringify(toolResponse || '');
  const exitCode = exitCodeOf(toolResponse);

  const state = readState();

  // O teto é por sessão: uma sessão nova zera os contadores.
  const sessionKey = sessionKeyFrom(input);
  if (state.sessionKey !== sessionKey) {
    state.sessionKey = sessionKey;
    state.sessionXp = 0;
    state.sessionEditXp = 0;
  }

  let action = 0;

  if (toolName === 'Edit' || toolName === 'Write') {
    const room = Math.max(0, SESSION_EDIT_CAP - (state.sessionEditXp || 0));
    const gain = Math.min(XP.EDIT_OR_WRITE, room);
    state.sessionEditXp = (state.sessionEditXp || 0) + gain;
    action += gain;
  }

  if (toolName === 'Bash') {
    if (COMMIT_PATTERN.test(command) && exitCode !== 1) {
      action += XP.GIT_COMMIT;
    }

    const outcome = testOutcome(command, output, exitCode);
    if (outcome === 'pass') {
      // Suíte que estava vermelha e ficou verde vale mais -- é a única forma
      // honesta de detectar "consertou alguma coisa" sem adivinhar por regex.
      action += state.lastTestRun === 'fail' ? XP.SUITE_RECOVERED : XP.TEST_PASS;
    }
    if (outcome) state.lastTestRun = outcome;
  }

  // Aplica o teto duro de ação da sessão.
  const room = Math.max(0, SESSION_ACTION_CAP - (state.sessionXp || 0));
  const capped = Math.min(action, room);
  state.sessionXp = (state.sessionXp || 0) + capped;

  // XP por economia do squeez: fonte primária, sem teto. Pode ser negativo uma
  // única vez, na re-base após mudança de taxa.
  const savings = applySqueezSavings(state);

  writeState(state);
  const gained = capped + savings;
  if (gained !== 0) addXp(gained);

  // PostToolUse não precisa emitir nada pro Claude ver; silencioso por design.
  process.stdout.write(JSON.stringify({}));
}

main();
