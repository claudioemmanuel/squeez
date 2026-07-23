'use strict';

// Fonte de XP por economia do squeez: lê <squeez data dir>/sessions/current.json
// e converte economia líquida (tokens_saved - overhead_tokens) em XP.
// 100k tokens líquidos = 1 XP (progressão lenta de propósito — rank máximo
// exige 650M tokens economizados). Baseline por start_ts: sessão nova zera a
// régua; o resto sub-taxa fica acumulado em lifetimeNetSaved (nada se perde).
// Mudança de taxa re-baseia o XP já concedido (tracker.rate registra a taxa
// vigente; o delta da recomputação pode ser negativo).

const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

const TOKENS_PER_XP = 100000;

function squeezDataDir() {
  return process.env.SQUEEZ_DIR || path.join(os.homedir(), '.claude', 'squeez');
}

function readCurrentSession() {
  try {
    const raw = fs.readFileSync(
      path.join(squeezDataDir(), 'sessions', 'current.json'),
      'utf8'
    );
    const j = JSON.parse(raw);
    if (!j || typeof j !== 'object') return null;
    return {
      startTs: Number(j.start_ts) || 0,
      tokensSaved: Number(j.tokens_saved) || 0,
      overheadTokens: Number(j.overhead_tokens) || 0,
    };
  } catch {
    return null;
  }
}

/**
 * Muta state.squeez (tracker) e devolve o XP a conceder agora. Normalmente
 * inteiro >= 0; negativo apenas na re-base após uma mudança de taxa (o
 * chamador deve aplicar via addXp mesmo assim). Idempotente entre chamadas
 * sem economia nova (baseline monotônico), então é seguro chamar tanto do
 * PostToolUse quanto do Stop.
 */
function applySqueezSavings(state) {
  const cur = readCurrentSession();
  if (!cur) return 0;

  const tracker =
    state.squeez && typeof state.squeez === 'object'
      ? state.squeez
      : { lastStartTs: 0, lastNetSaved: 0, lifetimeNetSaved: 0, xpGranted: 0, rate: TOKENS_PER_XP };
  state.squeez = tracker;

  // Re-base após mudança de taxa: recomputa o XP total que o lifetime vale na
  // taxa nova e devolve a diferença contra o já concedido (pode ser negativa).
  let rebase = 0;
  if ((Number(tracker.rate) || 0) !== TOKENS_PER_XP) {
    const worth = Math.floor((Number(tracker.lifetimeNetSaved) || 0) / TOKENS_PER_XP);
    rebase = worth - (Number(tracker.xpGranted) || 0);
    tracker.xpGranted = worth;
    tracker.rate = TOKENS_PER_XP;
  }

  const net = Math.max(0, cur.tokensSaved - cur.overheadTokens);
  if (cur.startTs !== tracker.lastStartTs) {
    tracker.lastStartTs = cur.startTs;
    tracker.lastNetSaved = 0;
  }
  const delta = Math.max(0, net - (Number(tracker.lastNetSaved) || 0));
  tracker.lastNetSaved = Math.max(Number(tracker.lastNetSaved) || 0, net);
  tracker.lifetimeNetSaved = (Number(tracker.lifetimeNetSaved) || 0) + delta;

  const totalXp = Math.floor(tracker.lifetimeNetSaved / TOKENS_PER_XP);
  const granted = Number(tracker.xpGranted) || 0;
  const gain = Math.max(0, totalXp - granted);
  tracker.xpGranted = granted + gain;
  return rebase + gain;
}

module.exports = { applySqueezSavings, TOKENS_PER_XP };
