(() => {
  const form = document.querySelector('#interest-form');
  if (!form) return;
  const email = document.querySelector('#interest-email');
  const quantity = document.querySelector('#interest-quantity');
  const country = document.querySelector('#interest-country');
  const bodyColor = document.querySelector('#interest-body');
  const standColor = document.querySelector('#interest-stand');
  const consent = document.querySelector('#interest-consent');
  const website = document.querySelector('#interest-website');
  const button = document.querySelector('#interest-submit');
  const status = document.querySelector('#interest-status');
  const retry = document.querySelector('#interest-retry');
  const countries = new Set([...country.options].map(option => option.value));
  let enabled = false, busy = false, submitted = false, operation = 0;
  const validColor = value => typeof value === 'string' && [...value.trim()].length <= 64 && !/[<>\u0000-\u001f\u007f]/u.test(value);
  function update() {
    for (const input of [email, quantity, country, bodyColor, standColor, consent, website]) input.disabled = busy || submitted;
    button.disabled = busy || submitted || !enabled;
    button.textContent = busy ? 'Sending confirmation…' : submitted ? 'Check your email' : 'Register interest';
    form.setAttribute('aria-busy', String(busy));
  }
  function validate() {
    email.setCustomValidity(/^[^\s@<>]+@[^\s@<>]+\.[^\s@<>]+$/u.test(email.value.trim()) && email.value.trim().length <= 254 && !/[\u0000-\u001f\u007f]/u.test(email.value) ? '' : 'Enter a valid email address.');
    for (const input of [bodyColor, standColor]) input.setCustomValidity(validColor(input.value) ? '' : 'Use up to 64 characters of plain text on one line.');
    return form.reportValidity() && consent.checked && Number.isInteger(Number(quantity.value)) && Number(quantity.value) >= 1 && Number(quantity.value) <= 5 && countries.has(country.value) && validColor(bodyColor.value) && validColor(standColor.value);
  }
  async function request(url, options = {}) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 15000);
    try {
      const response = await fetch(url, {...options, signal: controller.signal});
      return {response, data: await response.json()};
    } finally { clearTimeout(timer); }
  }
  async function loadConfig() {
    enabled = false; retry.hidden = true;
    status.textContent = 'Checking interest signup availability…'; update();
    try {
      const {response, data} = await request('/api/interest-config', {cache: 'no-store'});
      if (!response.ok || typeof data.enabled !== 'boolean' || data.requiresEmailConfirmation !== true) throw Error();
      enabled = data.enabled;
      status.textContent = enabled ? 'Register unpaid interest, then confirm using the link sent by email.' : 'Interest signup is not open right now. Please check again later.';
      retry.hidden = enabled;
    } catch {
      status.textContent = 'We couldn’t check signup availability. Please try again.';
      retry.hidden = false;
    }
    update();
  }
  for (const input of [email, bodyColor, standColor]) input.addEventListener('input', () => input.setCustomValidity(''));
  retry.addEventListener('click', loadConfig);
  form.addEventListener('submit', async event => {
    event.preventDefault();
    if (!enabled || busy || submitted || !validate()) return;
    const currentOperation = ++operation;
    busy = true; retry.hidden = true; update();
    status.textContent = 'Requesting your confirmation email…';
    try {
      const {response, data} = await request('/api/interest', {method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({email: email.value.trim(), quantity: Number(quantity.value), country: country.value, bodyColor: bodyColor.value.trim(), standColor: standColor.value.trim(), consent: consent.checked, website: website.value})});
      if (currentOperation !== operation) return;
      if (response.status !== 202 || data.accepted !== true) {
        if (response.status === 429) throw Error('Please wait a little before trying again.');
        throw Error('We couldn’t process this request. Please try again later.');
      }
      submitted = true;
      email.value = ''; consent.checked = false;
      status.textContent = 'Check your email. If this address can be added, you’ll receive a confirmation link valid for seven days. Open it and choose Confirm interest to join the availability list. This is not an order or reservation.';
    } catch (error) {
      if (currentOperation !== operation) return;
      status.textContent = error.name === 'AbortError' ? 'We couldn’t confirm that your request was received. Check your email before trying again.' : error instanceof TypeError ? 'Connection interrupted. Check your email or try again when you’re connected.' : error.message;
    } finally { if (currentOperation === operation) { busy = false; update(); } }
  });
  window.addEventListener('pageshow', event => {
    if (event.persisted && busy && !submitted) {
      operation++; busy = false; update();
      status.textContent = 'The request may still be processing. Check your email before trying the form again.';
    }
  });
  loadConfig();
  if (navigator.modelContext) {
    try { navigator.modelContext.registerTool({name: 'read_glimdock_interest', description: 'Read public interest signup availability and planned price. Does not reveal email addresses, submit a signup, reserve or buy anything.', inputSchema: {type: 'object', properties: {}}, execute: async () => ({enabled, requiresEmailConfirmation: true, plannedPriceEstimate: 99, currency: 'EUR', shippingAdditional: true, createsOrder: false})}); } catch {}
  }
})();
