import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';

const storageKey = 'glimdock-checkout-v1';
const intent = 'f4e0a806-04e2-478f-a10f-2b610a0f3e24';
const sessionId = 'cs_live_fixture1234567890ABCDEF';
const otherSession = 'cs_live_other1234567890ABCDEF';
const checkoutURL = 'https://checkout.stripe.com/c/pay/fixture';
const available = {checkoutReady: true, currency: 'EUR', unitPrice: 9900, shippingBG: 500, shippingEU: 1500};
const paid = {reference: 'GD-1234567890ABCDEF', paymentStatus: 'paid', checkoutStatus: 'complete', emailConfirmed: true, quantity: 2, amount: 21300, currency: 'eur'};
const scripts = Object.fromEntries(['order', 'complete'].map(name => [name, readFileSync(new URL('../public/' + name + '.js', import.meta.url), 'utf8')]));
const response = (data, status = 200) => ({ok: status >= 200 && status < 300, status, json: async () => data});
const flush = async () => { for (let i = 0; i < 30; i++) await Promise.resolve(); };
let generatedUUID = 0;

// Run the actual browser scripts with a small DOM, session storage and clock.
// Provider requests and navigation are observations only; no network is used.
function browser(script, {fetch, saved, storage = new Map(), href, closed = false} = {}) {
  const ids = ['order-form', 'quantity', 'country', 'body-color', 'stand-color', 'terms-agreed', 'checkout', 'checkout-status', 'checkout-retry', 'checkout-cancelled', 'plural', 'unit-math', 'subtotal', 'shipping', 'total', 'completion-title', 'completion-status', 'check-payment', 'verified-order', 'order-reference', 'order-quantity', 'order-paid'];
  const elements = Object.fromEntries(ids.map(id => [id, {
    value: id === 'quantity' ? '1' : id === 'country' ? 'BG' : id === 'body-color' ? 'Purple' : id === 'stand-color' ? 'White' : '',
    checked: false, hidden: true, disabled: false, textContent: '', dataset: id === 'order-form' && closed ? {checkoutClosed:'true'} : {},
    options: id === 'country' ? ['BG', 'DE', 'FR'].map(value => ({value})) : [],
    events: new Map(), attributes: {},
    addEventListener(name, listener) { this.events.set(name, listener); },
    setAttribute(name, value) { this.attributes[name] = value; },
    setCustomValidity(message) { this.validationMessage = message; },
    reportValidity() { return !Object.values(elements).some(element => element.validationMessage); }
  }]));
  if (saved) storage.set(storageKey, JSON.stringify(saved));
  const requests = [], navigation = [], analytics = [], tools = new Map(), events = new Map(), timers = new Map();
  let now = 0, nextTimer = 0;
  const context = {
    document: {querySelector: selector => elements[selector.slice(1)]},
    crypto: {randomUUID: () => '00000000-0000-4000-8000-' + String(++generatedUUID).padStart(12, '0')},
    sessionStorage: {getItem: key => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value), removeItem: key => storage.delete(key)},
    navigator: {modelContext: {registerTool: tool => tools.set(tool.name, tool)}},
    window: {addEventListener: (name, listener) => events.set(name, listener), glimdockAnalyticsEvent: (...args) => analytics.push(args)},
    location: {href: href || (script === 'order' ? 'https://glimdock.com/order/' : 'https://glimdock.com/order/complete/?session_id=' + sessionId), assign: url => navigation.push(url)},
    fetch: async (url, options = {}) => { requests.push({url, options}); return fetch(url, options); },
    setTimeout: (callback, delay) => { const id = ++nextTimer; timers.set(id, {callback, at: now + delay}); return id; },
    clearTimeout: id => timers.delete(id),
    AbortController, URL, Intl, Date, JSON, Set, Number, Error, TypeError
  };
  runInNewContext(scripts[script], context, {filename: script + '.js'});
  return {
    elements, requests, navigation, analytics, tools, timers, storage,
    saved: () => JSON.parse(storage.get(storageKey) || 'null'),
    submit: () => elements['order-form'].events.get('submit')({preventDefault() {}}),
    async click(id) { await elements[id].events.get('click')(); await flush(); },
    async window(name, event) { events.get(name)?.(event); await flush(); },
    async nextTimer() {
      assert(timers.size, 'Expected a scheduled timeout or status check');
      const [id, timer] = [...timers].sort((a, b) => a[1].at - b[1].at)[0];
      timers.delete(id); now = timer.at; timer.callback(); await flush();
    }
  };
}

function stalledBody(signal) {
  return {ok: true, status: 200, json: () => new Promise((resolve, reject) => {
    if (signal.aborted) reject(signal.reason);
    else signal.addEventListener('abort', () => reject(signal.reason), {once: true});
  })};
}
const cartPosts = page => page.requests.filter(request => request.url === '/api/checkout').map(request => JSON.parse(request.options.body));

test('preorder tools only configure: payment requires explicit terms and form submission', async () => {
  const page = browser('order', {fetch: async url => response(url === '/api/commerce' ? available : {url: checkoutURL, sessionId})});
  await flush();
  assert.equal(cartPosts(page).length, 0);
  await page.submit();
  await page.tools.get('configure_glimdock_preorder').execute({quantity: 2, country: 'DE'});
  const cart = await page.tools.get('read_glimdock_preorder').execute();
  assert.equal(cart.quantity, 2); assert.equal(cart.shipping, 15);
  assert.equal(page.elements['terms-agreed'].checked, false);
  await page.submit();
  assert.equal(cartPosts(page).length, 0);
  page.elements['terms-agreed'].checked = true;
  await page.submit();
  assert.equal(cartPosts(page).length, 1);
  assert.equal(cartPosts(page)[0].terms, true);
  assert.deepEqual(page.navigation, [checkoutURL]);
});

test('old per-tab cart identity survives reload and retry until an explicit selection change', async () => {
  let attempts = 0;
  const page = browser('order', {saved: {quantity: 2, country: 'DE', intent, startedAt: 1}, fetch: async url => {
    if (url === '/api/commerce') return response(available);
    return ++attempts === 1 ? response({error: 'Temporary provider outage.'}, 502) : response({url: checkoutURL, sessionId});
  }});
  await flush(); page.elements['terms-agreed'].checked = true;
  await page.submit(); await page.submit();
  assert.deepEqual(cartPosts(page).map(cart => cart.intent), [intent, intent]);
  assert.equal(page.saved().sessionId, sessionId);
  const restored = browser('order', {storage: page.storage, fetch: async () => response(available)});
  await flush();
  assert.equal(restored.elements.quantity.value, '2'); assert.equal(restored.elements.country.value, 'DE');
  assert.equal(restored.elements['body-color'].value, 'Purple'); assert.equal(restored.elements['stand-color'].value, 'White');
  restored.elements.quantity.value = '3';
  restored.elements.quantity.events.get('change')();
  assert.notEqual(restored.saved().intent, intent);
  assert.equal(restored.saved().sessionId, null);
});

test('colour requests are trimmed, sent and retained while tools never submit payment', async () => {
  const page = browser('order', {fetch: async url => response(url === '/api/commerce' ? available : {error: 'Temporary provider outage.'}, url === '/api/commerce' ? 200 : 502)});
  await flush();
  await page.tools.get('configure_glimdock_preorder').execute({quantity: 2, country: 'DE', bodyColor: '  Midnight blue  ', standColor: 'Warm orange'});
  const cart = await page.tools.get('read_glimdock_preorder').execute();
  assert.equal(cart.bodyColor, 'Midnight blue'); assert.equal(cart.standColor, 'Warm orange');
  assert.equal(cartPosts(page).length, 0);
  page.elements['terms-agreed'].checked = true;
  await page.submit();
  const posted = cartPosts(page)[0];
  assert.equal(posted.bodyColor, 'Midnight blue'); assert.equal(posted.standColor, 'Warm orange');
  assert.equal(page.saved().bodyColor, 'Midnight blue'); assert.equal(page.saved().standColor, 'Warm orange');
  const restored = browser('order', {storage: page.storage, href: 'https://glimdock.com/order/?cancelled=1', fetch: async () => response(available)});
  await flush();
  assert.equal(restored.elements['body-color'].value, 'Midnight blue');
  assert.equal(restored.elements['stand-color'].value, 'Warm orange');
  assert.equal(restored.elements['checkout-cancelled'].hidden, false);
  assert.equal(restored.saved().intent, posted.intent);
  assert.equal(restored.elements['terms-agreed'].checked, false);
  restored.elements['body-color'].value = 'Deep red';
  restored.elements['body-color'].events.get('input')();
  assert.notEqual(restored.saved().intent, posted.intent);
  assert.equal(restored.saved().bodyColor, 'Deep red');
  assert.equal(restored.saved().sessionId, null);
});

test('colour validation rejects blank, long, markup and multiline requests before checkout', async () => {
  const page = browser('order', {fetch: async url => response(url === '/api/commerce' ? available : {url: checkoutURL, sessionId})});
  await flush(); page.elements['terms-agreed'].checked = true;
  for (const invalid of ['', '   ', 'R'.repeat(65), '<b>Red</b>', 'Red\nBlue', 'Red\tBlue', 'Red\u007fBlue']) {
    page.elements['body-color'].value = invalid;
    page.elements['body-color'].events.get('input')();
    await page.submit();
    assert.equal(cartPosts(page).length, 0, JSON.stringify(invalid));
    assert.ok(page.elements['body-color'].validationMessage, JSON.stringify(invalid));
  }
  await assert.rejects(page.tools.get('configure_glimdock_preorder').execute({quantity: 1, country: 'BG', bodyColor: 'Blue', standColor: '<White>'}), /valid body\/stand/);
  assert.equal(cartPosts(page).length, 0);
  page.elements['body-color'].value = 'R'.repeat(64);
  page.elements['stand-color'].value = 'White';
  page.elements['body-color'].events.get('input')();
  const pending = page.submit();
  assert.equal(page.elements['body-color'].disabled, true);
  assert.equal(page.elements['stand-color'].disabled, true);
  await pending;
  assert.equal(cartPosts(page).length, 1);
  assert.equal(cartPosts(page)[0].bodyColor.length, 64);
});

test('browser Back restores usable controls and refreshes availability without another purchase', async () => {
  const page = browser('order', {fetch: async url => response(url === '/api/commerce' ? available : {url: checkoutURL, sessionId})});
  await flush(); page.elements['terms-agreed'].checked = true; await page.submit();
  assert.equal(page.elements.quantity.disabled, true);
  await page.window('pageshow', {persisted: true});
  assert.equal(page.elements.quantity.disabled, false);
  assert.equal(page.elements.country.disabled, false);
  assert.equal(page.elements['body-color'].disabled, false);
  assert.equal(page.elements['stand-color'].disabled, false);
  assert.equal(page.elements['terms-agreed'].disabled, false);
  assert.equal(page.elements.checkout.disabled, false);
  assert.equal(page.requests.filter(request => request.url === '/api/commerce').length, 2);
  assert.equal(cartPosts(page).length, 1);
});

test('availability timeout includes stalled JSON body and exposes a working manual retry', async () => {
  let calls = 0;
  const page = browser('order', {fetch: async (url, options) => ++calls === 1 ? stalledBody(options.signal) : response(available)});
  await flush();
  assert.equal(page.timers.size, 1);
  await page.nextTimer();
  assert.equal(page.elements['checkout-retry'].hidden, false);
  assert.equal(page.elements.checkout.disabled, true);
  await page.click('checkout-retry');
  assert.equal(page.elements.checkout.disabled, false);
  assert.equal(cartPosts(page).length, 0);
});

test('checkout timeout covers stalled response body and reuses its original intent on retry', async () => {
  let calls = 0;
  const page = browser('order', {fetch: async (url, options) => {
    if (url === '/api/commerce') return response(available);
    return ++calls === 1 ? stalledBody(options.signal) : response({url: checkoutURL, sessionId});
  }});
  await flush(); page.elements['terms-agreed'].checked = true;
  const pending = page.submit(); await flush(); await page.nextTimer(); await pending;
  assert.match(page.elements['checkout-status'].textContent, /took too long/);
  assert.equal(page.elements.checkout.disabled, false);
  await page.submit();
  const posts = cartPosts(page);
  assert.equal(posts.length, 2); assert.equal(posts[0].intent, posts[1].intent);
  assert.deepEqual(page.navigation, [checkoutURL]);
});

test('already completed checkout opens verified status without emitting a fresh checkout event', async () => {
  const page = browser('order', {fetch: async url => response(url === '/api/commerce' ? available : {completed: true, sessionId})});
  await flush(); page.elements['terms-agreed'].checked = true; await page.submit();
  assert.deepEqual(page.navigation, ['/order/complete/?session_id=' + sessionId]);
  assert.equal(page.saved().sessionId, sessionId);
  assert.equal(page.analytics.length, 0);
});

test('expired checkout rotates its intent but requires another explicit submission', async () => {
  let calls = 0;
  const page = browser('order', {saved: {quantity: 2, country: 'DE', intent, sessionId}, fetch: async url => {
    if (url === '/api/commerce') return response(available);
    return ++calls === 1 ? response({restartCheckout: true, error: 'Checkout expired.'}, 409) : response({url: checkoutURL, sessionId: otherSession});
  }});
  await flush(); page.elements['terms-agreed'].checked = true; await page.submit();
  assert.equal(cartPosts(page).length, 1); assert.equal(page.navigation.length, 0);
  assert.notEqual(page.saved().intent, intent); assert.equal(page.saved().sessionId, null);
  await page.submit();
  assert.notEqual(cartPosts(page)[1].intent, intent);
  assert.deepEqual(page.navigation, [checkoutURL]);
});

test('checkout refuses lookalike, insecure, script and credential-bearing redirects', async () => {
  for (const url of ['https://checkout.stripe.com.example.com/pay', 'http://checkout.stripe.com/pay', 'javascript:alert(1)', 'https://attacker@checkout.stripe.com/pay']) {
    const page = browser('order', {fetch: async path => response(path === '/api/commerce' ? available : {url})});
    await flush(); page.elements['terms-agreed'].checked = true; await page.submit();
    assert.equal(page.navigation.length, 0, url); assert.equal(page.analytics.length, 0, url);
    assert.equal(page.elements.checkout.disabled, false, url);
  }
});

test('paid receipt displays verified totals and clears only the matching saved checkout', async () => {
  for (const current of [sessionId, otherSession]) {
    const page = browser('complete', {saved: {quantity: 2, country: 'DE', intent, sessionId: current}, fetch: async () => response(paid)});
    await flush();
    assert.match(page.elements['completion-title'].textContent, /confirmed/);
    assert.equal(page.elements['verified-order'].hidden, false);
    assert.equal(page.elements['order-reference'].textContent, paid.reference);
    assert.equal(page.elements['order-quantity'].textContent, '2 monitors');
    assert.match(page.elements['order-paid'].textContent, /213/);
    assert.equal(page.saved() === null, current === sessionId);
    assert.equal(page.timers.size, 0);
    assert.equal(page.elements['check-payment'].hidden, true);
    assert.equal(cartPosts(page).length, 0);
  }
});

test('expired receipt stops polling and clears only its own checkout', async () => {
  for (const current of [sessionId, otherSession]) {
    const page = browser('complete', {saved: {intent, sessionId: current}, fetch: async () => response({...paid, paymentStatus: 'unpaid', checkoutStatus: 'expired', emailConfirmed: false})});
    await flush();
    assert.match(page.elements['completion-title'].textContent, /expired/);
    assert.equal(page.elements['check-payment'].hidden, true);
    assert.equal(page.timers.size, 0);
    assert.equal(page.saved() === null, current === sessionId);
  }
});

test('unconfirmed status uses bounded polling and permits a new manual check', async () => {
  const page = browser('complete', {fetch: async () => response({...paid, paymentStatus: 'unpaid', checkoutStatus: 'complete', emailConfirmed: false})});
  await flush();
  for (let i = 0; i < 5; i++) await page.nextTimer();
  assert.equal(page.requests.length, 6); assert.equal(page.timers.size, 0);
  assert.match(page.elements['completion-status'].textContent, /Check again or contact support/);
  assert.equal(page.elements['check-payment'].disabled, false);
  await page.click('check-payment');
  assert.equal(page.requests.length, 7); assert.equal(page.timers.size, 1);
  await page.window('pagehide', {}); assert.equal(page.timers.size, 0);
  assert.equal(cartPosts(page).length, 0);
});

test('status failures remain manually retryable without deleting a cart or claiming payment', async () => {
  let calls = 0;
  const page = browser('complete', {saved: {intent, sessionId}, fetch: async () => ++calls === 1 ? response({}, 503) : response(paid)});
  await flush();
  assert.match(page.elements['completion-title'].textContent, /couldn’t verify/);
  assert.equal(page.saved().sessionId, sessionId); assert.equal(page.timers.size, 0);
  assert.equal(page.elements['check-payment'].disabled, false);
  await page.click('check-payment');
  assert.match(page.elements['completion-title'].textContent, /confirmed/);
  assert.equal(page.saved(), null);
});

test('a later email-status outage does not erase an already verified payment', async () => {
  let calls = 0;
  const page = browser('complete', {fetch: async () => ++calls === 1 ? response({...paid, emailConfirmed: false}) : response({}, 503)});
  await flush(); await page.nextTimer();
  assert.match(page.elements['completion-title'].textContent, /confirmed/);
  assert.match(page.elements['completion-status'].textContent, /Payment is verified/);
  assert.equal(page.elements['verified-order'].hidden, false);
  assert.equal(page.elements['check-payment'].disabled, false);
});

test('invalid receipt references never request verification or initiate checkout', async () => {
  const page = browser('complete', {href: 'https://glimdock.com/order/complete/?session_id=untrusted', fetch: () => { throw Error('Unexpected request'); }});
  await flush();
  assert.equal(page.requests.length, 0); assert.equal(page.timers.size, 0);
  assert.match(page.elements['completion-title'].textContent, /No checkout/);
  assert.equal(page.elements['check-payment'].hidden, true);
});


test('the public closed checkout cannot reopen or submit through the form or tools', async () => {
  const page=browser('order',{closed:true,fetch:async()=>{throw Error('Closed checkout must not request availability');}});
  await flush();page.elements['terms-agreed'].checked=true;await page.submit();
  assert.equal(page.requests.length,0);assert.equal(page.elements.checkout.disabled,true);
  assert.match(page.elements['checkout-status'].textContent,/Paid checkout is closed/);
  const info=await page.tools.get('read_glimdock_preorder').execute();assert.equal(info.checkoutReady,false);
  await assert.rejects(page.tools.get('configure_glimdock_preorder').execute({quantity:1,country:'BG'}));
});
