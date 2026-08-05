'use strict';

// Janelas de uso (5h / 7d) direto da API OAuth da Anthropic — mesma fonte que o
// Claude Code usa no /usage. Sem dependência npm: credenciais vêm do Keychain
// (macOS) ou de `~/.claude/.credentials.json`, e a resposta é cacheada em disco
// porque a status line roda um processo novo a cada render.

const fs = require('fs');
const path = require('path');
const os = require('os');
const https = require('https');
const { execFileSync } = require('child_process');

const KEYCHAIN_SERVICE = 'Claude Code-credentials';
const KEYCHAIN_TIMEOUT_MS = 3000;
const TTL_OK_MS = 60_000; // resposta boa vale 1 minuto
const TTL_ERR_MS = 15_000; // erro segura 15s antes de tentar de novo
const BACKOFF_MS = 60_000; // falha de credencial: 1 min sem reprompt
const API_TIMEOUT_MS = 2000; // status line não pode travar o terminal

function configDir() {
  const env = (process.env.CLAUDE_CONFIG_DIR || '').trim();
  return env || path.join(os.homedir(), '.claude');
}

function cacheFile() {
  return path.join(configDir(), 'squeez', 'buddy', 'usage-cache.json');
}

function readCache() {
  try {
    return JSON.parse(fs.readFileSync(cacheFile(), 'utf8'));
  } catch {
    return null;
  }
}

function writeCache(entry) {
  try {
    fs.mkdirSync(path.dirname(cacheFile()), { recursive: true });
    fs.writeFileSync(cacheFile(), JSON.stringify(entry));
  } catch {
    /* cache é otimização; falhar aqui não pode derrubar a status line */
  }
}

function cacheFresh(entry, now) {
  if (!entry || typeof entry.at !== 'number') return false;
  const ttl = entry.error ? TTL_ERR_MS : TTL_OK_MS;
  return now - entry.at < ttl;
}

/** Credenciais OAuth: Keychain primeiro (macOS), arquivo como fallback. */
function readCredentials(now) {
  const cached = readCache();
  if (cached && cached.credBackoffUntil && now < cached.credBackoffUntil) return null;

  if (process.platform === 'darwin') {
    let account = null;
    try {
      account = os.userInfo().username.trim() || null;
    } catch {
      account = null;
    }
    const attempts = account
      ? [['-s', KEYCHAIN_SERVICE, '-a', account, '-w'], ['-s', KEYCHAIN_SERVICE, '-w']]
      : [['-s', KEYCHAIN_SERVICE, '-w']];
    for (const args of attempts) {
      try {
        const raw = execFileSync('/usr/bin/security', ['find-generic-password', ...args], {
          encoding: 'utf8',
          stdio: ['pipe', 'pipe', 'pipe'],
          timeout: KEYCHAIN_TIMEOUT_MS,
        });
        const oauth = JSON.parse(raw).claudeAiOauth;
        if (oauth && oauth.accessToken) return oauth;
      } catch {
        /* tenta o próximo formato de lookup */
      }
    }
  }

  try {
    const raw = fs.readFileSync(path.join(configDir(), '.credentials.json'), 'utf8');
    const oauth = JSON.parse(raw).claudeAiOauth;
    if (oauth && oauth.accessToken) return oauth;
  } catch {
    /* sem credencial em arquivo */
  }

  writeCache({ ...(cached || {}), credBackoffUntil: now + BACKOFF_MS });
  return null;
}

function fetchUsage(accessToken, cb) {
  let done = false;
  const finish = (err, data) => {
    if (done) return;
    done = true;
    cb(err, data);
  };

  const req = https.request(
    {
      hostname: 'api.anthropic.com',
      path: '/api/oauth/usage',
      method: 'GET',
      timeout: API_TIMEOUT_MS,
      headers: {
        Authorization: `Bearer ${accessToken}`,
        'anthropic-beta': 'oauth-2025-04-20',
        Accept: 'application/json',
      },
    },
    (res) => {
      let body = '';
      res.setEncoding('utf8');
      res.on('data', (chunk) => {
        body += chunk;
      });
      res.on('end', () => {
        if (res.statusCode !== 200) return finish(new Error(`HTTP ${res.statusCode}`));
        try {
          finish(null, JSON.parse(body));
        } catch (e) {
          finish(e);
        }
      });
    }
  );

  req.on('timeout', () => {
    req.destroy();
    finish(new Error('timeout'));
  });
  req.on('error', (e) => finish(e));
  req.end();
}

function pct(value) {
  if (typeof value !== 'number' || !Number.isFinite(value)) return null;
  return Math.round(Math.max(0, Math.min(100, value)));
}

/** `{amount_minor, exponent}` → valor na unidade maior (4237/2 → 42.37), ou null. */
function amount(m) {
  if (!m || typeof m.amount_minor !== 'number') return null;
  const exp = Number.isFinite(m.exponent) ? m.exponent : 2;
  return m.amount_minor / 10 ** exp;
}

/**
 * Achata o payload da API no que o HUD desenha.
 *
 * Em conta enterprise a chamada devolve 200 mas TODAS as janelas de rate limit
 * vêm null (#198) — o que morde lá é o limite de crédito, que viaja em `spend`
 * e `extra_usage`. Ambos já trazem percentuais 0-100, então alimentam `pct()`
 * direto. `enabled`/`is_enabled` falso significa "não é limite desta conta", e
 * vira null para o HUD não desenhar um medidor que não quer dizer nada.
 */
function shape(payload) {
  const p = payload || {};
  const sp = p.spend && p.spend.enabled !== false ? p.spend : null;
  const extra = p.extra_usage && p.extra_usage.is_enabled ? p.extra_usage : null;
  return {
    fiveHour: pct(p.five_hour && p.five_hour.utilization),
    sevenDay: pct(p.seven_day && p.seven_day.utilization),
    fiveHourResetsAt: (p.five_hour && p.five_hour.resets_at) || null,
    sevenDayResetsAt: (p.seven_day && p.seven_day.resets_at) || null,
    spend: pct(sp && sp.percent),
    spendUsed: amount(sp && sp.used),
    spendLimit: amount(sp && sp.limit),
    spendCurrency: (sp && sp.used && sp.used.currency) || (sp && sp.currency) || null,
    extraUsage: pct(extra && extra.utilization),
  };
}

/**
 * Uso atual. Chama `cb(usage|null)` — nunca lança. Usa cache fresco quando há,
 * cai pro cache velho quando a rede falha, e `null` quando não há nada.
 */
function getUsage(cb) {
  const now = Date.now();
  const cached = readCache();
  if (cacheFresh(cached, now) && cached.data) return cb(cached.data);

  const creds = readCredentials(now);
  if (!creds) return cb(cached && cached.data ? cached.data : null);

  fetchUsage(creds.accessToken, (err, payload) => {
    if (err) {
      writeCache({ ...(cached || {}), at: now, error: String(err.message || err) });
      return cb(cached && cached.data ? cached.data : null);
    }
    const data = shape(payload);
    writeCache({ at: now, data });
    cb(data);
  });
}

module.exports = { getUsage, shape, pct, cacheFile };
