#!/usr/bin/env node
'use strict';

/**
 * NOTA DE IMPLEMENTAÇÃO: os nomes exatos de campo no JSON de stdin de cada
 * hook podem variar entre versões do Claude Code. Este script lê com
 * fallback defensivo e não quebra se os campos não baterem -- rode
 * `claude --debug` durante o desenvolvimento pra conferir o payload real
 * da sua versão antes de confiar cegamente nestes nomes.
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
  readStdin(); // reservado pra quando quisermos ler accountUuid/session id daqui
  const state = touchSession();
  const rank = currentRank(state);

  const card = renderCard({
    name: state.name,
    archetype: state.archetype,
    rankLabel: rank.label,
    rankHex: rank.hex,
    xp: state.xp,
    shiny: state.shiny,
    ansi: false, // texto puro pra ir no contexto, não no terminal
  });

  const additionalContext = [
    `Você tem um companheiro chamado ${state.name} (arquétipo: ${state.archetype}, intensidade: ${state.intensity}, degrau: ${rank.label}).`,
    'Quando o usuário chamar o companheiro pelo nome diretamente na conversa, deixe-o responder em primeira pessoa, no tom do arquétipo, e você (Claude) fica de fora dessa fala.',
    'Fora isso, não mencione o companheiro a menos que ele tenha algo relevante a comentar sobre o que está acontecendo na sessão.',
    `Card atual:\n${card}`,
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
