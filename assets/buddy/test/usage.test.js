'use strict';

const test = require('node:test');
const assert = require('node:assert');

const { shape } = require('../lib/usage');

// Payload de conta enterprise (#198): a chamada devolve HTTP 200, mas TODAS as
// janelas de rate limit vêm null — quem carrega o dado real é spend/extra_usage.
const ENTERPRISE = {
  five_hour: null,
  seven_day: null,
  seven_day_oauth_apps: null,
  seven_day_opus: null,
  seven_day_sonnet: null,
  seven_day_cowork: null,
  spend: {
    used: { amount_minor: 4237, currency: 'USD', exponent: 2 },
    limit: { amount_minor: 20000, currency: 'USD', exponent: 2 },
    percent: 21,
    severity: 'ok',
    enabled: true,
  },
  extra_usage: {
    is_enabled: true,
    monthly_limit: 500,
    used_credits: 40,
    utilization: 8,
    currency: 'USD',
    decimal_places: 2,
    spend_limit_reached: false,
  },
};

const CONSUMER = {
  five_hour: { utilization: 19, resets_at: '2026-08-05T20:00:00Z' },
  seven_day: { utilization: 45, resets_at: '2026-08-07T20:00:00Z' },
};

test('shape expõe spend e extra_usage quando as janelas vêm null', () => {
  const s = shape(ENTERPRISE);
  assert.strictEqual(s.fiveHour, null);
  assert.strictEqual(s.sevenDay, null);
  assert.strictEqual(s.spend, 21);
  assert.strictEqual(s.spendUsed, 42.37, 'amount_minor / 10^exponent');
  assert.strictEqual(s.spendLimit, 200);
  assert.strictEqual(s.spendCurrency, 'USD');
  assert.strictEqual(s.extraUsage, 8);
});

test('shape preserva o caminho de plano consumidor intacto', () => {
  const s = shape(CONSUMER);
  assert.strictEqual(s.fiveHour, 19);
  assert.strictEqual(s.sevenDay, 45);
  assert.strictEqual(s.fiveHourResetsAt, '2026-08-05T20:00:00Z');
  assert.strictEqual(s.spend, null);
  assert.strictEqual(s.extraUsage, null);
});

test('shape não quebra com payload vazio, nulo ou com spend desligado', () => {
  for (const p of [null, undefined, {}, { spend: null }, { spend: { enabled: false, percent: 90 } }]) {
    const s = shape(p);
    assert.strictEqual(s.fiveHour, null);
    assert.strictEqual(s.spend, null, `spend desligado não vira medidor (${JSON.stringify(p)})`);
    assert.strictEqual(s.spendUsed, null);
  }
});

test('shape ignora extra_usage desabilitado', () => {
  const s = shape({ extra_usage: { is_enabled: false, utilization: 77 } });
  assert.strictEqual(s.extraUsage, null);
});
