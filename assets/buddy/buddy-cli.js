#!/usr/bin/env node
'use strict';

// The duck's stdlib CLI — replaces the standalone pato-buddy MCP server (the
// only part with npm deps, not vendored). Usage:
//   node buddy-cli.js card            draw the full card
//   node buddy-cli.js stats           status, XP pace and projection
//   node buddy-cli.js pet             pet it (no XP effect)
//   node buddy-cli.js rename <name>   rename the duck
//   node buddy-cli.js reset           zero the XP (keeps identity and name)

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
    ansi: false, // output goes to the conversation, not the terminal
  });
}

function br(n) {
  return Math.round(n).toLocaleString('en-US');
}

/** ISO date `days` days from now, without depending on a date library. */
function isoInDays(days) {
  const d = new Date();
  d.setDate(d.getDate() + Math.ceil(days));
  return d.toISOString().slice(0, 10);
}

/**
 * Pace lines: without these there is no way to tell whether the curve is
 * calibrated. Shows XP per day since the last reset, how much is missing for
 * the next rank and the date it lands on at this pace.
 */
function paceLines(state, rank) {
  const since = state.xpResetAt || state.createdAt;
  const days = Math.max(1, daysBetween(since, todayIso()));
  const perDay = state.xp / days;

  const granted = Number(state.squeez && state.squeez.xpGranted) || 0;
  const fromAction = Math.max(0, state.xp - granted);

  const lines = [
    `Pace: ${perDay.toFixed(1)} XP/day (${state.xp} XP in ${days} ${days === 1 ? 'day' : 'days'} since ${since})`,
    `  squeez savings: ${granted} XP · action: ${fromAction} XP`,
  ];

  if (rank.index >= MAX_RANK_INDEX) {
    lines.push('Next rank: none — Legendary I is the top.');
    return lines;
  }

  const next = RANKS[rank.index + 1];
  const missing = next.xpRequired - state.xp;
  const eta = perDay > 0 ? `~${isoInDays(missing / perDay)}` : 'never at this pace';
  lines.push(`Next rank: ${next.label} in ${br(missing)} XP (${eta})`);
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
          `Accumulated squeez savings: ${br(lifetime)} net tokens`,
          `Next savings XP in: ${br(nextXpAt - lifetime)} tokens (rate ${br(TOKENS_PER_XP)}:1)`,
          `Created: ${state.createdAt || '?'}`,
        ].join('\n')
      );
      return;
    }
    case 'pet': {
      // No XP effect, by design (inherited from the pet_buddy MCP).
      state.lastSessionDate = todayIso();
      writeState(state);
      console.log(`${state.name} wags its tail, happy. (no XP effect)`);
      return;
    }
    case 'rename': {
      const name = rest.join(' ').trim();
      if (!name) {
        console.error('usage: buddy-cli.js rename <name>');
        process.exitCode = 1;
        return;
      }
      state.name = name.slice(0, 40);
      writeState(state);
      console.log(`From now on it answers to ${state.name}.`);
      return;
    }
    case 'reset': {
      const had = state.xp;
      writeState(resetXp(state, 'manual'));
      console.log(`XP zeroed (was ${had}). ${state.name} is still the same duck — only the score went back to the start.`);
      return;
    }
    case 'card':
    default:
      console.log(card(state, rank));
  }
}

main();
