#!/usr/bin/env node
'use strict';

// CLI stdlib do pato — substitui o MCP server do pato-buddy standalone (única
// parte com deps npm, não vendorada). Uso:
//   node buddy-cli.js card            desenha o card completo
//   node buddy-cli.js stats           status + economia squeez acumulada
//   node buddy-cli.js pet             carinho (sem efeito em XP)
//   node buddy-cli.js rename <nome>   renomeia o pato

const { readState, writeState, currentRank } = require('./lib/state');
const { renderCard } = require('./lib/art');
const { TOKENS_PER_XP } = require('./lib/squeez');

function card(state, rank) {
  return renderCard({
    name: state.name,
    archetype: state.archetype,
    rankLabel: rank.label,
    rankHex: rank.hex,
    xp: state.xp,
    shiny: state.shiny,
    ansi: false, // saída vai pra conversa, não pro terminal
  });
}

function main() {
  const [cmd, ...rest] = process.argv.slice(2);
  const state = readState();
  const rank = currentRank(state);

  switch (cmd) {
    case 'stats': {
      const t = state.squeez || {};
      const lifetime = Number(t.lifetimeNetSaved) || 0;
      const nextXpAt = (Math.floor(lifetime / TOKENS_PER_XP) + 1) * TOKENS_PER_XP;
      console.log(
        [
          `${state.name} — ${state.archetype} · ${rank.label}${state.shiny ? ' · shiny ✦' : ''}`,
          `XP: ${state.xp}`,
          `Economia squeez acumulada: ${lifetime.toLocaleString('pt-BR')} tokens líquidos`,
          `Próximo XP de economia em: ${(nextXpAt - lifetime).toLocaleString('pt-BR')} tokens (taxa ${TOKENS_PER_XP.toLocaleString('pt-BR')}:1)`,
          `Criado em: ${state.createdAt || '?'}`,
        ].join('\n')
      );
      return;
    }
    case 'pet': {
      // Sem efeito em XP, por design (herdado do MCP pet_buddy).
      state.lastSessionDate = new Date().toISOString().slice(0, 10);
      writeState(state);
      console.log(`${state.name} balança o rabinho, feliz. (sem efeito em XP)`);
      return;
    }
    case 'rename': {
      const name = rest.join(' ').trim();
      if (!name) {
        console.error('uso: buddy-cli.js rename <nome>');
        process.exitCode = 1;
        return;
      }
      state.name = name.slice(0, 40);
      writeState(state);
      console.log(`Agora ele atende por ${state.name}.`);
      return;
    }
    case 'card':
    default:
      console.log(card(state, rank));
  }
}

main();
