(() => {
  const title = document.querySelector('#completion-title');
  const status = document.querySelector('#completion-status');
  const check = document.querySelector('#check-payment');
  const summary = document.querySelector('#verified-order');
  const id = new URL(location.href).searchParams.get('session_id');
  let timer;
  let attempts = 0;
  let busy = false;
  let paymentVerified = false;
  function clearMatchingCart() {
    try {
      const saved = JSON.parse(sessionStorage.getItem('glimdock-checkout-v1'));
      if (saved?.sessionId === id) sessionStorage.removeItem('glimdock-checkout-v1');
    } catch {}
  }
  if (!/^cs_(test_|live_)?[A-Za-z0-9]+$/.test(id || '')) {
    title.textContent = 'No checkout to verify.';
    status.textContent = 'Return to the preorder page or contact support with your order reference.';
    return;
  }
  check.hidden = false;
  async function verify() {
    if (busy) return;
    clearTimeout(timer);
    busy = true;
    check.disabled = true;
    check.textContent = 'Checking payment…';
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 15000);
    try {
      const response = await fetch('/api/order-status?session_id=' + encodeURIComponent(id), {cache: 'no-store', signal: controller.signal});
      if (!response.ok) throw Error();
      const order = await response.json();
      if (!/^GD-[A-Z0-9]{16}$/.test(order.reference || '') || !['paid', 'unpaid', 'no_payment_required'].includes(order.paymentStatus)) throw Error();
      if (order.paymentStatus === 'paid') {
        paymentVerified = true;
        title.textContent = 'Your preorder is confirmed.';
        status.textContent = 'Thank you. Order reference ' + order.reference + '. Estimated delivery: 4–6 weeks from payment. ' + (order.emailConfirmed ? 'Your order confirmation has been sent by email; keep it for your records.' : 'Payment is verified. Your email confirmation is pending. You can safely close this page; contact support if it does not arrive.');
        clearMatchingCart();
        document.querySelector('#order-reference').textContent = order.reference;
        if (Number.isInteger(order.quantity) && order.quantity >= 1 && order.quantity <= 5 && Number.isInteger(order.amount) && order.currency === 'eur') {
          document.querySelector('#order-quantity').textContent = order.quantity + (order.quantity === 1 ? ' monitor' : ' monitors');
          document.querySelector('#order-paid').textContent = new Intl.NumberFormat('en', {style: 'currency', currency: 'EUR'}).format(order.amount / 100);
          const colors = document.querySelector('#order-colors');
          if (colors && typeof order.bodyColor === 'string' && typeof order.standColor === 'string') {
            colors.textContent = 'Body: ' + order.bodyColor + ' · Stand: ' + order.standColor;
            document.querySelector('#order-colors-row').hidden = false;
          }
          summary.hidden = false;
        }
        check.hidden = !!order.emailConfirmed;
      } else if (order.checkoutStatus === 'expired') {
        title.textContent = 'This checkout has expired.';
        status.textContent = 'Payment was not completed for this checkout. Return to the preorder page to start again, or contact support if your bank shows a charge.';
        check.hidden = true;
        clearMatchingCart();
      } else {
        title.textContent = 'Payment is not confirmed yet.';
        status.textContent = 'Your payment may still be processing. We’ll check again shortly. Do not pay again while its status is unclear.';
      }
      if (!check.hidden && attempts < 5) { timer = setTimeout(verify, Math.min(2000 * 2 ** attempts++, 15000)); }
      else if (order.paymentStatus !== 'paid' && order.checkoutStatus !== 'expired') status.textContent = 'Payment is not confirmed yet. Check again or contact support before making another payment.';
    } catch {
      title.textContent = paymentVerified ? 'Your preorder is confirmed.' : 'We couldn’t verify checkout.';
      status.textContent = paymentVerified ? 'Payment is verified. We couldn’t refresh your email confirmation status. You can safely close this page; contact support if your confirmation does not arrive.' : 'Your payment status is currently unavailable. Check again or contact support before trying another payment.';
    } finally {
      clearTimeout(timeout);
      busy = false;
      check.disabled = false;
      check.textContent = 'Check status again';
    }
  }
  check.addEventListener('click', () => { attempts = 0; verify(); });
  window.addEventListener('pagehide', () => clearTimeout(timer));
  verify();
})();
