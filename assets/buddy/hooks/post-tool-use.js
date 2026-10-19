#!/usr/bin/env node
'use strict';

// See the implementation note in session-start.js about the stdin field
// names -- check with `claude --debug` before trusting them blindly.

const { readState, writeState, addXp, sessionKeyFrom } = require('../lib/state');
const { applySqueezSavings } = require('../lib/squeez');

// Action XP is the SECONDARY source and is capped by SESSION_ACTION_CAP. The
// primary source is squeez token savings (lib/squeez.js), which has no cap
// because it is not gameable: either squeez saved real tokens, or it didn't.
//
// The old BUG_RESOLVED trigger (10 XP) was REMOVED, not rebalanced. It paid out
// every time the word "error" appeared in command+output and the next Bash came
// back clean -- that is, a `grep error`, a path containing "failed" or a log
// line armed the trigger, and in an agent session the arm/fire pair repeated
// indefinitely. Measured in production: ~700 XP/day against the ~70 the design
// assumed. There is no way to tell "bug fixed" from "the word error appeared on
// screen" by regex; the replacement (SUITE_RECOVERED) requires a real
// fail -> pass transition of a test run.
const XP = {
  EDIT_OR_WRITE: 1,
  GIT_COMMIT: 2,
  TEST_PASS: 2,
  SUITE_RECOVERED: 3,
};

const SESSION_ACTION_CAP = 8; // hard cap on action XP per session
const SESSION_EDIT_CAP = 5; // within the cap above, how much can come from edits

// These patterns are applied ONLY TO THE OUTPUT, never to the command text --
// the command is exactly what generated the false positives (`grep error`,
// `cargo test`, paths containing "failed").
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

/** Exit code from the payload when the host sends it; null when it can't be known. */
function exitCodeOf(response) {
  if (!response || typeof response !== 'object') return null;
  for (const key of ['exit_code', 'exitCode', 'returnCode', 'status']) {
    const value = response[key];
    if (typeof value === 'number') return value;
  }
  return null;
}

/**
 * Outcome of a test run: 'pass', 'fail' or null (not a test).
 * Prefers the exit code; the regex only kicks in when the host doesn't expose it.
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

  // The cap is per session: a new session zeroes the counters.
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
      // A suite that was red and turned green is worth more -- it is the only
      // honest way to detect "something got fixed" without guessing by regex.
      action += state.lastTestRun === 'fail' ? XP.SUITE_RECOVERED : XP.TEST_PASS;
    }
    if (outcome) state.lastTestRun = outcome;
  }

  // Apply the session's hard action cap.
  const room = Math.max(0, SESSION_ACTION_CAP - (state.sessionXp || 0));
  const capped = Math.min(action, room);
  state.sessionXp = (state.sessionXp || 0) + capped;

  // XP from squeez savings: primary source, no cap. Can be negative exactly
  // once, on the re-base after a rate change.
  const savings = applySqueezSavings(state);

  writeState(state);
  const gained = capped + savings;
  if (gained !== 0) addXp(gained);

  // PostToolUse doesn't need to emit anything for Claude to see; silent by design.
  process.stdout.write(JSON.stringify({}));
}

main();
