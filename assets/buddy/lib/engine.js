'use strict';

/**
 * Deterministic engine for pato-buddy.
 *
 * Everything in this file is a pure function (no I/O, no direct Date.now() in
 * the central calculations) on purpose: the trigger, the archetype and the
 * personality intensity need to be auditable and table-testable, not
 * "emergent". Only the final comment text (outside this file) is LLM-generated.
 */

const { rankLabel } = require('./i18n');

const STAT_NAMES = ['DEBUGGING', 'PATIENCE', 'CHAOS', 'WISDOM', 'SNARK'];

// ---------------------------------------------------------------------------
// Deterministic hash + PRNG (same algorithm family documented for the original
// /buddy: identity hash -> seed -> deterministic PRNG).
// ---------------------------------------------------------------------------

/** 32-bit FNV-1a. Deterministic, no external dependencies. */
function fnv1a(str) {
  let hash = 0x811c9dc5;
  for (let i = 0; i < str.length; i++) {
    hash ^= str.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

/** mulberry32 PRNG: fast, deterministic, quality good enough for this. */
function mulberry32(seed) {
  let a = seed >>> 0;
  return function next() {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const SALT = 'pato-buddy-2026';

function seedFromIdentity(identity) {
  return fnv1a(SALT + ':' + identity);
}

// ---------------------------------------------------------------------------
// Stat generation (peak/dump)
// ---------------------------------------------------------------------------

/**
 * Generates the 5 stats (0-100) from a stable identity (e.g. account id).
 * There is always exactly 1 peak stat (the highest) and 1 dump stat (the
 * lowest) -- the archetype decision tree depends on that guarantee.
 */
function generateStats(identity) {
  const rand = mulberry32(seedFromIdentity(identity));

  const order = [...STAT_NAMES];
  // Deterministic Fisher-Yates using the same PRNG
  for (let i = order.length - 1; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    [order[i], order[j]] = [order[j], order[i]];
  }

  const peakName = order[0];
  const dumpName = order[1];
  const midNames = order.slice(2);

  const stats = {};
  stats[peakName] = 65 + Math.floor(rand() * 36); // 65-100
  stats[dumpName] = Math.floor(rand() * 31); // 0-30
  for (const name of midNames) {
    stats[name] = 31 + Math.floor(rand() * 34); // 31-64
  }

  return { stats, peakName, dumpName };
}

// ---------------------------------------------------------------------------
// Decision tree: stats -> archetype
// ---------------------------------------------------------------------------

// Language-neutral ids: these are what state.json persists. Display labels per
// language live in lib/i18n.js.
const ARCHETYPES = {
  MANDAO: 'bossy',
  RECLAMAO: 'grumbler',
  CAOTICO: 'chaotic',
  FISCAL: 'inspector',
  SABIO: 'sage',
  MOTIVADOR: 'motivator',
};

/**
 * Closed priority rule (see spec). Covers the 20 possible (peak, dump)
 * combinations across the 5 stats. Exhaustively tested in engine.test.js.
 */
function resolveArchetype(peakName, dumpName) {
  if (dumpName === 'PATIENCE' && (peakName === 'DEBUGGING' || peakName === 'CHAOS')) {
    return ARCHETYPES.MANDAO;
  }
  if (peakName === 'SNARK') return ARCHETYPES.RECLAMAO;
  if (peakName === 'CHAOS') return ARCHETYPES.CAOTICO;
  if (peakName === 'DEBUGGING') return ARCHETYPES.FISCAL;
  if (peakName === 'WISDOM' && dumpName !== 'SNARK') return ARCHETYPES.SABIO;
  if (peakName === 'PATIENCE') return ARCHETYPES.MOTIVADOR;
  return ARCHETYPES.SABIO; // fallback: peak WISDOM, dump SNARK ("acidic Sage")
}

/** Intensity = distance between peak and dump, not the peak's absolute value. */
function resolveIntensity(stats, peakName, dumpName) {
  const gap = stats[peakName] - stats[dumpName];
  if (gap <= 25) return 'mild';
  if (gap <= 50) return 'moderate';
  return 'intense';
}

/** Builds a buddy's full, deterministic identity. */
function buildBuddy(identity) {
  const { stats, peakName, dumpName } = generateStats(identity);
  const archetype = resolveArchetype(peakName, dumpName);
  const intensity = resolveIntensity(stats, peakName, dumpName);
  return { identity, stats, peakName, dumpName, archetype, intensity };
}

// ---------------------------------------------------------------------------
// Ranking (XP -> rank), 5 colors x 3 sub-levels = 15 ranks
// ---------------------------------------------------------------------------

const RANK_COLORS = ['common', 'uncommon', 'rare', 'epic', 'legendary']; // ids, see lib/i18n.js
const SUB_LEVELS = ['III', 'II', 'I']; // III is the lowest within the color

const RANK_HEX = {
  common: '#9CA3AF',
  uncommon: '#34D399',
  rare: '#60A5FA',
  epic: '#C084FC',
  legendary: '#FBBF24',
};

/**
 * Cumulative XP curve required to reach each of the 15 ranks.
 *
 * Geometric progression: base 60, ratio ~1.45. Each rank costs ~45% more than
 * the previous one; the ceiling is 24,100 cumulative XP.
 *
 * The previous curve (base 150, ratio 1.55, ceiling 125,730) was calibrated on
 * a wrong premise — "heavy session = ~70 XP". The real measurement from the
 * first duck in production gave ~700 XP/day, 10x that, because the "bug fixed"
 * trigger paid 10 XP every time the word "error" appeared in an output and the
 * next command came back clean. With the leak fixed (see
 * hooks/post-tool-use.js), the honest faucet is squeez token savings.
 *
 * CALIBRATION PREMISE: ~20 XP per active day
 *   = up to 8 XP from action (hard cap per session) + ~12 XP from savings
 *     (~300k net tokens/day at 25,000:1, see lib/squeez.js).
 *
 * The two columns below measure different things — confusing them is what
 * broke the previous calibration:
 *
 *   rank          cumulative XP   cumulative time at 20 XP/active day
 *   Common II              60      ~3 days        <- quick initial bite
 *   Uncommon I          1,100      ~2 months
 *   Rare I              3,650      ~6 months
 *   Epic I             11,400      ~1.5 years
 *   Legendary I        24,100      ~3.3 years     <- endgame, not a checkpoint
 *
 * Changing anything here reprices the whole history: cumulative XP doesn't
 * change, only the rank it buys. To recalibrate the pace prefer adjusting
 * TOKENS_PER_XP in lib/squeez.js — the rate is the calibration knob, the curve
 * is the shape.
 */
const RANK_THRESHOLDS = [
  0, 60, 150, 275, 460, 720, 1100, 1660, 2470, 3650, 5350, 7800, 11400, 16600,
  24100,
];

const RANKS = RANK_COLORS.flatMap((color) =>
  SUB_LEVELS.map((sub) => ({ color, sub, label: rankLabel({ color, sub }, 'en') }))
).map((r, i) => ({ ...r, index: i, xpRequired: RANK_THRESHOLDS[i], hex: RANK_HEX[r.color] }));

const MAX_RANK_INDEX = RANKS.length - 1;
const FLOOR_RANK_INDEX = 0; // Common III -- decay floor

function rankIndexFromXp(xp) {
  let idx = 0;
  for (let i = 0; i < RANKS.length; i++) {
    if (xp >= RANK_THRESHOLDS[i]) idx = i;
    else break;
  }
  return idx;
}

function rankFromIndex(index) {
  const clamped = Math.max(0, Math.min(MAX_RANK_INDEX, index));
  return RANKS[clamped];
}

/**
 * Art growth stage (0-4), one per rarity color. It is what gives a visible
 * reward for ranking up: the sub-level (III/II/I) only changes the label, the
 * color changes the whole silhouette. See lib/art.js STAGES.
 */
function stageFromRank(index) {
  return Math.floor(Math.max(0, Math.min(MAX_RANK_INDEX, index)) / SUB_LEVELS.length);
}

// ---------------------------------------------------------------------------
// Decay from inactivity
// ---------------------------------------------------------------------------

const GRACE_PERIOD_DAYS = 3;
const DECAY_INTERVAL_DAYS = 7;

/**
 * Computes the effective rank by applying inactivity decay.
 * - up to GRACE_PERIOD_DAYS without a session: no effect
 * - every DECAY_INTERVAL_DAYS beyond the grace: -1 sub-rank
 * - never drops below FLOOR_RANK_INDEX (Common III)
 * - returns the effective index; cumulative XP is NEVER changed by this --
 *   decay affects only the display/current rank, not the XP history.
 */
function applyDecay(xp, daysSinceLastSession) {
  const baseIndex = rankIndexFromXp(xp);
  const idleDays = Math.max(0, daysSinceLastSession - GRACE_PERIOD_DAYS);
  if (idleDays <= 0) return baseIndex;
  const steps = Math.floor(idleDays / DECAY_INTERVAL_DAYS);
  return Math.max(FLOOR_RANK_INDEX, baseIndex - steps);
}

function daysBetween(isoA, isoB) {
  const msPerDay = 1000 * 60 * 60 * 24;
  const a = new Date(isoA).getTime();
  const b = new Date(isoB).getTime();
  return Math.max(0, Math.floor((b - a) / msPerDay));
}

module.exports = {
  STAT_NAMES,
  ARCHETYPES,
  RANKS,
  RANK_COLORS,
  SUB_LEVELS,
  MAX_RANK_INDEX,
  FLOOR_RANK_INDEX,
  GRACE_PERIOD_DAYS,
  DECAY_INTERVAL_DAYS,
  fnv1a,
  mulberry32,
  seedFromIdentity,
  generateStats,
  resolveArchetype,
  resolveIntensity,
  buildBuddy,
  rankIndexFromXp,
  rankFromIndex,
  stageFromRank,
  applyDecay,
  daysBetween,
};
