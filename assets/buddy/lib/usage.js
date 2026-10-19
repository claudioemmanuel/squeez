'use strict';

// Usage windows (5h / 7d) straight from Anthropic's OAuth API — the same source
// Claude Code uses for /usage. No npm dependency: credentials come from the
// Keychain (macOS) or `~/.claude/.credentials.json`, and the response is cached
// on disk because the status line runs a fresh process on every render.

const fs = require('fs');
const path = require('path');
const os = require('os');
const https = require('https');
const { execFileSync } = require('child_process');

const KEYCHAIN_SERVICE = 'Claude Code-credentials';
const KEYCHAIN_TIMEOUT_MS = 3000;
const TTL_OK_MS = 60_000; // a good response is valid for 1 minute
const TTL_ERR_MS = 15_000; // an error holds for 15s before retrying
const BACKOFF_MS = 60_000; // credential failure: 1 min without reprompt
const API_TIMEOUT_MS = 2000; // the status line must not freeze the terminal

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
    /* the cache is an optimization; failing here must not take the status line down */
  }
}

function cacheFresh(entry, now) {
  if (!entry || typeof entry.at !== 'number') return false;
  const ttl = entry.error ? TTL_ERR_MS : TTL_OK_MS;
  return now - entry.at < ttl;
}

/** OAuth credentials: Keychain first (macOS), file as fallback. */
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
        /* try the next lookup format */
      }
    }
  }

  try {
    const raw = fs.readFileSync(path.join(configDir(), '.credentials.json'), 'utf8');
    const oauth = JSON.parse(raw).claudeAiOauth;
    if (oauth && oauth.accessToken) return oauth;
  } catch {
    /* no credential in file */
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

/** `{amount_minor, exponent}` → value in the major unit (4237/2 → 42.37), or null. */
function amount(m) {
  if (!m || typeof m.amount_minor !== 'number') return null;
  const exp = Number.isFinite(m.exponent) ? m.exponent : 2;
  return m.amount_minor / 10 ** exp;
}

/**
 * Flattens the API payload into what the HUD draws.
 *
 * On an enterprise account the call returns 200 but ALL rate limit windows
 * come back null (#198) — what bites there is the credit limit, which travels
 * in `spend` and `extra_usage`. Both already carry 0-100 percentages, so they
 * feed `pct()` directly. `enabled`/`is_enabled` false means "not a limit on
 * this account", and becomes null so the HUD doesn't draw a meter that means
 * nothing.
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
 * Current usage. Calls `cb(usage|null)` — never throws. Uses a fresh cache when
 * available, falls back to a stale cache when the network fails, and `null`
 * when there is nothing.
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
