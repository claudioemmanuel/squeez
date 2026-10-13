#!/usr/bin/env node
'use strict';

/**
 * IMPLEMENTATION NOTE: the exact field names in each hook's stdin JSON can
 * vary between Claude Code versions. This script reads them with a defensive
 * fallback and does not break if the fields don't match -- run
 * `claude --debug` during development to check the real payload for your
 * version before trusting these names blindly.
 */

const { touchSession, currentRank } = require('../lib/state');
const { renderCard } = require('../lib/art');

function readStdin() {
  try {
    const raw = require('node:fs').readFileSync(0, 'utf8');
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

function main() {
  readStdin(); // reserved for when we want to read accountUuid/session id from here
  const state = touchSession();
  const rank = currentRank(state);

  const card = renderCard({
    name: state.name,
    archetype: state.archetype,
    rankLabel: rank.label,
    rankHex: rank.hex,
    rankIndex: rank.index,
    xp: state.xp,
    shiny: state.shiny,
    ansi: false, // plain text, meant for the context, not the terminal
  });

  const additionalContext = [
    `You have a companion named ${state.name} (archetype: ${state.archetype}, intensity: ${state.intensity}, rank: ${rank.label}).`,
    'When the user calls the companion by name directly in the conversation, let it reply in first person, in the tone of its archetype, and you (Claude) stay out of that speech.',
    'Otherwise, do not mention the companion unless it has something relevant to comment about what is happening in the session.',
    `Current card:\n${card}`,
  ].join('\n');

  const output = {
    hookSpecificOutput: {
      hookEventName: 'SessionStart',
      additionalContext,
    },
  };

  process.stdout.write(JSON.stringify(output));
}

main();
