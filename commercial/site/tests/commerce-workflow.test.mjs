import test from 'node:test';
import assert from 'node:assert/strict';
import {DatabaseSync} from 'node:sqlite';
import {readFileSync,readdirSync} from 'node:fs';
import worker from '../dist/server/index.js';
import {CURRENT_TERMS_VERSION,acceptedTermsHTML} from '../worker/accepted-terms.js';

const origin='https://glimdock.com';
const token='fixture-operations-token-32-characters';
const secret='whsec_fixture_not_a_real_secret';
const cart={quantity:2,country:'DE',terms:true,intent:'f4e0a806-04e2-478f-a10f-2b610a0f3e24'};
const session=()=>({id:'cs_live_fixture1234567890ABCDEF',mode:'payment',currency:'eur',payment_status:'paid',amount_total:21300,created:1791500000,customer_details:{email:'buyer@example.com'},metadata:{quantity:'2',country:'DE',product:'glimdock-desk-first-batch',terms_accepted:'true',terms_version:CURRENT_TERMS_VERSION,seller_postal_address:'Accepted fixture address <Unit A>'}});
const fixtureEnv=DB=>({DB,CHECKOUT_ENABLED:'true',STRIPE_SECRET_KEY:'sk_live_fixture_only',STRIPE_WEBHOOK_SECRET:secret,RESEND_API_KEY:'re_fixture',SELLER_POSTAL_ADDRESS:'Later fixture address',SUPPORT_READY:'true',PRODUCTION_READY:'true',ORDER_CONFIRMATIONS_READY:'true',OPERATIONS_READY:'true',OPERATIONS_TOKEN:token});
const post=(path,body,headers={})=>new Request(origin+path,{method:'POST',headers:{Origin:origin,'Content-Type':'application/json',...headers},body:JSON.stringify(body)});

// Exercise the production migrations and SQL using SQLite, with the same
// asynchronous statement shape used by the Sites D1 binding.
function fixtureDB(){
 const connection=new DatabaseSync(':memory:');
 for(const file of readdirSync(new URL('../drizzle/',import.meta.url)).filter(x=>/^\d{4}.*\.sql$/.test(x)).sort())connection.exec(readFileSync(new URL('../drizzle/'+file,import.meta.url),'utf8'));
 return {connection,prepare(sql){const statement=connection.prepare(sql);let values=[];return {bind(...x){values=x;return this;},async first(){return statement.get(...values)||null;},async all(){return {results:statement.all(...values)};},async run(){const result=statement.run(...values);return {meta:{changes:Number(result.changes)}};}};}};
}
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
 try{await mockProviders(async(url,options)=>{assert.equal(url,'https://api.stripe.com/v1/checkout/sessions');sent=new URLSearchParams(options.body);return jsonResponse({url:'https://checkout.stripe.com/c/pay/fixture'});},async()=>{
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
  if(url==='https://api.stripe.com/v1/checkout/sessions/'+s.id)return jsonResponse(s);
  assert.equal(url,'https://api.resend.com/emails');messages.push({key:options.headers['Idempotency-Key'],body:JSON.parse(options.body)});
  return ++attempts===1?new Response('mail outage',{status:503}):jsonResponse({id:'email_fixture'});
 },async()=>{
  assert.equal((await worker.fetch(await signed(event(s)),fixtureEnv(DB),{})).status,503);
  let row=DB.connection.prepare('SELECT * FROM orders').get();assert.equal(row.payment_status,'paid');assert.equal(row.confirmed_at,null);assert.equal(typeof row.email_hash,'string');
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
  mailOK=true;r=await worker.fetch(request(),fixtureEnv(DB),{});assert.equal(r.status,200);data=await r.json();assert.equal(data.emailConfirmed,true);
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
