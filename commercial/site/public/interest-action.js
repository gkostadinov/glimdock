(() => {
  const action = location.pathname === '/interest/confirm/' ? 'confirm' : location.pathname === '/interest/unsubscribe/' ? 'unsubscribe' : null;
  let id = '', token = '', valid = false;
  function readLink() {
    const params = new URLSearchParams(location.hash.slice(1));
    id = params.get('id') || ''; token = params.get('token') || '';
    // Clear link credentials before loading other scripts or sending requests.
    let cleared = false;
    try { history.replaceState(null, '', location.pathname); cleared = !location.hash && !location.search; } catch {}
    valid = cleared && /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(id) && /^[a-f0-9]{64}$/.test(token) && action !== null;
  }
  readLink();
  document.addEventListener('DOMContentLoaded', () => {
    const card = document.querySelector('#interest-action-card');
    const button = document.querySelector('#interest-action');
    const status = document.querySelector('#interest-action-status');
    if (!button || !card || !status) return;
    let busy = false, complete = false, operation = 0;
    button.disabled = !valid;
    if (!valid) status.textContent = 'This link could not be used. It may be incomplete or expired. Use the link in your email, or register interest again.';
    button.addEventListener('click', async () => {
      if (!valid || busy || complete) return;
      const currentOperation = ++operation;
      busy = true; button.disabled = true; card.setAttribute('aria-busy', 'true');
      status.textContent = action === 'confirm' ? 'Confirming your interest…' : 'Updating your email preference…';
      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), 15000);
      try {
        const response = await fetch('/api/interest/' + action, {method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({id, token}), signal: controller.signal});
        const data = await response.json();
        if (currentOperation !== operation) return;
        if (!response.ok || (action === 'confirm' ? data.confirmed !== true : data.unsubscribed !== true)) {
          status.textContent = 'This link could not be used. It may have expired or already been processed. You can register interest again or contact support.';
          return;
        }
        complete = true;
        button.textContent = action === 'confirm' ? 'Interest confirmed' : 'Unsubscribed';
        status.textContent = action === 'confirm' ? 'Your interest is confirmed. We’ll email you about Glimdock availability. You can unsubscribe at any time. No order or reservation has been created.' : 'If this link matched an availability signup, it has been removed. You won’t receive further Glimdock availability emails for that signup.';
      } catch {
        if (currentOperation !== operation) return;
        status.textContent = 'We couldn’t confirm the result. Check your connection and try this button again.';
      } finally {
        clearTimeout(timeout); if (currentOperation === operation) { busy = false; button.disabled = complete; card.setAttribute('aria-busy', 'false'); }
      }
    });
    window.addEventListener('hashchange', () => {
      const fragment = new URLSearchParams(location.hash.slice(1));
      if (!fragment.has('id') && !fragment.has('token')) return;
      operation++; busy = false; complete = false; readLink();
      button.disabled = !valid; button.textContent = action === 'confirm' ? 'Confirm interest' : 'Unsubscribe'; card.setAttribute('aria-busy', 'false');
      status.textContent = valid ? 'Opening this page alone does not change your email preference.' : 'This link could not be used. It may be incomplete or expired. Use the link in your email, or register interest again.';
    });
    window.addEventListener('pageshow', event => {
      if (event.persisted && busy && !complete) {
        operation++; busy = false; button.disabled = !valid; card.setAttribute('aria-busy', 'false');
        status.textContent = 'The request may still be processing. Choose this button again to check or apply your preference.';
      }
    });
  }, {once: true});
})();
