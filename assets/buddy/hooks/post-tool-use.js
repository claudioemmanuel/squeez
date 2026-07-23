#!/usr/bin/env node
'use strict';

// Ver nota de implementação em session-start.js sobre os nomes de campo do
// stdin -- confira com `claude --debug` antes de confiar cegamente.

const { readState, writeState, addXp } = require('../lib/state');
const { applySqueezSavings } = require('../lib/squeez');

const XP = {
  EDIT_OR_WRITE: 1,
  TEST_PASS: 5,
  GIT_COMMIT: 3,
  BUG_RESOLVED: 10,
};

const ERROR_PATTERN = /\b(error|exception|traceback|failed|failure)\b/i;
const TEST_PASS_PATTERN = /\b(passed|passing|0 failing|all tests pass|✓)\b/i;
const TEST_CONTEXT_PATTERN = /\btest/i;
const COMMIT_PATTERN = /\bgit\s+commit\b/i;

function readStdin() {
  try {
    const raw = require('node:fs').readFileSync(0, 'utf8');
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

function main() {
  const input = readStdin();
  const toolName = input.tool_name || input.toolName || '';
  const toolInput = input.tool_input || input.toolInput || {};
  const toolOutput = input.tool_response || input.toolResponse || input.output || '';
  const command = typeof toolInput.command === 'string' ? toolInput.command : '';
  const outputText = typeof toolOutput === 'string' ? toolOutput : JSON.stringify(toolOutput || '');
  const combined = `${command}\n${outputText}`;

  let gained = 0;
  let state = readState();

  if (toolName === 'Edit' || toolName === 'Write') {
    gained += XP.EDIT_OR_WRITE;
  }

  if (toolName === 'Bash') {
    if (COMMIT_PATTERN.test(command)) {
      gained += XP.GIT_COMMIT;
    }
    if (TEST_CONTEXT_PATTERN.test(combined) && TEST_PASS_PATTERN.test(combined)) {
      gained += XP.TEST_PASS;
    }
    if (ERROR_PATTERN.test(combined)) {
      state.hasRecentError = true;
    } else if (state.hasRecentError) {
      // heurística MVP: próximo comando limpo depois de um erro = "bug resolvido"
      gained += XP.BUG_RESOLVED;
      state.hasRecentError = false;
    }
  }

  // XP por economia do squeez (100k tokens líquidos = 1 XP); pode ser
  // negativo uma única vez, na re-base após mudança de taxa.
  gained += applySqueezSavings(state);

  writeState(state);
  if (gained !== 0) addXp(gained);

  // PostToolUse não precisa emitir nada pro Claude ver; silencioso por design.
  process.stdout.write(JSON.stringify({}));
}

main();
