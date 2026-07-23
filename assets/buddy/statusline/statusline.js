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

  // --compact: só a cabeça colorida, sem balão — modo usado quando o pato
  // divide a linha com um HUD de terceiro (espaço é caro na ponta da linha).
  const compact = process.argv.includes('--compact');
  const bubble = !compact && state.lastComment ? `  "${state.lastComment}"` : '';
  process.stdout.write(`${duck}${bubble}\n`);
}

main();
