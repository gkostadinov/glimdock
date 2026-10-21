import test from 'node:test';
import assert from 'node:assert/strict';
import worker from '../worker/index.js';
import {INTEREST_CONSENT_VERSION} from '../worker/interest.js';
import {fixtureDB} from './fixtures/db.mjs';

const origin='https://glimdock.com';
const operationsToken='fixture-operations-token-32-characters';
const account='b'.repeat(32),tokenSecret='c'.repeat(64);
const input={email:'buyer@example.com',quantity:2,country:'DE',bodyColor:'Black',standColor:'White',consent:true,website:''};
const envFor=DB=>({DB,CHECKOUT_ENABLED:'false',MAIL_PROVIDER:'cloudflare',CLOUDFLARE_EMAIL_ACCOUNT_ID:account,CLOUDFLARE_EMAIL_API_TOKEN:'fixture-cloudflare-mail-token',WAITLIST_TOKEN_SECRET:tokenSecret,OPERATIONS_TOKEN:operationsToken});
const post=(path,body,headers={})=>new Request(origin+path,{method:'POST',headers:{Origin:origin,'Content-Type':'application/json','CF-Connecting-IP':'192.0.2.10',...headers},body:JSON.stringify(body)});
const admin=(path='/api/admin/retry-commerce',method='POST')=>new Request(origin+path,{method,headers:{Authorization:'Bearer '+operationsToken}});
const jsonResponse=value=>new Response(JSON.stringify(value),{headers:{'Content-Type':'application/json'}});
const accepted=to=>jsonResponse({success:true,result:{delivered:[],queued:to,permanent_bounces:[],suppressed_recipients:[]}});
function links(mail){
 const result={};for(const purpose of ['confirm','unsubscribe']){const url=new URL(mail.text.match(new RegExp('https://glimdock\\.com/interest/'+purpose+'/#[^\\s]+'))[0]);result[purpose]=Object.fromEntries(new URLSearchParams(url.hash.slice(1)));assert.equal(url.search,'');assert(!url.href.includes('buyer'));}
 return result;
}
async function submit(DB,body=input,changes={}){const work=[];const response=await worker.fetch(post('/api/interest',body),{...envFor(DB),...changes},{waitUntil:promise=>work.push(promise)});return {response,work};}
async function providers(handler,fn){const original=globalThis.fetch;globalThis.fetch=handler;try{return await fn();}finally{globalThis.fetch=original;}}
async function mailProvider(messages,fn){return providers(async(url,options)=>{assert.equal(url,'https://api.cloudflare.com/client/v4/accounts/'+account+'/email/sending/send');const body=JSON.parse(options.body);messages.push(body);return accepted(body.to);},fn);}
async function clocked(fn){const old=Date.now;let now=Date.UTC(2026,9,17,12,0);Date.now=()=>now;try{return await fn(seconds=>now+=seconds*1000,()=>Math.floor(now/1000));}finally{Date.now=old;}}

test('interest config requires storage, a separate signing secret and a selected mail provider while checkout stays closed',async()=>{
 const DB=fixtureDB();try{
  const request=()=>new Request(origin+'/api/interest-config');assert.deepEqual(await (await worker.fetch(request(),envFor(DB),{})).json(),{enabled:true,requiresEmailConfirmation:true});
  for(const changes of [{DB:undefined},{WAITLIST_TOKEN_SECRET:''},{WAITLIST_TOKEN_SECRET:'too-short'},{CLOUDFLARE_EMAIL_API_TOKEN:''},{INTEREST_ENABLED:'false'}])assert.deepEqual(await (await worker.fetch(request(),{...envFor(DB),...changes},{})).json(),{enabled:false,requiresEmailConfirmation:true});
  const commerce=await (await worker.fetch(new Request(origin+'/api/commerce'),envFor(DB),{})).json();assert.equal(commerce.checkoutReady,false);
 }finally{DB.connection.close();}
});

test('valid expression stores explicit consent and preferences, returns only generic202 and sends an opaque double-opt-in link',async()=>{
 const DB=fixtureDB(),messages=[];try{await mailProvider(messages,async()=>{
  const {response,work}=await submit(DB,{...input,email:' BUYER@EXAMPLE.COM ',bodyColor:' Black ',standColor:''});assert.equal(response.status,202);const publicBody=await response.json();assert.equal(publicBody.accepted,true);assert(!JSON.stringify(publicBody).includes('buyer'));assert.equal(publicBody.id,undefined);assert.equal(publicBody.token,undefined);
  await Promise.all(work);assert.equal(messages.length,1);assert.deepEqual(messages[0].to,['buyer@example.com']);assert(messages[0].text.includes('unpaid expression of interest, not an order or a reservation'));assert(messages[0].text.includes('Body color: Black'));assert(messages[0].text.includes('Stand color: White'));
  const tokens=links(messages[0]);assert.match(tokens.confirm.id,/^[a-f0-9-]{36}$/);assert.match(tokens.confirm.token,/^[a-f0-9]{64}$/);assert.equal(tokens.confirm.id,tokens.unsubscribe.id);assert.notEqual(tokens.confirm.token,tokens.unsubscribe.token);
  const row=DB.connection.prepare('SELECT * FROM interest_signups').get();assert.equal(row.email,'buyer@example.com');assert.equal(row.quantity,2);assert.equal(row.body_color,'Black');assert.equal(row.stand_color,'White');assert.equal(row.consent_version,INTEREST_CONSENT_VERSION);assert(row.consented_at>0);assert.equal(row.confirmed_at,null);assert(row.confirmation_sent_at>0);assert.equal(row.expires_at-row.created_at,7*86400);
  assert.equal(DB.connection.prepare('SELECT COUNT(*) AS n FROM orders').get().n,0);assert.equal(DB.connection.prepare('SELECT COUNT(*) AS n FROM checkout_attempts').get().n,0);
 });}finally{DB.connection.close();}
});

test('invalid, cross-origin and oversized expressions are rejected and honeypots never store or send',async()=>{
 const DB=fixtureDB();try{await providers(()=>{throw Error('No provider call expected');},async()=>{
  for(const change of [{email:'bad'},{email:'buyer@example.com\nBcc:spy@example.com'},{consent:false},{quantity:0},{quantity:6},{quantity:'2'},{country:'US'},{bodyColor:'<red>'},{bodyColor:'\n'},{standColor:'\t'},{standColor:'x'.repeat(65)}])assert.equal((await worker.fetch(post('/api/interest',{...input,...change}),envFor(DB),{})).status,400);
  assert.equal((await worker.fetch(post('/api/interest',input,{Origin:'https://attacker.example'}),envFor(DB),{})).status,403);
  assert.equal((await worker.fetch(post('/api/interest',input,{'Content-Type':'text/plain'}),envFor(DB),{})).status,415);
  const raw=body=>new Request(origin+'/api/interest',{method:'POST',headers:{Origin:origin,'Content-Type':'application/json'},body});assert.equal((await worker.fetch(raw('{'),envFor(DB),{})).status,400);assert.equal((await worker.fetch(raw('x'.repeat(4097)),envFor(DB),{})).status,413);
  const honeypot=await worker.fetch(post('/api/interest',{...input,website:'https://spam.example'}),envFor(DB),{});assert.equal(honeypot.status,202);assert.equal((await honeypot.json()).accepted,true);assert.equal(DB.connection.prepare('SELECT COUNT(*) AS n FROM interest_signups').get().n,0);assert.equal(DB.connection.prepare('SELECT COUNT(*) AS n FROM interest_rate_limits').get().n,0);
 });}finally{DB.connection.close();}
});

test('GET links cannot confirm and HMAC tokens are bound to both opaque row and action',async()=>{
 const DB=fixtureDB(),messages=[];try{await mailProvider(messages,async()=>{
  const queued=await submit(DB);await Promise.all(queued.work);const tokens=links(messages[0]);
  const scanner=await worker.fetch(new Request(origin+'/api/interest/confirm?'+new URLSearchParams(tokens.confirm)),envFor(DB),{});assert.equal(scanner.status,404);assert.equal(DB.connection.prepare('SELECT confirmed_at FROM interest_signups').get().confirmed_at,null);
  assert.equal((await worker.fetch(new Request(origin+'/api/interest/unsubscribe?'+new URLSearchParams(tokens.unsubscribe)),envFor(DB),{})).status,404);
  assert.equal((await worker.fetch(post('/api/interest/unsubscribe',tokens.confirm),envFor(DB),{})).status,410);
  for(const body of [tokens.unsubscribe,{...tokens.confirm,token:'0'.repeat(64)},{...tokens.confirm,id:crypto.randomUUID()},{...tokens.confirm,id:[tokens.confirm.id]}])assert.equal((await worker.fetch(post('/api/interest/confirm',body),envFor(DB),{})).status,410);
  const confirmed=await worker.fetch(post('/api/interest/confirm',tokens.confirm),envFor(DB),{});assert.equal(confirmed.status,200);assert.deepEqual(await confirmed.json(),{confirmed:true});assert.deepEqual(await (await worker.fetch(post('/api/interest/confirm',tokens.confirm),envFor(DB),{})).json(),{confirmed:true});assert.equal(messages.length,1);
 });}finally{DB.connection.close();}
});

test('duplicate cooldown preserves original preferences and confirmed re-submission causes no new email or retention extension',async()=>{
 await clocked(async(advance)=>{const DB=fixtureDB(),messages=[];try{await mailProvider(messages,async()=>{
  let queued=await submit(DB);const generic=await queued.response.json();await Promise.all(queued.work);const original=DB.connection.prepare('SELECT * FROM interest_signups').get();const tokens=links(messages[0]);
  queued=await submit(DB,{...input,quantity:5,country:'BG',bodyColor:'Red'});assert.deepEqual(await queued.response.json(),generic);await Promise.all(queued.work);assert.equal(messages.length,1);
  advance(3600);queued=await submit(DB,{...input,quantity:5,country:'BG'});await Promise.all(queued.work);assert.equal(messages.length,2);assert.equal(DB.connection.prepare('SELECT quantity FROM interest_signups').get().quantity,2);assert.equal(DB.connection.prepare('SELECT expires_at FROM interest_signups').get().expires_at,original.expires_at);
  assert.deepEqual(links(messages[1]),tokens);assert.equal((await worker.fetch(post('/api/interest/confirm',tokens.confirm),envFor(DB),{})).status,200);const confirmed=DB.connection.prepare('SELECT * FROM interest_signups').get();assert(confirmed.confirmed_at>0);
  advance(3600);queued=await submit(DB,{...input,quantity:5,country:'BG',bodyColor:'Red'});assert.deepEqual(await queued.response.json(),generic);await Promise.all(queued.work);assert.equal(messages.length,2);assert.deepEqual(DB.connection.prepare('SELECT * FROM interest_signups').get(),confirmed);
 });}finally{DB.connection.close();}});
});

test('hashed IP admission is atomic and expires without retaining plaintext IP',async()=>{
 await clocked(async(advance,now)=>{const DB=fixtureDB();try{await providers(()=>{throw Error('Durable requests have no immediate provider dispatch without waitUntil');},async()=>{
  let generic;const responses=await Promise.all(Array.from({length:12},(_,i)=>worker.fetch(post('/api/interest',{...input,email:'buyer'+i+'@example.com'}),envFor(DB),{})));for(const response of responses){assert.equal(response.status,202);const body=await response.json();if(generic)assert.deepEqual(body,generic);generic=body;}
  assert.equal(DB.connection.prepare('SELECT COUNT(*) AS n FROM interest_signups').get().n,10);const rate=DB.connection.prepare('SELECT * FROM interest_rate_limits').get();assert.match(rate.ip_hash,/^[a-f0-9]{64}$/);assert.equal(rate.request_count,10);assert(rate.expires_at<=now()+86400);assert(!JSON.stringify(rate).includes('192.0.2.10'));
  advance(86400);await worker.fetch(admin(),{...envFor(DB),CLOUDFLARE_EMAIL_API_TOKEN:''},{});assert.equal(DB.connection.prepare('SELECT COUNT(*) AS n FROM interest_rate_limits').get().n,0);assert.equal(DB.connection.prepare('SELECT COUNT(*) AS n FROM interest_signups').get().n,10);
 });}finally{DB.connection.close();}});
});

test('durable interest mail retries provider failures with backoff and leased concurrent retries send once',async()=>{
 const DB=fixtureDB(),messages=[];let succeed=false;try{await providers(async(url,options)=>{assert(url.startsWith('https://api.cloudflare.com/'));const body=JSON.parse(options.body);messages.push(body);return succeed?accepted(body.to):new Response('outage',{status:503});},async()=>{
  const queued=await submit(DB);await Promise.all(queued.work);let row=DB.connection.prepare('SELECT * FROM interest_signups').get();assert.equal(row.confirmation_sent_at,null);assert.equal(row.attempt_count,1);assert(row.next_attempt_at>Math.floor(Date.now()/1000));
  let counts=await (await worker.fetch(admin(),envFor(DB),{})).json();assert.deepEqual(counts.interest,{sent:0,pending:1,due:0});assert.equal(messages.length,1);
  succeed=true;DB.connection.prepare('UPDATE interest_signups SET next_attempt_at=0').run();const responses=await Promise.all([worker.fetch(admin(),envFor(DB),{}),worker.fetch(admin(),envFor(DB),{})]);for(const response of responses)assert.equal(response.status,200);assert.equal(messages.length,2);assert.deepEqual(messages[0],messages[1]);row=DB.connection.prepare('SELECT * FROM interest_signups').get();assert.equal(row.attempt_count,2);assert(row.confirmation_sent_at>0);
  counts=await (await worker.fetch(admin(),envFor(DB),{})).json();assert.deepEqual(counts.interest,{sent:0,pending:0,due:0});assert.equal(messages.length,2);
 });}finally{DB.connection.close();}
});

test('confirmation and unsubscribe work with mail disabled; unsubscribe erases the record and old tokens cannot affect a new request',async()=>{
 const DB=fixtureDB(),messages=[];try{await mailProvider(messages,async()=>{
  const queued=await submit(DB);await Promise.all(queued.work);const tokens=links(messages[0]),disabled={...envFor(DB),CLOUDFLARE_EMAIL_API_TOKEN:'',INTEREST_ENABLED:'false'};
  assert.equal((await worker.fetch(post('/api/interest/confirm',tokens.confirm),disabled,{})).status,200);
  assert.deepEqual(await (await worker.fetch(post('/api/interest/unsubscribe',tokens.unsubscribe),disabled,{})).json(),{unsubscribed:true});assert.equal(DB.connection.prepare('SELECT COUNT(*) AS n FROM interest_signups').get().n,0);
  assert.deepEqual(await (await worker.fetch(post('/api/interest/unsubscribe',tokens.unsubscribe),disabled,{})).json(),{unsubscribed:true});assert.equal((await worker.fetch(post('/api/interest/confirm',tokens.confirm),disabled,{})).status,410);
  const again=await submit(DB);await Promise.all(again.work);assert.equal(messages.length,2);const newRow=DB.connection.prepare('SELECT * FROM interest_signups').get();assert.notEqual(newRow.id,tokens.confirm.id);assert.equal(newRow.confirmed_at,null);assert.equal((await worker.fetch(post('/api/interest/confirm',tokens.confirm),envFor(DB),{})).status,410);assert.equal(DB.connection.prepare('SELECT confirmed_at FROM interest_signups').get().confirmed_at,null);
 });}finally{DB.connection.close();}
});

test('unconfirmed requests expire after seven days and confirmed requests are erased after twelve calendar months',async()=>{
 await clocked(async(advance,now)=>{const DB=fixtureDB(),messages=[];try{await mailProvider(messages,async()=>{
  let queued=await submit(DB);await Promise.all(queued.work);const expired=links(messages[0]);advance(7*86400);
  assert.equal((await worker.fetch(post('/api/interest/confirm',expired.confirm),envFor(DB),{})).status,410);assert.equal(DB.connection.prepare('SELECT COUNT(*) AS n FROM interest_signups').get().n,0);
  queued=await submit(DB);await Promise.all(queued.work);const tokens=links(messages[1]);assert.equal((await worker.fetch(post('/api/interest/confirm',tokens.confirm),envFor(DB),{})).status,200);
  const row=DB.connection.prepare('SELECT * FROM interest_signups').get(),expected=new Date(now()*1000);expected.setUTCMonth(expected.getUTCMonth()+12);assert.equal(row.expires_at,expected.getTime()/1000);advance(row.expires_at-now());
  let deferred;const result=await worker.scheduled({}, {...envFor(DB),CLOUDFLARE_EMAIL_API_TOKEN:''},{waitUntil:work=>deferred=work});await deferred;assert.deepEqual(result.interest,{sent:0,pending:0,due:0});assert.equal(DB.connection.prepare('SELECT COUNT(*) AS n FROM interest_signups').get().n,0);assert.equal(messages.length,2);
 });}finally{DB.connection.close();}});
});

test('protected interest summary reports counts and confirmed units by country without email or link identifiers',async()=>{
 const DB=fixtureDB(),messages=[];try{await mailProvider(messages,async()=>{
  let queued=await submit(DB);await Promise.all(queued.work);const tokens=links(messages[0]);await worker.fetch(post('/api/interest/confirm',tokens.confirm),envFor(DB),{});
  queued=await submit(DB,{...input,email:'other@example.com',country:'BG',quantity:5});await Promise.all(queued.work);
  const denied=await worker.fetch(new Request(origin+'/api/admin/interest-summary'),envFor(DB),{});assert.equal(denied.status,401);
  const response=await worker.fetch(admin('/api/admin/interest-summary','GET'),envFor(DB),{});assert.equal(response.status,200);const data=await response.json();assert.deepEqual(data,{confirmed:1,pending:1,requestedUnits:2,countries:[{country:'BG',confirmed:0,pending:1,requestedUnits:0},{country:'DE',confirmed:1,pending:0,requestedUnits:2}]});assert(!JSON.stringify(data).includes('@'));assert(!JSON.stringify(data).includes(tokens.confirm.id));
 });}finally{DB.connection.close();}
});
