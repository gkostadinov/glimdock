import test from 'node:test';
import assert from 'node:assert/strict';
import {fixtureDB} from './fixtures/db.mjs';
import worker,{ready} from '../worker/index.js';
import {CURRENT_TERMS_VERSION,acceptedTermsHTML} from '../worker/accepted-terms.js';

const origin='https://glimdock.com';
const token='fixture-operations-token-32-characters';
const secret='whsec_fixture_not_a_real_secret';
const cart={quantity:2,country:'DE',terms:true,intent:'f4e0a806-04e2-478f-a10f-2b610a0f3e24'};
const session=()=>({id:'cs_live_fixture1234567890ABCDEF',object:'checkout.session',mode:'payment',currency:'eur',status:'complete',livemode:true,payment_status:'paid',amount_total:21300,created:1791500000,customer_details:{email:'buyer@example.com'},metadata:{quantity:'2',country:'DE',product:'glimdock-desk-first-batch',terms_accepted:'true',terms_version:CURRENT_TERMS_VERSION,seller_postal_address:'Accepted fixture address <Unit A>'}});
const fixtureEnv=DB=>({DB,CHECKOUT_ENABLED:'true',STRIPE_SECRET_KEY:'sk_live_fixture_only',STRIPE_WEBHOOK_SECRET:secret,RESEND_API_KEY:'re_fixture',SELLER_POSTAL_ADDRESS:'Later fixture address',SUPPORT_READY:'true',PRODUCTION_READY:'true',ORDER_CONFIRMATIONS_READY:'true',OPERATIONS_READY:'true',OPERATIONS_TOKEN:token});
const cloudflareEnv=DB=>({...fixtureEnv(DB),RESEND_API_KEY:'',MAIL_PROVIDER:'cloudflare',CLOUDFLARE_EMAIL_ACCOUNT_ID:'a'.repeat(32),CLOUDFLARE_EMAIL_API_TOKEN:'fixture-cloudflare-token'});
const cloudflareResult=(result={})=>jsonResponse({success:true,errors:[],result:{delivered:['buyer@example.com'],queued:[],permanent_bounces:[],suppressed_recipients:[],message_id:'<fixture-message@glimdock.com>',...result}});
const post=(path,body,headers={})=>new Request(origin+path,{method:'POST',headers:{Origin:origin,'Content-Type':'application/json',...headers},body:JSON.stringify(body)});

async function signed(body){
 const t=Math.floor(Date.now()/1000);
 const key=await crypto.subtle.importKey('raw',new TextEncoder().encode(secret),{name:'HMAC',hash:'SHA-256'},false,['sign']);
 const mac=[...new Uint8Array(await crypto.subtle.sign('HMAC',key,new TextEncoder().encode(t+'.'+body)))].map(x=>x.toString(16).padStart(2,'0')).join('');
 return new Request(origin+'/api/stripe-webhook',{method:'POST',headers:{'Content-Type':'application/json','stripe-signature':'t='+t+',v1='+mac},body});
}
const event=s=>JSON.stringify({id:'evt_fixture',type:'checkout.session.completed',data:{object:s}});
const admin=()=>new Request(origin+'/api/admin/retry-withdrawals',{method:'POST',headers:{Authorization:'Bearer '+token}});
async function mockProviders(handler,fn){const old=globalThis.fetch;globalThis.fetch=handler;try{return await fn();}finally{globalThis.fetch=old;}}
const jsonResponse=value=>new Response(JSON.stringify(value),{headers:{'Content-Type':'application/json'}});

test('operations readiness blocks checkout until both schedule attestation and token are set',async()=>{
 const DB=fixtureDB();
 try{await mockProviders(()=>{throw Error('No provider request expected');},async()=>{
  for(const mutation of [{OPERATIONS_READY:'false'},{OPERATIONS_TOKEN:''},{OPERATIONS_TOKEN:'too-short'}]){
   const env={...fixtureEnv(DB),...mutation};
   assert.equal((await worker.fetch(post('/api/checkout',cart),env,{})).status,503);
   const status=await (await worker.fetch(new Request(origin+'/api/commerce'),env,{})).json();
   assert.equal(status.checkoutReady,false);assert.equal(status.withdrawalReady,true);
  }
 });}finally{DB.connection.close();}
});

test('checkout captures accepted terms version and public seller address in session metadata',async()=>{
 const DB=fixtureDB();let sent;
 try{await mockProviders(async(url,options)=>{assert.equal(url,'https://api.stripe.com/v1/checkout/sessions');sent=new URLSearchParams(options.body);return jsonResponse({...session(),status:'open',payment_status:'unpaid',url:'https://checkout.stripe.com/c/pay/fixture'});},async()=>{
  assert.equal((await worker.fetch(post('/api/checkout',cart),fixtureEnv(DB),{})).status,200);
  assert.equal(sent.get('metadata[terms_version]'),CURRENT_TERMS_VERSION);
  assert.equal(sent.get('metadata[seller_postal_address]'),'Later fixture address');
 });}finally{DB.connection.close();}
});

test('signed malformed webhook payloads return 400 without provider calls or order writes',async()=>{
 const DB=fixtureDB();const malformed=['{','null','[]','{}','{"type":12}','{"type":"checkout.session.completed"}','{"type":"checkout.session.completed","data":{"object":null}}','{"type":"checkout.session.completed","data":{"object":{"id":"bad"}}}'];
 try{await mockProviders(()=>{throw Error('No provider request expected');},async()=>{
  for(const body of malformed){const r=await worker.fetch(await signed(body),fixtureEnv(DB),{});assert.equal(r.status,400,body);}
  assert.equal(DB.connection.prepare('SELECT COUNT(*) AS n FROM orders').get().n,0);
 });}finally{DB.connection.close();}
});

test('paid webhook retains order when mail fails, safely retries, then suppresses duplicate confirmation',async()=>{
 const DB=fixtureDB(),s=session(),messages=[];let attempts=0;
 try{await mockProviders(async(url,options)=>{
  if(url==='https://api.stripe.com/v1/checkout/sessions/'+s.id){assert.equal(options.headers['Stripe-Version'],'2026-09-30.endive');return jsonResponse(s);}
  assert.equal(url,'https://api.resend.com/emails');messages.push({key:options.headers['Idempotency-Key'],body:JSON.parse(options.body)});
  return ++attempts===1?new Response('mail outage',{status:503}):jsonResponse({id:'email_fixture'});
 },async()=>{
  assert.equal((await worker.fetch(await signed(event(s)),fixtureEnv(DB),{})).status,503);
  let row=DB.connection.prepare('SELECT * FROM orders').get();assert.equal(row.payment_status,'paid');assert.equal(row.confirmed_at,null);assert.equal(typeof row.email_hash,'string');
  DB.connection.prepare('UPDATE orders SET confirmation_next_attempt_at=0').run();
  assert.equal((await worker.fetch(await signed(event(s)),fixtureEnv(DB),{})).status,200);
  row=DB.connection.prepare('SELECT * FROM orders').get();assert(row.confirmed_at>0);
  assert.equal((await worker.fetch(await signed(event(s)),fixtureEnv(DB),{})).status,200);
  assert.equal(messages.length,2);assert.equal(messages[0].key,messages[1].key);assert.deepEqual(messages[0].body,messages[1].body);
  const mail=messages[1].body;assert(mail.text.includes('Accepted fixture address <Unit A>'));assert(!mail.text.includes('Later fixture address'));
  const html=Buffer.from(mail.attachments[0].content,'base64').toString('utf8');assert(html.includes('Accepted fixture address &lt;Unit A&gt;'));assert(!html.includes('Later fixture address'));assert(!html.includes('Checkout is being connected'));
  assert(mail.text.includes('Total paid: EUR 213.00'));assert(mail.text.includes('Accepted terms version: '+CURRENT_TERMS_VERSION));
 });}finally{DB.connection.close();}
});

test('status endpoint distinguishes verified payment from failed mail confirmation',async()=>{
 const DB=fixtureDB(),s=session();let mailOK=false;
 try{await mockProviders(async(url)=>{
  if(url==='https://api.stripe.com/v1/checkout/sessions/'+s.id)return jsonResponse(s);
  assert.equal(url,'https://api.resend.com/emails');return mailOK?jsonResponse({id:'email_fixture'}):new Response('outage',{status:503});
 },async()=>{
  const request=()=>new Request(origin+'/api/order-status?session_id='+s.id);
  let r=await worker.fetch(request(),fixtureEnv(DB),{});assert.equal(r.status,200);let data=await r.json();assert.equal(data.paymentStatus,'paid');assert.equal(data.emailConfirmed,false);
  mailOK=true;DB.connection.prepare('UPDATE orders SET confirmation_next_attempt_at=0').run();r=await worker.fetch(request(),fixtureEnv(DB),{});assert.equal(r.status,200);data=await r.json();assert.equal(data.emailConfirmed,true);
 });}finally{DB.connection.close();}
});

test('unknown accepted terms fail closed without falsely marking confirmation sent',async()=>{
 const DB=fixtureDB(),s=session();s.metadata.terms_version='future-unavailable-version';let mailCalls=0;
 try{await mockProviders(async(url)=>{if(url==='https://api.stripe.com/v1/checkout/sessions/'+s.id)return jsonResponse(s);mailCalls++;throw Error('Mail should not be sent');},async()=>{
  assert.equal((await worker.fetch(await signed(event(s)),fixtureEnv(DB),{})).status,503);
  assert.equal(mailCalls,0);assert.equal(DB.connection.prepare('SELECT confirmed_at FROM orders').get().confirmed_at,null);
 });}finally{DB.connection.close();}
});

test('a historical paid session receives its accepted terms instead of the newer public page',async()=>{
 const DB=fixtureDB(),s=session();s.metadata.terms_version='2026-10-08.1';let mail;
 try{await mockProviders(async(url,options)=>{
  if(url==='https://api.stripe.com/v1/checkout/sessions/'+s.id)return jsonResponse(s);
  assert.equal(url,'https://api.resend.com/emails');mail=JSON.parse(options.body);return jsonResponse({id:'email_fixture'});
 },async()=>{
  assert.equal((await worker.fetch(await signed(event(s)),fixtureEnv(DB),{})).status,200);
  const html=Buffer.from(mail.attachments[0].content,'base64').toString('utf8');
  assert(html.includes('Last updated: 8 October 2026'));assert(!html.includes('Last updated: 9 October 2026'));
  assert(mail.text.includes('Accepted terms version: 2026-10-08.1'));
 });}finally{DB.connection.close();}
});

test('canonical unpaid state prevents confirmation from a stale paid event',async()=>{
 const DB=fixtureDB(),s=session();let mails=0;
 try{await mockProviders(async(url)=>{if(url==='https://api.stripe.com/v1/checkout/sessions/'+s.id)return jsonResponse({...s,payment_status:'unpaid'});mails++;throw Error('No mail expected');},async()=>{
  assert.equal((await worker.fetch(await signed(event(s)),fixtureEnv(DB),{})).status,200);assert.equal(mails,0);assert.equal(DB.connection.prepare('SELECT COUNT(*) AS n FROM orders').get().n,0);
 });}finally{DB.connection.close();}
});

test('operations endpoint reports all pending notices, including future retries and rows past batch limit',async()=>{
 const DB=fixtureDB(),now=Math.floor(Date.now()/1000);for(let i=0;i<25;i++)DB.connection.prepare('INSERT INTO withdrawals (id,reference,email,name,submitted_at,next_attempt_at) VALUES (?,?,?,?,?,?)').run('withdraw-fixture-'+i,'GD-1234567890ABCDEF','buyer@example.com','Fixture Buyer',now,now+600);
 try{await mockProviders(()=>{throw Error('No due messages expected');},async()=>{
  const r=await worker.fetch(admin(),fixtureEnv(DB),{});assert.equal(r.status,200);assert.deepEqual(await r.json(),{sent:0,pending:25,due:0});
  const denied=await worker.fetch(new Request(origin+'/api/admin/retry-withdrawals',{method:'POST'}),fixtureEnv(DB),{});assert.equal(denied.status,401);
  const noMailer={...fixtureEnv(DB),RESEND_API_KEY:''};const stillPending=await (await worker.fetch(admin(),noMailer,{})).json();assert.equal(stillPending.pending,25);
 });}finally{DB.connection.close();}
});

test('outbox retries a due failure, preserves notice and succeeds after its backoff',async()=>{
 const DB=fixtureDB(),now=Math.floor(Date.now()/1000);DB.connection.prepare('INSERT INTO withdrawals (id,reference,email,name,submitted_at,next_attempt_at) VALUES (?,?,?,?,?,?)').run('withdraw-fixture-retry','GD-1234567890ABCDEF','buyer@example.com','Fixture Buyer',now,0);let mailOK=false;const keys=[];
 try{await mockProviders(async(url,options)=>{assert.equal(url,'https://api.resend.com/emails');keys.push(options.headers['Idempotency-Key']);return mailOK?jsonResponse({id:'email_fixture'}):new Response('outage',{status:503});},async()=>{
  let counts=await (await worker.fetch(admin(),fixtureEnv(DB),{})).json();assert.deepEqual(counts,{sent:0,pending:1,due:0});let row=DB.connection.prepare('SELECT * FROM withdrawals').get();assert.equal(row.acknowledged_at,null);assert.equal(row.attempt_count,1);assert(row.next_attempt_at>now);
  mailOK=true;DB.connection.prepare('UPDATE withdrawals SET next_attempt_at=0').run();counts=await (await worker.fetch(admin(),fixtureEnv(DB),{})).json();assert.deepEqual(counts,{sent:1,pending:0,due:0});row=DB.connection.prepare('SELECT * FROM withdrawals').get();assert(row.acknowledged_at>0);assert.equal(keys[0],keys[1]);
 });}finally{DB.connection.close();}
});

test('accepted snapshot escapes the captured address and rejects absent versions or addresses',()=>{
 const metadata=session().metadata;const first=acceptedTermsHTML(metadata);assert.equal(first,acceptedTermsHTML({...metadata}));assert(first.includes('4–6 weeks from payment'));assert(first.includes('&lt;Unit A&gt;'));
 assert.throws(()=>acceptedTermsHTML({...metadata,terms_version:'missing'}));assert.throws(()=>acceptedTermsHTML({...metadata,seller_postal_address:''}));assert.throws(()=>acceptedTermsHTML({...metadata,seller_postal_address:'x'.repeat(501)}));
});

test('malformed JSON, oversized requests, invalid content type and cross-origin checkout fail before providers',async()=>{
 const DB=fixtureDB();
 try{await mockProviders(()=>{throw Error('Provider must not be called');},async()=>{
  const raw=(body,headers={})=>new Request(origin+'/api/checkout',{method:'POST',headers:{Origin:origin,'Content-Type':'application/json',...headers},body});
  assert.equal((await worker.fetch(raw('{'),fixtureEnv(DB),{})).status,400);
  assert.equal((await worker.fetch(raw('"'+'€'.repeat(2000)+'"'),fixtureEnv(DB),{})).status,413);
  assert.equal((await worker.fetch(raw(JSON.stringify(cart),{'Content-Type':'application/json-invalid'}),fixtureEnv(DB),{})).status,415);
  assert.equal((await worker.fetch(raw(JSON.stringify(cart),{Origin:'https://attacker.example'}),fixtureEnv(DB),{})).status,403);
  assert.equal((await worker.fetch(post('/api/withdrawal',null),fixtureEnv(DB),{})).status,400);
  assert.equal(DB.connection.prepare('SELECT COUNT(*) AS n FROM checkout_attempts').get().n,0);
 });}finally{DB.connection.close();}
});

test('durable checkout attempts retain accepted terms, reuse sessions and distinguish completed or expired checkout',async()=>{
 const DB=fixtureDB(),s={...session(),status:'open',payment_status:'unpaid',url:'https://checkout.stripe.com/c/pay/fixture'};let creates=0,reads=0;
 try{await mockProviders(async(url,options)=>{
  if(options.method==='POST'){creates++;assert.equal(new URLSearchParams(options.body).get('client_reference_id'),cart.intent);return jsonResponse(s);}
  reads++;assert.equal(url,'https://api.stripe.com/v1/checkout/sessions/'+s.id);return jsonResponse(s);
 },async()=>{
  const env=fixtureEnv(DB);assert.equal((await worker.fetch(post('/api/checkout',cart),env,{})).status,200);
  assert.equal((await worker.fetch(post('/api/checkout',cart),{...env,SELLER_POSTAL_ADDRESS:'New seller address'},{})).status,200);
  assert.equal(creates,1);assert.equal(reads,1);
  const attempt=DB.connection.prepare('SELECT * FROM checkout_attempts').get();assert.equal(attempt.seller_postal_address,'Later fixture address');assert.equal(attempt.terms_version,CURRENT_TERMS_VERSION);assert.equal(attempt.session_id,s.id);
  const mismatch=await worker.fetch(post('/api/checkout',{...cart,quantity:1}),env,{});assert.equal(mismatch.status,409);
  s.status='complete';s.payment_status='paid';let result=await (await worker.fetch(post('/api/checkout',cart),env,{})).json();assert.deepEqual(result,{completed:true,sessionId:s.id});
  s.status='expired';s.payment_status='unpaid';const expired=await worker.fetch(post('/api/checkout',cart),env,{});assert.equal(expired.status,409);result=await expired.json();assert.equal(result.restartCheckout,true);assert.equal(creates,1);
 });}finally{DB.connection.close();}
});

test('an unresolved checkout beyond provider idempotency retention is never recreated',async()=>{
 const DB=fixtureDB();DB.connection.prepare('INSERT INTO checkout_attempts (intent,quantity,country,terms_version,seller_postal_address,created_at,checkout_origin) VALUES (?,?,?,?,?,?,?)').run(cart.intent,cart.quantity,cart.country,CURRENT_TERMS_VERSION,'Original address',Math.floor(Date.now()/1000)-24*3600,origin);
 try{await mockProviders(()=>{throw Error('Provider must not be called');},async()=>{
  const response=await worker.fetch(post('/api/checkout',cart),fixtureEnv(DB),{});assert.equal(response.status,409);assert((await response.json()).error.includes('contact support before paying again'));
 });}finally{DB.connection.close();}
});

test('status reports verified summary and rejects different session IDs, mode or impossible payment lifecycle',async()=>{
 const DB=fixtureDB(),s=session();let canonical=s;
 try{await mockProviders(async(url)=>{assert.equal(url,'https://api.stripe.com/v1/checkout/sessions/'+s.id);return jsonResponse(canonical);},async()=>{
  const status=()=>worker.fetch(new Request(origin+'/api/order-status?session_id='+s.id),fixtureEnv(DB),{});
  for(const mutation of [{id:'cs_live_other1234567890ABCDEF'},{livemode:false},{status:'open'},{status:'unexpected'},{payment_status:'no_payment_required'}]){canonical={...s,...mutation};assert.equal((await status()).status,404);}
  assert.equal(DB.connection.prepare('SELECT COUNT(*) AS n FROM orders').get().n,0);
  canonical={...s,status:'expired',payment_status:'unpaid'};
  const response=await status();assert.equal(response.status,200);const data=await response.json();assert.deepEqual(data,{paymentStatus:'unpaid',checkoutStatus:'expired',reference:'GD-1234567890ABCDEF',emailConfirmed:false,quantity:2,country:'DE',bodyColor:'Purple',standColor:'White',amount:21300,currency:'eur'});
 });}finally{DB.connection.close();}
});

test('operations retries paid confirmations without a buyer visit and retains the first accepted snapshot',async()=>{
 const DB=fixtureDB(),s=session(),messages=[];let mailOK=false;
 const retry=()=>new Request(origin+'/api/admin/retry-commerce',{method:'POST',headers:{Authorization:'Bearer '+token}});
 try{await mockProviders(async(url,options)=>{
  if(url==='https://api.stripe.com/v1/checkout/sessions/'+s.id)return jsonResponse(s);
  assert.equal(url,'https://api.resend.com/emails');messages.push({key:options.headers['Idempotency-Key'],body:JSON.parse(options.body)});return mailOK?jsonResponse({id:'mail_fixture'}):new Response('outage',{status:503});
 },async()=>{
  assert.equal((await worker.fetch(await signed(event(s)),fixtureEnv(DB),{})).status,503);
  let counts=await (await worker.fetch(retry(),fixtureEnv(DB),{})).json();assert.deepEqual(counts,{confirmations:{sent:0,pending:1,due:0},withdrawals:{sent:0,pending:0,due:0},interest:{sent:0,pending:0,due:0}});
  assert.equal(messages.length,1);s.metadata={...s.metadata,terms_version:'2026-10-08.1',seller_postal_address:'Changed Stripe metadata address'};mailOK=true;
  DB.connection.prepare('UPDATE orders SET confirmation_next_attempt_at=0').run();
  counts=await (await worker.fetch(retry(),fixtureEnv(DB),{})).json();assert.equal(counts.confirmations.sent,1);assert.equal(counts.confirmations.pending,0);
  assert.equal(messages.length,2);assert.deepEqual(messages[0],messages[1]);
  const row=DB.connection.prepare('SELECT * FROM orders').get();assert.equal(row.terms_version,CURRENT_TERMS_VERSION);assert.equal(row.seller_postal_address,'Accepted fixture address <Unit A>');assert.equal(row.confirmation_attempt_count,2);assert(row.confirmed_at>0);
 });}finally{DB.connection.close();}
});

test('concurrent status checks claim one paid confirmation attempt',async()=>{
 const DB=fixtureDB(),s=session();let mails=0,releaseMail,mailStarted;const started=new Promise(resolve=>mailStarted=resolve),held=new Promise(resolve=>releaseMail=resolve);
 try{await mockProviders(async(url)=>{
  if(url==='https://api.stripe.com/v1/checkout/sessions/'+s.id)return jsonResponse(s);
  assert.equal(url,'https://api.resend.com/emails');mails++;mailStarted();await held;return jsonResponse({id:'mail_fixture'});
 },async()=>{
  const request=()=>new Request(origin+'/api/order-status?session_id='+s.id);
  const responses=Promise.all([worker.fetch(request(),fixtureEnv(DB),{}),worker.fetch(request(),fixtureEnv(DB),{})]);
  await started;releaseMail();const results=await responses;for(const response of results)assert.equal(response.status,200);
  assert.equal(mails,1);assert.equal(DB.connection.prepare('SELECT confirmation_attempt_count FROM orders').get().confirmation_attempt_count,1);
 });}finally{releaseMail?.();DB.connection.close();}
});

test('scheduled commerce retry includes all pending confirmations even when provider is unavailable',async()=>{
 const DB=fixtureDB(),now=Math.floor(Date.now()/1000);for(let i=0;i<25;i++)DB.connection.prepare('INSERT INTO orders (session_id,reference,payment_status,quantity,country,amount,created_at,confirmation_next_attempt_at) VALUES (?,?,?,?,?,?,?,?)').run('cs_live_pending'+i,'GD-FIXTURE'+String(i).padStart(9,'0'),'paid',1,'BG',10400,now,now+600);
 try{await mockProviders(()=>{throw Error('No due messages expected');},async()=>{
  let deferred;const result=await worker.scheduled({},fixtureEnv(DB),{waitUntil:work=>deferred=work});await deferred;
  assert.deepEqual(result,{confirmations:{sent:0,pending:25,due:0},withdrawals:{sent:0,pending:0,due:0},interest:{sent:0,pending:0,due:0}});
  const noMail={...fixtureEnv(DB),RESEND_API_KEY:''};const request=new Request(origin+'/api/admin/retry-confirmations',{method:'POST',headers:{Authorization:'Bearer '+token}});assert.equal((await (await worker.fetch(request,noMail,{})).json()).pending,25);
 });}finally{DB.connection.close();}
});

test('lost persistence response safely repeats the original Stripe request across origins and seller updates',async()=>{
 const DB=fixtureDB(),originalPrepare=DB.prepare,s={...session(),status:'open',payment_status:'unpaid',url:'https://checkout.stripe.com/c/pay/fixture'},sent=[];let failOnce=true;
 DB.prepare=function(sql){const statement=originalPrepare(sql);if(sql.startsWith('UPDATE checkout_attempts SET session_id')&&failOnce){statement.run=async()=>{failOnce=false;throw Error('Transient database outage');};}return statement;};
 try{await mockProviders(async(url,options)=>{assert.equal(url,'https://api.stripe.com/v1/checkout/sessions');sent.push({key:options.headers['Idempotency-Key'],body:String(options.body)});return jsonResponse(s);},async()=>{
  assert.equal((await worker.fetch(post('/api/checkout',cart),fixtureEnv(DB),{})).status,502);
  const otherOrigin='https://www.glimdock.com',request=new Request(otherOrigin+'/api/checkout',{method:'POST',headers:{Origin:otherOrigin,'Content-Type':'application/json'},body:JSON.stringify(cart)});
  const result=await worker.fetch(request,{...fixtureEnv(DB),SELLER_POSTAL_ADDRESS:'New address after first request'},{});assert.equal(result.status,200);assert.deepEqual(await result.json(),{url:s.url,sessionId:s.id});assert.equal(sent.length,2);assert.deepEqual(sent[0],sent[1]);
 });}finally{DB.connection.close();}
});

test('a paid order uses terms accepted at checkout even if Stripe metadata was later edited',async()=>{
 const DB=fixtureDB(),s={...session(),status:'open',payment_status:'unpaid',url:'https://checkout.stripe.com/c/pay/fixture'};let mail;
 try{await mockProviders(async(url,options)=>{
  if(url==='https://api.stripe.com/v1/checkout/sessions')return jsonResponse(s);
  if(url==='https://api.stripe.com/v1/checkout/sessions/'+s.id)return jsonResponse(s);
  assert.equal(url,'https://api.resend.com/emails');mail=JSON.parse(options.body);return jsonResponse({id:'mail_fixture'});
 },async()=>{
  assert.equal((await worker.fetch(post('/api/checkout',cart),fixtureEnv(DB),{})).status,200);
  s.status='complete';s.payment_status='paid';s.metadata={...s.metadata,terms_version:'2026-10-08.1',seller_postal_address:'Edited metadata address'};
  assert.equal((await worker.fetch(await signed(event(s)),fixtureEnv(DB),{})).status,200);
  assert(mail.text.includes('Later fixture address'));assert(!mail.text.includes('Edited metadata address'));assert(mail.text.includes('Accepted terms version: '+CURRENT_TERMS_VERSION));
 });}finally{DB.connection.close();}
});

test('Cloudflare confirms a paid order only after the intended recipient is accepted with the original terms attachment',async()=>{
 const DB=fixtureDB(),s=session();let sent;
 try{await mockProviders(async(url,options)=>{
  if(url==='https://api.stripe.com/v1/checkout/sessions/'+s.id)return jsonResponse(s);
  assert.equal(url,'https://api.cloudflare.com/client/v4/accounts/'+'a'.repeat(32)+'/email/sending/send');
  assert.equal(options.headers.Authorization,'Bearer fixture-cloudflare-token');assert.equal(options.headers['Idempotency-Key'],undefined);sent=JSON.parse(options.body);
  return cloudflareResult({delivered:[],queued:['BUYER@example.com']});
 },async()=>{
  assert(ready(cloudflareEnv(DB)));assert.equal((await worker.fetch(await signed(event(s)),cloudflareEnv(DB),{})).status,200);
  assert.deepEqual(sent.from,{address:'support@glimdock.com',name:'Glimdock'});assert.equal(sent.reply_to,'support@glimdock.com');assert.deepEqual(sent.to,['buyer@example.com']);assert.match(sent.headers['X-Glimdock-Message-Key'],/^[a-f0-9]{64}$/);
  assert.equal(sent.attachments[0].type,'text/html');assert.equal(sent.attachments[0].disposition,'attachment');assert.equal(sent.attachments[0].filename,'Glimdock-preorder-terms.html');assert(Buffer.from(sent.attachments[0].content,'base64').toString('utf8').includes('Accepted fixture address &lt;Unit A&gt;'));
  assert(DB.connection.prepare('SELECT confirmed_at FROM orders').get().confirmed_at>0);
 });}finally{DB.connection.close();}
});

test('Cloudflare HTTP success without recipient acceptance leaves confirmation pending and retries through operations',async()=>{
 const DB=fixtureDB(),s=session();let answer=cloudflareResult({delivered:[],permanent_bounces:['buyer@example.com']}),attempts=0;
 try{await mockProviders(async(url)=>{
  if(url==='https://api.stripe.com/v1/checkout/sessions/'+s.id)return jsonResponse(s);
  assert(url.startsWith('https://api.cloudflare.com/'));attempts++;return answer;
 },async()=>{
  assert.equal((await worker.fetch(await signed(event(s)),cloudflareEnv(DB),{})).status,503);assert.equal(DB.connection.prepare('SELECT confirmed_at FROM orders').get().confirmed_at,null);
  for(const result of [{delivered:[],queued:[]},{delivered:['other@example.com']},{suppressed_recipients:['buyer@example.com']},{permanent_bounces:['BUYER@EXAMPLE.COM']}]){
   answer=cloudflareResult(result);DB.connection.prepare('UPDATE orders SET confirmation_next_attempt_at=0').run();const data=await (await worker.fetch(new Request(origin+'/api/admin/retry-confirmations',{method:'POST',headers:{Authorization:'Bearer '+token}}),cloudflareEnv(DB),{})).json();assert.equal(data.sent,0);assert.equal(data.pending,1);assert.equal(DB.connection.prepare('SELECT confirmed_at FROM orders').get().confirmed_at,null);
  }
  answer=jsonResponse({success:false,result:{delivered:['buyer@example.com']}});DB.connection.prepare('UPDATE orders SET confirmation_next_attempt_at=0').run();assert.equal((await worker.fetch(await signed(event(s)),cloudflareEnv(DB),{})).status,503);
  answer=cloudflareResult();DB.connection.prepare('UPDATE orders SET confirmation_next_attempt_at=0').run();const data=await (await worker.fetch(new Request(origin+'/api/admin/retry-commerce',{method:'POST',headers:{Authorization:'Bearer '+token}}),cloudflareEnv(DB),{})).json();assert.equal(data.confirmations.sent,1);assert.equal(data.confirmations.pending,0);assert.equal(attempts,7);
 });}finally{DB.connection.close();}
});

test('Cloudflare withdrawal acknowledgements use the same adapter and can retry after provider errors',async()=>{
 const DB=fixtureDB(),s=session(),now=Math.floor(Date.now()/1000);let accepted=false;const sent=[];
 // Confirm the order first with the same real storage path, then submit a notice.
 try{await mockProviders(async(url,options)=>{
  if(url==='https://api.stripe.com/v1/checkout/sessions/'+s.id)return jsonResponse(s);
  assert(url.startsWith('https://api.cloudflare.com/'));const body=JSON.parse(options.body);sent.push(body);
  if(body.subject.startsWith('Your Glimdock'))return cloudflareResult();
  return accepted?cloudflareResult():new Response('Unavailable',{status:503});
 },async()=>{
  assert.equal((await worker.fetch(await signed(event(s)),cloudflareEnv(DB),{})).status,200);
  const notice={id:'withdraw-'+crypto.randomUUID(),name:'Fixture Buyer',email:'buyer@example.com',reference:'GD-'+s.id.slice(-16).toUpperCase(),confirm:true};
  let response=await worker.fetch(post('/api/withdrawal',notice),cloudflareEnv(DB),{}),data=await response.json();assert.equal(response.status,200);assert.equal(data.submitted,true);assert.equal(data.emailAcknowledged,false);assert.equal(DB.connection.prepare('SELECT acknowledged_at FROM withdrawals').get().acknowledged_at,null);
  accepted=true;DB.connection.prepare('UPDATE withdrawals SET next_attempt_at=0').run();response=await worker.fetch(admin(),cloudflareEnv(DB),{});data=await response.json();assert.equal(data.sent,1);assert.equal(data.pending,0);assert(DB.connection.prepare('SELECT acknowledged_at FROM withdrawals').get().acknowledged_at>=now);
  assert.deepEqual(sent[1],sent[2]);assert.equal(sent[1].reply_to,'support@glimdock.com');assert.equal(sent[1].attachments,undefined);assert.match(sent[1].headers['X-Glimdock-Message-Key'],/^[a-f0-9]{64}$/);
 });}finally{DB.connection.close();}
});

test('Cloudflare background retries run without any Resend credential',async()=>{
 const DB=fixtureDB(),now=Math.floor(Date.now()/1000);DB.connection.prepare('INSERT INTO withdrawals (id,reference,email,name,submitted_at) VALUES (?,?,?,?,?)').run('withdraw-fixture-cloudflare','GD-1234567890ABCDEF','buyer@example.com','Fixture Buyer',now);let calls=0;
 try{await mockProviders(async(url)=>{assert(url.startsWith('https://api.cloudflare.com/'));calls++;return cloudflareResult();},async()=>{
  const work=[];const response=await worker.fetch(new Request(origin+'/api/commerce'),cloudflareEnv(DB),{waitUntil:promise=>work.push(promise)});assert.equal(response.status,200);await Promise.all(work);assert.equal(work.length,1);assert.equal(calls,1);assert(DB.connection.prepare('SELECT acknowledged_at FROM withdrawals').get().acknowledged_at>0);
 });}finally{DB.connection.close();}
});

test('custom colors persist through checkout, same-intent conflicts, order storage, status and confirmation email',async()=>{
 const DB=fixtureDB(),custom={...cart,bodyColor:' Black ',standColor:'Warm white'},s={...session(),status:'open',payment_status:'unpaid',url:'https://checkout.stripe.com/c/pay/custom'};let creates=0,mail;
 try{await mockProviders(async(url,options)=>{
  if(url==='https://api.stripe.com/v1/checkout/sessions'){
   creates++;const p=new URLSearchParams(options.body);assert.equal(p.get('metadata[body_color]'),'Black');assert.equal(p.get('metadata[stand_color]'),'Warm white');assert.equal(p.get('line_items[0][price_data][unit_amount]'),'9900');s.metadata={...s.metadata,body_color:p.get('metadata[body_color]'),stand_color:p.get('metadata[stand_color]')};return jsonResponse(s);
  }
  if(url==='https://api.stripe.com/v1/checkout/sessions/'+s.id)return jsonResponse(s);
  assert.equal(url,'https://api.resend.com/emails');mail=JSON.parse(options.body);return jsonResponse({id:'mail_fixture'});
 },async()=>{
  const env=fixtureEnv(DB);assert.equal((await worker.fetch(post('/api/checkout',custom),env,{})).status,200);
  const row=DB.connection.prepare('SELECT * FROM checkout_attempts').get();assert.equal(row.body_color,'Black');assert.equal(row.stand_color,'Warm white');
  assert.equal((await worker.fetch(post('/api/checkout',{...custom,bodyColor:'Black'}),env,{})).status,200);
  for(const change of [{bodyColor:'Purple'},{standColor:'White'}])assert.equal((await worker.fetch(post('/api/checkout',{...custom,...change}),env,{})).status,409);assert.equal(creates,1);
  s.status='complete';s.payment_status='paid';const response=await worker.fetch(new Request(origin+'/api/order-status?session_id='+s.id),env,{});assert.equal(response.status,200);const data=await response.json();assert.equal(data.bodyColor,'Black');assert.equal(data.standColor,'Warm white');assert.equal(data.amount,21300);
  const order=DB.connection.prepare('SELECT * FROM orders').get();assert.equal(order.body_color,'Black');assert.equal(order.stand_color,'Warm white');assert.equal(order.payment_status,'paid');assert(mail.text.includes('Body color: Black\nStand color: Warm white'));
 });}finally{DB.connection.close();}
});

test('invalid color descriptions fail before checkout storage or providers',async()=>{
 const DB=fixtureDB();
 try{await mockProviders(()=>{throw Error('No provider calls expected');},async()=>{
  for(const change of [{bodyColor:'<script>'},{standColor:'white\nblack'},{bodyColor:'x'.repeat(65)}])assert.equal((await worker.fetch(post('/api/checkout',{...cart,...change}),fixtureEnv(DB),{})).status,400);
  assert.equal(DB.connection.prepare('SELECT COUNT(*) AS n FROM checkout_attempts').get().n,0);
 });}finally{DB.connection.close();}
});

test('concurrent Cloudflare withdrawal submissions claim one acknowledgement attempt',async()=>{
 const DB=fixtureDB(),s=session();let acknowledgementCalls=0,releaseMail,mailStarted;const started=new Promise(resolve=>mailStarted=resolve),held=new Promise(resolve=>releaseMail=resolve);
 try{await mockProviders(async(url,options)=>{
  if(url==='https://api.stripe.com/v1/checkout/sessions/'+s.id)return jsonResponse(s);
  const body=JSON.parse(options.body);if(body.subject.startsWith('Your Glimdock'))return cloudflareResult();
  acknowledgementCalls++;mailStarted();await held;return cloudflareResult();
 },async()=>{
  assert.equal((await worker.fetch(await signed(event(s)),cloudflareEnv(DB),{})).status,200);
  const notice={id:'withdraw-'+crypto.randomUUID(),name:'Fixture Buyer',email:'buyer@example.com',reference:'GD-'+s.id.slice(-16).toUpperCase(),confirm:true};
  const responses=Promise.all([worker.fetch(post('/api/withdrawal',notice),cloudflareEnv(DB),{}),worker.fetch(post('/api/withdrawal',notice),cloudflareEnv(DB),{})]);await started;releaseMail();for(const response of await responses)assert.equal(response.status,200);
  assert.equal(acknowledgementCalls,1);const row=DB.connection.prepare('SELECT * FROM withdrawals').get();assert.equal(row.attempt_count,1);assert(row.acknowledged_at>0);
 });}finally{releaseMail?.();DB.connection.close();}
});
