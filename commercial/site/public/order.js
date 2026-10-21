(() => {
  const form = document.querySelector('#order-form');
  if (!form) return;
  const checkoutClosed = form.dataset?.checkoutClosed === 'true';
  const quantity = document.querySelector('#quantity');
  const country = document.querySelector('#country');
  const bodyColor = document.querySelector('#body-color');
  const standColor = document.querySelector('#stand-color');
  const terms = document.querySelector('#terms-agreed');
  const button = document.querySelector('#checkout');
  const status = document.querySelector('#checkout-status');
  const retry = document.querySelector('#checkout-retry');
  const storageKey = 'glimdock-checkout-v1';
  const countries = new Set([...country.options].map(option => option.value));
  const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;
  let config = null;
  let busy = false;
  let intent = crypto.randomUUID();
  let sessionId = null;
  const money = value => new Intl.NumberFormat('en', {style: 'currency', currency: 'EUR'}).format(value / 100);
  const validSelection = (q, c) => Number.isInteger(q) && q >= 1 && q <= 5 && countries.has(c);
  const validColor = value => typeof value === 'string' && value.trim().length > 0 && [...value.trim()].length <= 64 && !/[<>\u0000-\u001f\u007f]/u.test(value);
  function validateColors() {
    for (const [input, label] of [[bodyColor, 'body'], [standColor, 'stand']]) input.setCustomValidity(validColor(input.value) ? '' : 'Enter a ' + label + ' colour request using 1–64 characters of plain text on one line.');
    return validColor(bodyColor.value) && validColor(standColor.value);
  }
  function remember() {
    try { sessionStorage.setItem(storageKey, JSON.stringify({quantity: Number(quantity.value), country: country.value, bodyColor: bodyColor.value.trim(), standColor: standColor.value.trim(), intent, sessionId})); } catch {}
  }
  try {
    const saved = JSON.parse(sessionStorage.getItem(storageKey));
    const savedBodyColor = saved?.bodyColor ?? 'Purple';
    const savedStandColor = saved?.standColor ?? 'White';
    if (saved && validSelection(saved.quantity, saved.country) && validColor(savedBodyColor) && validColor(savedStandColor) && uuid.test(saved.intent)) {
      quantity.value = String(saved.quantity);
      country.value = saved.country;
      bodyColor.value = savedBodyColor.trim();
      standColor.value = savedStandColor.trim();
      intent = saved.intent;
      sessionId = /^cs_(test_|live_)?[A-Za-z0-9]+$/.test(saved.sessionId || '') ? saved.sessionId : null;
    }
  } catch {}
  function totals() {
    return {subtotal: Number(quantity.value) * (config?.unitPrice || 9900), shipping: country.value === 'BG' ? (config?.shippingBG ?? 500) : (config?.shippingEU ?? 1500)};
  }
  function update() {
    const count = Number(quantity.value);
    const {subtotal, shipping} = totals();
    document.querySelector('#plural').textContent = count > 1 ? 's' : '';
    document.querySelector('#unit-math').textContent = count + ' × ' + money(config?.unitPrice || 9900);
    document.querySelector('#subtotal').textContent = money(subtotal);
    document.querySelector('#shipping').textContent = money(shipping);
    document.querySelector('#total').textContent = money(subtotal + shipping);
    button.disabled = checkoutClosed || busy || !config?.checkoutReady;
    button.textContent = checkoutClosed ? 'Paid checkout is closed' : busy ? 'Opening secure checkout…' : config?.checkoutReady ? 'Continue to Stripe · ' + money(subtotal + shipping) : config ? 'Preorders opening soon' : 'Checking checkout…';
    quantity.disabled = country.disabled = bodyColor.disabled = standColor.disabled = terms.disabled = busy || checkoutClosed;
    validateColors();
    form.setAttribute('aria-busy', String(busy));
  }
  async function request(url, options = {}) {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 15000);
    try {
      const response = await fetch(url, {...options, signal: controller.signal});
      return {response, data: await response.json()};
    }
    finally { clearTimeout(timeout); }
  }
  async function loadConfig() {
    if (checkoutClosed) { status.textContent = 'Paid checkout is closed. Register unpaid interest to hear about future availability.'; retry.hidden = true; update(); return; }
    retry.hidden = true;
    status.textContent = 'Checking secure checkout availability…';
    try {
      const {response, data} = await request('/api/commerce', {cache: 'no-store'});
      if (!response.ok) throw Error();
      if (typeof data.checkoutReady !== 'boolean' || data.currency !== 'EUR' || data.unitPrice !== 9900 || data.shippingBG !== 500 || data.shippingEU !== 1500) throw Error();
      config = data;
      status.textContent = data.checkoutReady ? 'Enter your delivery address and pay securely on Stripe. Your total includes shipping.' : 'Preorders are not open yet. You haven’t placed an order and won’t be charged.';
    } catch {
      config = null;
      status.textContent = 'We couldn’t check checkout availability. Your selections are saved in this tab. Please try again.';
      retry.hidden = false;
    }
    update();
  }
  function change() { intent = crypto.randomUUID(); sessionId = null; remember(); update(); }
  quantity.addEventListener('change', change);
  country.addEventListener('change', change);
  bodyColor.addEventListener('input', change);
  standColor.addEventListener('input', change);
  retry.addEventListener('click', loadConfig);
  if (new URL(location.href).searchParams.get('cancelled') === '1') document.querySelector('#checkout-cancelled').hidden = false;
  update();
  loadConfig();
  form.addEventListener('submit', async event => {
    event.preventDefault();
    if (checkoutClosed || busy || !config?.checkoutReady) return;
    const colorsValid = validateColors();
    if (!form.reportValidity() || !terms.checked || !colorsValid || !validSelection(Number(quantity.value), country.value)) return;
    bodyColor.value = bodyColor.value.trim();
    standColor.value = standColor.value.trim();
    remember();
    busy = true;
    update();
    status.textContent = 'Opening your secure Stripe checkout…';
    try {
      const {response, data} = await request('/api/checkout', {method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({quantity: Number(quantity.value), country: country.value, bodyColor: bodyColor.value, standColor: standColor.value, terms: terms.checked, intent})});
      if (!response.ok) {
        if (data.restartCheckout === true) { intent = crypto.randomUUID(); sessionId = null; remember(); }
        if (response.status === 503) { config.checkoutReady = false; retry.hidden = false; }
        throw Error(typeof data.error === 'string' ? data.error : 'Could not open checkout. Please try again.');
      }
      if (/^cs_(test_|live_)?[A-Za-z0-9]+$/.test(data.sessionId || '')) { sessionId = data.sessionId; remember(); }
      if (data.completed === true && /^cs_(test_|live_)?[A-Za-z0-9]+$/.test(data.sessionId || '')) {
        location.assign('/order/complete/?session_id=' + encodeURIComponent(data.sessionId));
        return;
      }
      const url = new URL(data.url);
      if (url.origin !== 'https://checkout.stripe.com' || url.username || url.password) throw Error('Checkout could not be verified. Please contact support.');
      const {subtotal, shipping} = totals();
      try { window.glimdockAnalyticsEvent?.('begin_checkout', {currency: 'EUR', value: (subtotal + shipping) / 100, items: [{item_id: 'glimdock-desk-first-batch', item_name: 'Glimdock desk monitor', price: 99, quantity: Number(quantity.value)}]}); } catch {}
      location.assign(url.href);
    } catch (error) {
      status.textContent = error.name === 'AbortError' ? 'Checkout took too long to respond. Retry with the same selections; your checkout request will be reused.' : error instanceof TypeError ? 'Connection interrupted. Your selections are saved. Check your connection and try again.' : error.message;
      busy = false;
      update();
    }
  });
  window.addEventListener('pageshow', event => {
    if (event.persisted) { busy = false; update(); loadConfig(); }
  });
  if (navigator.modelContext) {
    try {
      navigator.modelContext.registerTool({name: 'read_glimdock_preorder', description: 'Read the visible preorder choices and checkout availability. Does not place an order.', inputSchema: {type: 'object', properties: {}}, execute: async () => ({quantity: Number(quantity.value), country: country.value, bodyColor: bodyColor.value.trim(), standColor: standColor.value.trim(), unitPrice: 99, currency: 'EUR', shipping: totals().shipping / 100, checkoutReady: !checkoutClosed && !!config?.checkoutReady})});
      navigator.modelContext.registerTool({name: 'configure_glimdock_preorder', description: 'Set quantity, EU destination and body/stand colour requests without submitting or paying.', inputSchema: {type: 'object', properties: {quantity: {type: 'integer', minimum: 1, maximum: 5}, country: {type: 'string'}, bodyColor: {type: 'string', minLength: 1, maxLength: 64}, standColor: {type: 'string', minLength: 1, maxLength: 64}}, required: ['quantity', 'country']}, execute: async value => {
        const requestedBody = value.bodyColor ?? bodyColor.value;
        const requestedStand = value.standColor ?? standColor.value;
        if (checkoutClosed || busy || !validSelection(value.quantity, value.country) || !validColor(requestedBody) || !validColor(requestedStand)) throw Error('Choose 1–5 monitors, an offered EU country and valid body/stand colour requests of 1–64 characters.');
        quantity.value = String(value.quantity); country.value = value.country; bodyColor.value = requestedBody.trim(); standColor.value = requestedStand.trim(); change(); return {configured: true};
      }});
    } catch {}
  }
})();
