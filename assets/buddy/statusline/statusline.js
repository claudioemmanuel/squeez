#!/usr/bin/env node
'use strict';

const { readState, currentRank } = require('../lib/state');
const { renderStatusline } = require('../lib/art');

function main() {
  const state = readState();
  const rank = currentRank(state);

  const duck = renderStatusline({
    archetype: state.archetype,
    rankLabel: rank.label,
    rankHex: rank.hex,
    shiny: state.shiny,
    ansi: true,
  });

  const bubble = state.lastComment ? `  "${state.lastComment}"` : '';
  process.stdout.write(`${duck}${bubble}\n`);
}

main();
