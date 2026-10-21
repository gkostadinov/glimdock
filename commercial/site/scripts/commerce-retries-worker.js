// Separate operations Worker. Keep the storefront and its database in Sites.
const ENDPOINT = 'https://glimdock.com/api/admin/retry-commerce';
const TIMEOUT_MS = 20000;

class RetryError extends Error {
  constructor(code) { super(code); this.code = code; }
}

function counts(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new RetryError('invalid_response');
  const result = {};
  for (const key of ['sent', 'pending', 'due']) {
    if (!Number.isSafeInteger(value[key]) || value[key] < 0) throw new RetryError('invalid_response');
    result[key] = value[key];
  }
  if (result.due > result.pending) throw new RetryError('invalid_response');
  return result;
}

function networkReason(error) {
  // Match known runtime failures, but emit only these fixed classifications.
  const message = typeof error?.message === 'string' ? error.message : '';
  if (/\b1042\b|worker.to.worker|same.zone.*worker/i.test(message)) return 'worker_subrequest_blocked';
  if (/\b1014\b|cross.account|cross.user/i.test(message)) return 'cross_account_blocked';
  if (/redirect/i.test(message)) return 'redirect_blocked';
  if (/\bDNS\b|\bENOTFOUND\b|\bEAI_AGAIN\b|could not resolve|name resolution/i.test(message)) return 'dns_failure';
  if (/\bTLS\b|\bSSL\b|certificate/i.test(message)) return 'tls_failure';
  return 'request_failed';
}

export async function runCommerceRetries(env) {
  let controller, timeout, response;
  let stage = 'configuration';
  try {
    const token = env?.OPERATIONS_TOKEN;
    if (typeof token !== 'string' || token.length < 32 || /\s/.test(token)) throw new RetryError('invalid_configuration');
    controller = new AbortController();
    timeout = setTimeout(() => controller.abort(), TIMEOUT_MS);
    stage = 'fetch';
    response = await fetch(ENDPOINT, {
      method: 'POST',
      // Workers supports manual/follow; reject 3xx below without forwarding auth.
      redirect: 'manual',
      signal: controller.signal,
      headers: {
        Authorization: 'Bearer ' + token,
        Accept: 'application/json',
        'User-Agent': 'GlimdockCommerceOperations/1.0'
      }
    });
    stage = 'headers';
    if (!response.ok) throw new RetryError('http_' + response.status);
    stage = 'json';
    const data = await response.json();
    stage = 'counts';
    // An older storefront may omit interest during a staggered deployment.
    // A present interest object must satisfy the same strict aggregate contract.
    const summary = {
      confirmations: counts(data?.confirmations),
      withdrawals: counts(data?.withdrawals),
      interest: data?.interest === undefined ? {sent: 0, pending: 0, due: 0} : counts(data.interest)
    };
    // Select only aggregate counts; never log a response body, token or error text.
    if (summary.confirmations.pending || summary.withdrawals.pending || summary.interest.pending) console.warn('Glimdock commerce retries pending', summary);
    else console.log('Glimdock commerce retries complete', summary);
    return summary;
  } catch (error) {
    const reason = controller?.signal.aborted ? 'timeout' : error instanceof RetryError ? error.code : stage === 'json' ? 'invalid_response' : networkReason(error);
    const errorType = ['Error', 'TypeError', 'SyntaxError', 'AbortError', 'TimeoutError'].includes(error?.name) ? error.name : 'Other';
    const diagnostic = {reason, stage, errorType};
    if (response) {
      diagnostic.httpStatus = response.status;
      const type = response.headers?.get('Content-Type') || '';
      diagnostic.responseType = /\bjson\b/i.test(type) ? 'json' : /\bhtml\b/i.test(type) ? 'html' : type ? 'other' : 'missing';
    }
    console.error('Glimdock commerce retry failed', diagnostic);
    throw new Error('Commerce retry failed: ' + reason);
  } finally {
    clearTimeout(timeout);
  }
}

export default {
  async scheduled(_controller, env) { await runCommerceRetries(env); },
  fetch() {
    return new Response('Not found', {status: 404, headers: {'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff'}});
  }
};
