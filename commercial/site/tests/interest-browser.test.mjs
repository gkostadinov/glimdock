import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';

const scripts = Object.fromEntries(['interest', 'interest-action'].map(name => [name, readFileSync(new URL('../public/' + name + '.js', import.meta.url), 'utf8')]));
const read = path => readFileSync(new URL('../public/' + path, import.meta.url), 'utf8');
const response = (data, status = 200) => ({ok: status >= 200 && status < 300, status, json: async () => data});
const flush = async () => { for (let i = 0; i < 30; i++) await Promise.resolve(); };
const enabled = {enabled: true, requiresEmailConfirmation: true};
const id = 'f4e0a806-04e2-478f-a10f-2b610a0f3e24', token = 'a'.repeat(64);
function browser(script, {fetch = async () => response(enabled), href = 'https://glimdock.com/interest/'} = {}) {
  const ids = ['interest-form','interest-email','interest-quantity','interest-country','interest-body','interest-stand','interest-consent','interest-website','interest-submit','interest-status','interest-retry','interest-action-card','interest-action','interest-action-status'];
  const elements = Object.fromEntries(ids.map(name => [name, {
    value: name === 'interest-quantity' ? '1' : name === 'interest-country' ? 'BG' : name === 'interest-body' ? 'Purple' : name === 'interest-stand' ? 'White' : '',
    checked: false, hidden: true, disabled: false, textContent: '', events: new Map(), attributes: {},
    options: name === 'interest-country' ? ['BG','DE','FR'].map(value => ({value})) : [],
    addEventListener(name, listener) { this.events.set(name, listener); },
    setAttribute(name, value) { this.attributes[name] = value; },
    setCustomValidity(message) { this.validationMessage = message; },
    reportValidity() { return !Object.values(elements).some(element => element.validationMessage); }
  }]));
  const events = new Map(), windowEvents = new Map(), requests = [], order = [], tools = new Map(), timers = new Map();
  const location = new URL(href);
  const context = {
    document: {querySelector: selector => elements[selector.slice(1)], addEventListener: (name, listener) => events.set(name, listener)},
    location,
    window: {addEventListener: (name, listener) => windowEvents.set(name, listener)},
    history: {replaceState(state, title, path) { order.push('clear-url'); location.href = new URL(path, location).href; }},
    fetch: async (url, options = {}) => { order.push('fetch'); requests.push({url, options}); return fetch(url, options); },
    navigator: {modelContext: {registerTool: tool => tools.set(tool.name, tool)}},
    setTimeout(callback, delay) { const key = timers.size + 1; timers.set(key,{callback,delay}); return key; },
    clearTimeout(key) { timers.delete(key); }, AbortController, URL, URLSearchParams, JSON, Set, Number, Error, TypeError
  };
  runInNewContext(scripts[script], context);
  if (script === 'interest-action') events.get('DOMContentLoaded')();
  return {elements, requests, order, location, tools, timers,
    async window(name, event) { windowEvents.get(name)?.(event); await flush(); },
    submit: () => elements['interest-form'].events.get('submit')({preventDefault() {}}),
    click: () => elements['interest-action'].events.get('click')(),
    input(name, value) { elements[name].value=value; elements[name].events.get('input')?.(); }
  };
}
const posts = page => page.requests.filter(request => request.options.method === 'POST');

test('interest registration requires explicit consent and accepted means check email, not confirmed', async () => {
  const page = browser('interest', {fetch: async url => response(url === '/api/interest-config' ? enabled : {accepted: true}, url === '/api/interest-config' ? 200 : 202)});
  await flush();
  page.input('interest-email',' visitor@example.com ');
  await page.submit(); assert.equal(posts(page).length,0);
  page.elements['interest-consent'].checked=true;
  page.input('interest-body',' Midnight blue '); page.input('interest-stand','');
  await page.submit();
  const body=JSON.parse(posts(page)[0].options.body);
  assert.deepEqual(body,{email:'visitor@example.com',quantity:1,country:'BG',bodyColor:'Midnight blue',standColor:'',consent:true,website:''});
  assert.match(page.elements['interest-status'].textContent,/Check your email/);
  assert.match(page.elements['interest-status'].textContent,/Confirm interest/);
  assert.equal(page.elements['interest-email'].value,'');
  assert.equal(page.elements['interest-consent'].checked,false);
  assert.equal(page.elements['interest-submit'].disabled,true);
  assert(!page.location.href.includes('visitor'));
  await page.submit(); assert.equal(posts(page).length,1);
});

test('read-only interest tool exposes no contact data and cannot submit', async () => {
  const page=browser('interest'); await flush();
  page.input('interest-email','private@example.com');
  assert.equal(page.tools.size,1);
  const info=await page.tools.get('read_glimdock_interest').execute();
  assert.equal(info.enabled,true);assert.equal(info.createsOrder,false);
  assert(!JSON.stringify(info).includes('private'));assert.equal(posts(page).length,0);
});

test('disabled or malformed availability never opens interest submission', async () => {
  for(const config of [{enabled:false,requiresEmailConfirmation:true},{enabled:true,requiresEmailConfirmation:false},{}]){
    const page=browser('interest',{fetch:async()=>response(config)});await flush();
    page.input('interest-email','visitor@example.com');page.elements['interest-consent'].checked=true;
    await page.submit();assert.equal(posts(page).length,0);assert.equal(page.elements['interest-submit'].disabled,true);
  }
});

test('invalid colour and email requests are blocked; network failures permit an explicit retry', async () => {
  const page=browser('interest',{fetch:async url=>url==='/api/interest-config'?response(enabled):Promise.reject(new TypeError('offline'))});await flush();
  page.elements['interest-consent'].checked=true;
  for(const invalid of ['bad','visitor@example.com\nBcc:other@example.com']){
    page.input('interest-email',invalid);await page.submit();assert.equal(posts(page).length,0);
  }
  page.input('interest-email','visitor@example.com');
  for(const invalid of ['<Red>','R'.repeat(65),'Red\nBlue']){
    page.input('interest-body',invalid);await page.submit();assert.equal(posts(page).length,0);
  }
  page.input('interest-body','Purple');await page.submit();
  assert.match(page.elements['interest-status'].textContent,/Connection interrupted/);
  assert.equal(page.elements['interest-email'].value,'visitor@example.com');
  assert.equal(page.elements['interest-submit'].disabled,false);
});

test('token pages clear the fragment before any action and never POST on opening or mail prefetch', async () => {
  for(const action of ['confirm','unsubscribe']){
    const page=browser('interest-action',{href:'https://glimdock.com/interest/'+action+'/?ignored=1#id='+id+'&token='+token,fetch:async()=>response(action==='confirm'?{confirmed:true}:{unsubscribed:true})});
    assert.equal(page.location.hash,'');assert.equal(page.location.search,'');
    assert.deepEqual(page.order,['clear-url']);assert.equal(page.requests.length,0);
    await page.click();
    assert.equal(page.requests[0].url,'/api/interest/'+action);
    assert.deepEqual(JSON.parse(page.requests[0].options.body),{id,token});
    assert.equal(page.elements['interest-action'].disabled,true);
    await page.click();assert.equal(page.requests.length,1);
  }
});

test('invalid and expired action links give generic feedback without contact disclosure', async () => {
  const missing=browser('interest-action',{href:'https://glimdock.com/interest/confirm/#id=invalid&token=invalid'});
  assert.equal(missing.elements['interest-action'].disabled,true);await missing.click();assert.equal(missing.requests.length,0);
  const expired=browser('interest-action',{href:'https://glimdock.com/interest/confirm/#id='+id+'&token='+token,fetch:async()=>response({error:'expired'},410)});
  await expired.click();assert.match(expired.elements['interest-action-status'].textContent,/could not be used/);
  assert(!expired.elements['interest-action-status'].textContent.includes(id));
});

test('a broken connection leaves an action retryable without falsely confirming success', async () => {
  const page=browser('interest-action',{href:'https://glimdock.com/interest/confirm/#id='+id+'&token='+token,fetch:async()=>{throw new TypeError('offline');}});
  await page.click();assert.match(page.elements['interest-action-status'].textContent,/couldn’t confirm the result/);
  assert.equal(page.elements['interest-action'].disabled,false);assert.equal(page.timers.size,0);
});

test('email-link pages use an early external scrubber and contain no analytics loader or private link data',()=>{
  for(const action of ['confirm','unsubscribe']){
    const html=read('interest/'+action+'/index.html');
    assert(html.includes('name="referrer" content="no-referrer"'));
    assert(html.indexOf('/interest-action.js')<html.indexOf('<title>'));
    assert(!html.includes('/consent.js'));assert(!html.includes('googletagmanager'));
    assert(html.includes('type="button" disabled'));
  }
  for(const script of Object.values(scripts)){assert(!/localStorage|sessionStorage|glimdockAnalyticsEvent/.test(script));}
});


test('browser Back releases busy interest controls and ignores a superseded request result', async () => {
  let finish;
  const page=browser('interest',{fetch:async url=>url==='/api/interest-config'?response(enabled):new Promise(resolve=>{finish=resolve;})});await flush();
  page.input('interest-email','visitor@example.com');page.elements['interest-consent'].checked=true;
  const pending=page.submit();await flush();assert.equal(page.elements['interest-submit'].disabled,true);
  await page.window('pageshow',{persisted:true});
  assert.equal(page.elements['interest-submit'].disabled,false);assert.match(page.elements['interest-status'].textContent,/may still be processing/);
  finish(response({accepted:true},202));await pending;
  assert.equal(page.elements['interest-email'].value,'visitor@example.com');
  assert.match(page.elements['interest-status'].textContent,/may still be processing/);
});

test('browser Back restores explicit token action without auto resubmission', async () => {
  let finish;
  const page=browser('interest-action',{href:'https://glimdock.com/interest/confirm/#id='+id+'&token='+token,fetch:()=>new Promise(resolve=>{finish=resolve;})});
  const pending=page.click();await flush();assert.equal(page.elements['interest-action'].disabled,true);
  await page.window('pageshow',{persisted:true});assert.equal(page.elements['interest-action'].disabled,false);
  assert.equal(page.requests.length,1);assert.match(page.elements['interest-action-status'].textContent,/may still be processing/);
  finish(response({confirmed:true}));await pending;
  assert.match(page.elements['interest-action-status'].textContent,/may still be processing/);
});


test('a new fragment on an already-open action page is scrubbed and requires another explicit click', async () => {
  const page=browser('interest-action',{href:'https://glimdock.com/interest/confirm/#id='+id+'&token='+token,fetch:async()=>response({confirmed:true})});
  await page.click();assert.equal(page.elements['interest-action'].disabled,true);
  page.location.hash='id='+id+'&token='+'b'.repeat(64);
  await page.window('hashchange',{});
  assert.equal(page.location.hash,'');assert.equal(page.requests.length,1);
  assert.equal(page.elements['interest-action'].disabled,false);
  assert.equal(page.elements['interest-action'].textContent,'Confirm interest');
  await page.click();assert.equal(JSON.parse(page.requests[1].options.body).token,'b'.repeat(64));
});


test('ordinary skip-link fragments preserve the token action and its completed state', async () => {
  const page=browser('interest-action',{href:'https://glimdock.com/interest/confirm/#id='+id+'&token='+token,fetch:async()=>response({confirmed:true})});
  page.location.hash='main';await page.window('hashchange',{});
  assert.equal(page.location.hash,'#main');assert.equal(page.elements['interest-action'].disabled,false);
  await page.click();assert.deepEqual(JSON.parse(page.requests[0].options.body),{id,token});
  page.location.hash='main';await page.window('hashchange',{});
  assert.equal(page.elements['interest-action'].disabled,true);assert.equal(page.requests.length,1);
});
