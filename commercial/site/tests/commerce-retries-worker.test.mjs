import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import worker, {runCommerceRetries} from '../scripts/commerce-retries-worker.js';

const token = 'fixture-operations-token-32-characters';
const clean = {confirmations: {sent: 2, pending: 0, due: 0}, withdrawals: {sent: 1, pending: 0, due: 0}, interest: {sent: 0, pending: 0, due: 0}};
const json = value => new Response(JSON.stringify(value), {headers: {'Content-Type': 'application/json'}});

async function mocked(fetch, run) {
  const originalFetch = globalThis.fetch;
  const originalConsole = {log: console.log, warn: console.warn, error: console.error};
  const logs = [];
  globalThis.fetch = fetch;
  for (const level of ['log', 'warn', 'error']) console[level] = (...args) => logs.push({level, args});
  try { await run(logs); }
  finally { globalThis.fetch = originalFetch; Object.assign(console, originalConsole); }
}

test('retry scheduler uses its fixed authenticated endpoint and logs only aggregate counts', async () => {
  let calls = 0;
  await mocked(async (url, options) => {
    calls++;
    assert.equal(url, 'https://glimdock.com/api/admin/retry-commerce');
    assert.equal(options.method, 'POST'); assert.equal(options.redirect, 'manual');
    assert.equal(options.headers.Authorization, 'Bearer ' + token);
    assert.equal(options.headers['User-Agent'], 'GlimdockCommerceOperations/1.0');
    assert(options.signal instanceof AbortSignal);
    return json({...clean, customer: 'private@example.com', token, confirmations: {...clean.confirmations, email: 'private@example.com'}});
  }, async logs => {
    assert.deepEqual(await runCommerceRetries({OPERATIONS_TOKEN: token, ENDPOINT: 'https://attacker.example'}), clean);
    assert.equal(calls, 1); assert.equal(logs[0].level, 'log');
    assert.deepEqual(logs[0].args[1], clean);
    const output = JSON.stringify(logs); assert(!output.includes(token)); assert(!output.includes('private@example.com'));
  });
});

test('pending mail is an aggregate warning, including notices waiting for a later backoff', async () => {
  for (const due of [0, 2]) {
    const pending = {confirmations: {sent: 1, pending: 3, due}, withdrawals: {sent: 0, pending: 1, due: 0}, interest: {sent: 0, pending: 0, due: 0}};
    await mocked(async () => json(pending), async logs => {
      assert.deepEqual(await runCommerceRetries({OPERATIONS_TOKEN: token}), pending);
      assert.equal(logs.length, 1); assert.equal(logs[0].level, 'warn');
    });
  }
});

test('a missing interest queue remains compatible with an older storefront during rollout', async () => {
  const {interest, ...legacy} = clean;
  await mocked(async () => json(legacy), async logs => {
    assert.deepEqual(await runCommerceRetries({OPERATIONS_TOKEN: token}), clean);
    assert.deepEqual(logs[0].args[1].interest, {sent: 0, pending: 0, due: 0});
  });
});

test('interest backlog alone warns with aggregate counts and drops subscriber details', async () => {
  const interest = {sent: 1, pending: 3, due: 2};
  await mocked(async () => json({...clean, interest: {...interest, email: 'private@example.com', token}}), async logs => {
    assert.deepEqual(await runCommerceRetries({OPERATIONS_TOKEN: token}), {...clean, interest});
    assert.equal(logs[0].level, 'warn');
    const output = JSON.stringify(logs); assert(!output.includes('private@example.com')); assert(!output.includes(token));
  });
});

test('a present malformed interest queue fails rather than concealing an operations error', async () => {
  for (const interest of [null, [], {}, {sent: 0, pending: 1, due: 2}, {sent: 0, pending: '1', due: 0}]) {
    await mocked(async () => json({...clean, interest}), async logs => {
      await assert.rejects(runCommerceRetries({OPERATIONS_TOKEN: token}), /invalid_response/);
      assert.equal(logs[0].args[1].stage, 'counts');
      assert.equal(logs[0].level, 'error');
    });
  }
});

test('missing, short or whitespace-bearing operations token fails before any request', async () => {
  await mocked(() => { throw Error('Provider must not be contacted'); }, async logs => {
    for (const value of [undefined, '', 'short', ' '.repeat(32), token + '\r\nprivate']) {
      await assert.rejects(runCommerceRetries({OPERATIONS_TOKEN: value}), /invalid_configuration/);
    }
    assert(logs.every(log => log.level === 'error'));
    assert(!JSON.stringify(logs).includes(token));
  });
});

test('HTTP failures and redirects fail the scheduled run without logging response content', async () => {
  for (const status of [301, 401, 403, 429, 500, 503]) {
    await mocked(async () => new Response('secret ' + token, {status}), async logs => {
      await assert.rejects(worker.scheduled({}, {OPERATIONS_TOKEN: token}), new RegExp('http_' + status));
      assert.equal(logs[0].args[1].reason, 'http_' + status);
      assert(!JSON.stringify(logs).includes(token));
    });
  }
});

test('invalid JSON and malformed aggregate counts cannot be mistaken for a successful retry', async () => {
  const invalid = [null, {}, {confirmations: clean.confirmations}, {...clean, withdrawals: []}, {...clean, confirmations: {...clean.confirmations, pending: '0'}}, {...clean, confirmations: {...clean.confirmations, sent: -1}}, {...clean, confirmations: {...clean.confirmations, due: 1}}, {...clean, confirmations: {...clean.confirmations, sent: 1.5}}];
  for (const body of invalid) {
    await mocked(async () => json(body), async logs => {
      await assert.rejects(runCommerceRetries({OPERATIONS_TOKEN: token}), /invalid_response/);
      assert.equal(logs[0].level, 'error');
    });
  }
  await mocked(async () => new Response('not JSON: ' + token), async logs => {
    await assert.rejects(runCommerceRetries({OPERATIONS_TOKEN: token}), /invalid_response/);
    assert.equal(logs[0].args[1].stage, 'json');
    assert(!JSON.stringify(logs).includes(token));
  });
});

test('request timeout covers a stalled response body and never logs a raw network error', async t => {
  t.mock.timers.enable({apis: ['setTimeout']});
  await mocked(async (_url, options) => ({ok: true, json: () => new Promise((resolve, reject) => {
    options.signal.addEventListener('abort', () => reject(Error('secret ' + token)), {once: true});
  })}), async logs => {
    const pending = runCommerceRetries({OPERATIONS_TOKEN: token});
    const rejected = assert.rejects(pending, /timeout/);
    await Promise.resolve();
    t.mock.timers.tick(19999); assert.equal(logs.length, 0);
    t.mock.timers.tick(1); await rejected;
    assert.equal(logs[0].args[1].reason, 'timeout');
    assert.equal(logs[0].args[1].stage, 'json');
    assert(!JSON.stringify(logs).includes(token));
  });
});

test('network exceptions are reduced to a fixed safe error code', async () => {
  await mocked(async () => { throw Error('Failed Bearer ' + token + ' buyer@example.com'); }, async logs => {
    await assert.rejects(runCommerceRetries({OPERATIONS_TOKEN: token}), /^Error: Commerce retry failed: request_failed$/);
    assert.deepEqual(logs[0].args[1], {reason: 'request_failed', stage: 'fetch', errorType: 'Error'});
    assert(!JSON.stringify(logs).includes(token)); assert(!JSON.stringify(logs).includes('buyer@example.com'));
  });
});

test('HTTP200 HTML is identified as invalid response JSON without leaking its content', async () => {
  await mocked(async () => new Response('<html>private ' + token + '</html>', {headers: {'Content-Type': 'text/html; charset=utf-8'}}), async logs => {
    await assert.rejects(runCommerceRetries({OPERATIONS_TOKEN: token}), /invalid_response/);
    assert.deepEqual(logs[0].args[1], {reason: 'invalid_response', stage: 'json', errorType: 'SyntaxError', httpStatus: 200, responseType: 'html'});
    assert(!JSON.stringify(logs).includes(token));
  });
});

test('fetch diagnostics classify only allowlisted failures and never expose raw exceptions', async () => {
  for (const [message, reason] of [
    ['error1042 Worker-to-Worker fetch blocked', 'worker_subrequest_blocked'],
    ['error1014 Cross-account CNAME forbidden', 'cross_account_blocked'],
    ['Redirect encountered', 'redirect_blocked'],
    ['DNS resolution failed', 'dns_failure'],
    ['TLS certificate failed', 'tls_failure'],
    ['Fetch failed', 'request_failed']
  ]) {
    await mocked(async () => { throw new TypeError(message + ' ' + token + ' private@example.com'); }, async logs => {
      await assert.rejects(runCommerceRetries({OPERATIONS_TOKEN: token}), new RegExp(reason));
      assert.deepEqual(logs[0].args[1], {reason, stage: 'fetch', errorType: 'TypeError'});
      assert(!JSON.stringify(logs).includes(token)); assert(!JSON.stringify(logs).includes('private@example.com'));
    });
  }
});

test('public requests return404 and never start retries; configuration has only the scheduled trigger', async () => {
  await mocked(() => { throw Error('Public trigger must not fetch'); }, async logs => {
    for (const method of ['GET', 'POST']) {
      const response = await worker.fetch(new Request('https://example.com/retry', {method}), {OPERATIONS_TOKEN: token});
      assert.equal(response.status, 404);
      assert.equal(response.headers.get('Cache-Control'), 'no-store');
    }
    assert.equal(logs.length, 0);
  });
  const config = JSON.parse(readFileSync(new URL('../scripts/commerce-retries.wrangler.jsonc', import.meta.url), 'utf8'));
  assert.equal(config.name, 'glimdock-commerce-retries');
  if (config.account_id !== undefined) assert.match(config.account_id, /^[a-f0-9]{32}$/);
  // The public storefront is a Sites Worker behind a zone owned by this account.
  // Private-origin routing would bypass that Worker, even with a recent date.
  assert.deepEqual(config.compatibility_flags, ['global_fetch_strictly_public']);
  assert.deepEqual(config.triggers.crons, ['*/5 * * * *']);
  assert.equal(config.workers_dev, false); assert.equal(config.preview_urls, false);
  assert.equal(config.vars, undefined); assert.equal(config.routes, undefined);
  assert.equal(config.observability.enabled, true);
});
