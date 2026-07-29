#!/usr/bin/env node
'use strict';

// CLI stdlib do pato — substitui o MCP server do pato-buddy standalone (única
// parte com deps npm, não vendorada). Uso:
//   node buddy-cli.js card            desenha o card completo
//   node buddy-cli.js stats           status, ritmo de XP e projeção
//   node buddy-cli.js pet             carinho (sem efeito em XP)
//   node buddy-cli.js rename <nome>   renomeia o pato
//   node buddy-cli.js reset           zera o XP (mantém identidade e nome)

const { readState, writeState, currentRank, resetXp, todayIso } = require('./lib/state');
const { renderCard } = require('./lib/art');
const { TOKENS_PER_XP } = require('./lib/squeez');
const { RANKS, MAX_RANK_INDEX, daysBetween } = require('./lib/engine');

function card(state, rank) {
  return renderCard({
    name: state.name,
    archetype: state.archetype,
    rankLabel: rank.label,
    rankHex: rank.hex,
    rankIndex: rank.index,
    xp: state.xp,
    shiny: state.shiny,
    ansi: false, // saída vai pra conversa, não pro terminal
  });
}

function br(n) {
  return Math.round(n).toLocaleString('pt-BR');
}

/** Data ISO daqui a `days` dias, sem depender de lib de data. */
function isoInDays(days) {
  const d = new Date();
  d.setDate(d.getDate() + Math.ceil(days));
  return d.toISOString().slice(0, 10);
}

/**
 * Linhas de ritmo: sem isso não dá pra saber se a curva está calibrada. Mostra
 * XP por dia desde o último reset, quanto falta pro próximo degrau e em que
 * data ele cai nesse ritmo.
 */
function paceLines(state, rank) {
  const since = state.xpResetAt || state.createdAt;
  const days = Math.max(1, daysBetween(since, todayIso()));
  const perDay = state.xp / days;

  const granted = Number(state.squeez && state.squeez.xpGranted) || 0;
  const fromAction = Math.max(0, state.xp - granted);

  const lines = [
    `Ritmo: ${perDay.toFixed(1)} XP/dia (${state.xp} XP em ${days} ${days === 1 ? 'dia' : 'dias'} desde ${since})`,
    `  economia squeez: ${granted} XP · ação: ${fromAction} XP`,
  ];

  if (rank.index >= MAX_RANK_INDEX) {
    lines.push('Próximo degrau: nenhum — Lendário I é o topo.');
    return lines;
  }

  const next = RANKS[rank.index + 1];
  const missing = next.xpRequired - state.xp;
  const eta = perDay > 0 ? `~${isoInDays(missing / perDay)}` : 'nunca nesse ritmo';
  lines.push(`Próximo degrau: ${next.label} em ${br(missing)} XP (${eta})`);
  return lines;
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
          ...paceLines(state, rank),
          `Economia squeez acumulada: ${br(lifetime)} tokens líquidos`,
          `Próximo XP de economia em: ${br(nextXpAt - lifetime)} tokens (taxa ${br(TOKENS_PER_XP)}:1)`,
          `Criado em: ${state.createdAt || '?'}`,
        ].join('\n')
      );
      return;
    }
    case 'pet': {
      // Sem efeito em XP, por design (herdado do MCP pet_buddy).
      state.lastSessionDate = todayIso();
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
    case 'reset': {
      const had = state.xp;
      writeState(resetXp(state, 'manual'));
      console.log(`XP zerado (era ${had}). ${state.name} continua o mesmo pato — só o placar voltou pro começo.`);
      return;
    }
    case 'card':
    default:
      console.log(card(state, rank));
  }
}

main();
