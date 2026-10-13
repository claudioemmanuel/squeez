#!/usr/bin/env node
'use strict';

// See the implementation note in session-start.js. This hook assumes the
// payload carries `transcript_path` pointing at the session JSONL -- confirm
// that field with `claude --debug` on your version before depending on it.

const fs = require('node:fs');
const { readState, writeState, addXp } = require('../lib/state');
const { applySqueezSavings } = require('../lib/squeez');

const COMMENT_PATTERN = /<!--\s*buddy:\s*(.*?)\s*-->/s;

function readStdin() {
  try {
    const raw = fs.readFileSync(0, 'utf8');
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

function lastAssistantText(transcriptPath) {
  if (!transcriptPath || !fs.existsSync(transcriptPath)) return null;
  try {
    const lines = fs.readFileSync(transcriptPath, 'utf8').trim().split('\n');
    for (let i = lines.length - 1; i >= 0; i--) {
      const entry = JSON.parse(lines[i]);
      if (entry.role === 'assistant' || entry.type === 'assistant') {
        return typeof entry.content === 'string' ? entry.content : JSON.stringify(entry.content);
      }
    }
  } catch {
    return null;
  }
  return null;
}

function main() {
  const input = readStdin();
  const transcriptPath = input.transcript_path || input.transcriptPath;
  const text = lastAssistantText(transcriptPath);

  if (text) {
    const match = text.match(COMMENT_PATTERN);
    if (match) {
      const state = readState();
      state.lastComment = match[1].slice(0, 200);
      writeState(state);
    }
  }

  // Capture squeez savings accumulated by the end of the turn (1M tokens = 1 XP).
  const state = readState();
  const gained = applySqueezSavings(state);
  writeState(state);
  if (gained !== 0) addXp(gained);

  process.stdout.write(JSON.stringify({}));
}

main();
