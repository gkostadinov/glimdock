import {createInterestService} from './interest.js';
import {CURRENT_TERMS_VERSION, acceptedTermsHTML} from './accepted-terms.js';
const ORIGINS=new Set(['https://glimdock.com','https://www.glimdock.com','https://glimdock.docile-guppy-9289.chatgpt.site','http://127.0.0.1:8772']);
const COUNTRIES=new Set('AT BE BG HR CY CZ DK EE FI FR DE GR HU IE IT LV LT LU MT NL PL PT RO SK SI ES SE'.split(' '));
const HEADERS={'X-Content-Type-Options':'nosniff','Referrer-Policy':'strict-origin-when-cross-origin','Permissions-Policy':'camera=(), microphone=(), geolocation=(), serial=()','Content-Security-Policy':"default-src 'self'; script-src 'self' https://www.googletagmanager.com; img-src 'self' data: https://www.googletagmanager.com https://*.google-analytics.com https://*.google.com; connect-src 'self' https://*.google-analytics.com https://*.analytics.google.com https://*.googletagmanager.com https://*.googleadservices.com https://*.doubleclick.net; style-src 'self'; font-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self' https://checkout.stripe.com"};
const json=(value,status=200)=>new Response(JSON.stringify(value),{status,headers:{...HEADERS,'Content-Type':'application/json; charset=utf-8','Cache-Control':'no-store'}});
const shipping=country=>country==='BG'?500:1500;
// Keep request and response contracts aligned with the configured webhook version.
const STRIPE_API_VERSION='2026-09-30.endive';
const SESSION_ID=/^cs_(test_|live_)?[A-Za-z0-9]+$/;
const UUID=/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;
function color(value,fallback){if(value===undefined)return fallback;if(typeof value!=='string'||/[<>\u0000-\u001f\u007f]/.test(value))return null;const normalized=value.trim();return normalized&&Array.from(normalized).length<=64?normalized:null;}
const cartColors=x=>({body:color(x?.bodyColor,'Purple'),stand:color(x?.standColor,'White')});
const sessionColors=s=>({body:color(s?.metadata?.body_color,'Purple'),stand:color(s?.metadata?.stand_color,'White')});
class RequestError extends Error{constructor(message,status){super(message);this.status=status;}}
export function operationsReady(env){return env.OPERATIONS_READY==='true'&&typeof env.OPERATIONS_TOKEN==='string'&&env.OPERATIONS_TOKEN.length>=32;}
export function ready(env){return operationsReady(env)&&env.CHECKOUT_ENABLED==='true'&&/^(sk|rk)_live_/.test(env.STRIPE_SECRET_KEY||'')&&!!env.STRIPE_WEBHOOK_SECRET&&!!env.DB&&mailReady(env)&&typeof env.SELLER_POSTAL_ADDRESS==='string'&&!!env.SELLER_POSTAL_ADDRESS.trim()&&env.SELLER_POSTAL_ADDRESS.length<=500&&env.SUPPORT_READY==='true'&&env.PRODUCTION_READY==='true'&&env.ORDER_CONFIRMATIONS_READY==='true';}
const db=env=>{if(!env.DB)throw Error('Storage unavailable');return env.DB;};
const interestService=createInterestService({db,mailReady,sendMail,json,bodyJSON,origins:ORIGINS,countries:COUNTRIES,color});
async function stripe(env,path,options={}){const r=await fetch('https://api.stripe.com/v1/'+path,{signal:AbortSignal.timeout(15000),...options,headers:{Authorization:'Bearer '+env.STRIPE_SECRET_KEY,'Stripe-Version':STRIPE_API_VERSION,...options.headers}});if(!r.ok)throw Error('Payment provider unavailable');return r.json();}
export function validCart(x){return x&&Number.isInteger(x.quantity)&&x.quantity>=1&&x.quantity<=5&&COUNTRIES.has(x.country)&&x.terms===true&&!!cartColors(x).body&&!!cartColors(x).stand&&UUID.test(x.intent||'');}
export function sessionParams(x,origin,sellerAddress=''){const colors=cartColors(x);const p=new URLSearchParams({mode:'payment','allowed_payment_method_types[0]':'card','line_items[0][price_data][currency]':'eur','line_items[0][price_data][unit_amount]':'9900','line_items[0][price_data][product_data][name]':'Glimdock desk monitor — preorder','line_items[0][price_data][product_data][description]':'Body: '+colors.body+'. Stand: '+colors.stand+'. USB-C cable. Estimated delivery 4–6 weeks from payment.','line_items[0][quantity]':String(x.quantity),'shipping_address_collection[allowed_countries][0]':x.country,'shipping_options[0][shipping_rate_data][type]':'fixed_amount','shipping_options[0][shipping_rate_data][fixed_amount][amount]':String(shipping(x.country)),'shipping_options[0][shipping_rate_data][fixed_amount][currency]':'eur','shipping_options[0][shipping_rate_data][display_name]':x.country==='BG'?'Bulgaria delivery':'EU delivery','success_url':origin+'/order/complete/?session_id={CHECKOUT_SESSION_ID}','cancel_url':origin+'/order/?cancelled=1','metadata[product]':'glimdock-desk-first-batch','metadata[quantity]':String(x.quantity),'metadata[country]':x.country,'metadata[body_color]':colors.body,'metadata[stand_color]':colors.stand,'metadata[terms_version]':CURRENT_TERMS_VERSION,'metadata[seller_postal_address]':sellerAddress,'metadata[terms_accepted]':'true','custom_text[submit][message]':'Preorder. Estimated delivery 4–6 weeks from payment. Seller is not VAT registered. Terms: https://glimdock.com/terms/'});return p;}
export function verifiedSession(s){if(!s||typeof s!=='object'||Array.isArray(s))return false;const q=Number(s.metadata?.quantity),c=s.metadata?.country;return s.mode==='payment'&&s.currency==='eur'&&s.metadata?.product==='glimdock-desk-first-batch'&&Number.isInteger(q)&&q>=1&&q<=5&&COUNTRIES.has(c)&&s.amount_total===q*9900+shipping(c)&&s.metadata?.terms_accepted==='true'&&!!sessionColors(s).body&&!!sessionColors(s).stand;}
const reference=s=>'GD-'+s.id.slice(-16).toUpperCase();
async function emailHash(email){return [...new Uint8Array(await crypto.subtle.digest('SHA-256',new TextEncoder().encode(String(email||'').trim().toLowerCase())))].map(x=>x.toString(16).padStart(2,'0')).join('');}
export function withdrawalReady(env){return !!env.DB;}
function cloudflareSender(value){
 if(typeof value!=='string'||/[\r\n]/.test(value))return null;
 const named=value.match(/^\s*([^<>]+?)\s*<([^<>\s]+)>\s*$/);
 const address=named?named[2]:value.trim();
 if(!/^[^\s<>@]+@[^\s<>@]+\.[^\s<>@]+$/.test(address))return null;
 return named?{address,name:named[1].trim().replace(/^"(.*)"$/,'$1')}:address;
}
export function mailReady(env){
 const provider=env.MAIL_PROVIDER||'resend';
 if(provider==='cloudflare')return /^[a-f0-9]{32}$/i.test(env.CLOUDFLARE_EMAIL_ACCOUNT_ID||'')&&typeof env.CLOUDFLARE_EMAIL_API_TOKEN==='string'&&!!env.CLOUDFLARE_EMAIL_API_TOKEN.trim()&&!!cloudflareSender(env.MAIL_FROM||'Glimdock <support@glimdock.com>');
 return provider==='resend'&&typeof env.RESEND_API_KEY==='string'&&!!env.RESEND_API_KEY.trim();
}
async function sendMail(env,key,body){
 if(!mailReady(env))throw Error('Mail unavailable');
 const message={from:env.MAIL_FROM||'Glimdock <support@glimdock.com>',reply_to:'support@glimdock.com',...body};
 if(env.MAIL_PROVIDER==='cloudflare'){
  const payload={...message,from:cloudflareSender(message.from),headers:{'X-Glimdock-Message-Key':await emailHash(key)}};
  if(message.attachments)payload.attachments=message.attachments.map(a=>({...a,type:a.type||(/\.html?$/i.test(a.filename)?'text/html':'application/octet-stream'),disposition:a.disposition||'attachment'}));
  // Cloudflare has no documented idempotency key. This header is correlation
  // only; SQL leases prevent concurrent sends, but a lost response can be retried.
  const response=await fetch('https://api.cloudflare.com/client/v4/accounts/'+env.CLOUDFLARE_EMAIL_ACCOUNT_ID+'/email/sending/send',{method:'POST',signal:AbortSignal.timeout(15000),headers:{Authorization:'Bearer '+env.CLOUDFLARE_EMAIL_API_TOKEN,'Content-Type':'application/json'},body:JSON.stringify(payload)});
  if(!response.ok)throw Error('Mail unavailable');
  const result=await response.json();
  const listed=(list,email)=>Array.isArray(list)&&list.some(address=>typeof address==='string'&&address.trim().toLowerCase()===email);
  const recipients=Array.isArray(message.to)?message.to:[message.to];
  if(result?.success!==true||!recipients.length||!recipients.every(address=>{
   if(typeof address!=='string')return false;const email=address.trim().toLowerCase();
   return (listed(result.result?.delivered,email)||listed(result.result?.queued,email))&&!listed(result.result?.permanent_bounces,email)&&!listed(result.result?.suppressed_recipients,email);
  }))throw Error('Mail recipient not accepted');
  return;
 }
 const response=await fetch('https://api.resend.com/emails',{method:'POST',signal:AbortSignal.timeout(15000),headers:{Authorization:'Bearer '+env.RESEND_API_KEY,'Content-Type':'application/json','Idempotency-Key':key},body:JSON.stringify(message)});
 if(!response.ok)throw Error('Mail unavailable');
 const result=await response.json();if(typeof result?.id!=='string'||!result.id)throw Error('Mail not accepted');
}
// A session's product/price and its lifecycle are separate checks: completion alone
// is never evidence that payment succeeded.
function validSession(s,id=s?.id){return SESSION_ID.test(id||'')&&s?.id===id&&verifiedSession(s)&&['open','complete','expired'].includes(s.status)&&['paid','unpaid'].includes(s.payment_status)&&!(s.payment_status==='paid'&&s.status!=='complete')&&Number.isInteger(s.created)&&s.created>0;}
function modeMatches(env,s){return /^(sk|rk)_live_/.test(env.STRIPE_SECRET_KEY||'')?s.livemode===true:s.livemode===false;}
async function claimConfirmation(env,id){
 const now=Math.floor(Date.now()/1000);
 const saved=await db(env).prepare('SELECT * FROM orders WHERE session_id=?').bind(id).first();
 if(!saved||saved.confirmed_at||saved.payment_status!=='paid')return {saved,claimed:false};
 const lease=await db(env).prepare("UPDATE orders SET confirmation_next_attempt_at=?, confirmation_attempt_count=confirmation_attempt_count+1 WHERE session_id=? AND payment_status='paid' AND confirmed_at IS NULL AND confirmation_next_attempt_at<=?").bind(now+300,id,now).run();
 return {saved,claimed:!!lease.meta?.changes};
}
async function deliverConfirmation(env,saved,session){
 try{
  const s=session||await stripe(env,'checkout/sessions/'+encodeURIComponent(saved.session_id));
  if(!validSession(s,saved.session_id)||!modeMatches(env,s)||s.payment_status!=='paid'||saved.quantity!==Number(s.metadata.quantity)||saved.country!==s.metadata.country||saved.amount!==s.amount_total||saved.body_color!==sessionColors(s).body||saved.stand_color!==sessionColors(s).stand||!s.customer_details?.email)throw Error('Order verification needed');
  const hash=await emailHash(s.customer_details.email);
  if(saved.email_hash&&saved.email_hash!==hash)throw Error('Order email changed');
  // Existing rows can be backfilled once; accepted snapshots never change on retry.
  await db(env).prepare('UPDATE orders SET email_hash=COALESCE(email_hash,?), terms_version=COALESCE(terms_version,?), seller_postal_address=COALESCE(seller_postal_address,?) WHERE session_id=?').bind(hash,s.metadata.terms_version||null,s.metadata.seller_postal_address||null,s.id).run();
  const row=await db(env).prepare('SELECT * FROM orders WHERE session_id=?').bind(s.id).first();
  const metadata={terms_version:row.terms_version,seller_postal_address:row.seller_postal_address};
  const termsHTML=acceptedTermsHTML(metadata);
  await sendMail(env,'order-'+s.id,{to:[s.customer_details.email.trim().toLowerCase()],subject:'Your Glimdock preorder — '+row.reference,text:'Your Glimdock preorder is confirmed.\nOrder: '+row.reference+'\nQuantity: '+row.quantity+' × EUR 99.00\nBody color: '+row.body_color+'\nStand color: '+row.stand_color+'\nShipping: EUR '+(shipping(row.country)/100).toFixed(2)+' to '+row.country+'\nTotal paid: EUR '+(row.amount/100).toFixed(2)+'\nNo VAT charged; seller is not VAT registered.\nEstimated delivery: 4–6 weeks from payment. We will email you if the schedule changes.\nSeller: Georgi Kostadinov\n'+row.seller_postal_address+'\nsupport@glimdock.com\nYou may request cancellation before dispatch. Your standard EU consumer withdrawal rights remain in place. Withdraw: https://glimdock.com/withdraw/\nAccepted terms version: '+row.terms_version,attachments:[{filename:'Glimdock-preorder-terms.html',content:btoa(unescape(encodeURIComponent(termsHTML)))}]});
  await db(env).prepare('UPDATE orders SET confirmed_at=?, confirmation_next_attempt_at=0 WHERE session_id=?').bind(Math.floor(Date.now()/1000),s.id).run();
  return true;
 }catch{
  const next=Math.floor(Date.now()/1000)+Math.min(7200,60*2**Math.min(saved.confirmation_attempt_count,7));
  await db(env).prepare('UPDATE orders SET confirmation_next_attempt_at=? WHERE session_id=? AND confirmed_at IS NULL').bind(next,saved.session_id).run();
  return false;
 }
}
async function recordOrder(env,s){
 if(!validSession(s)||!modeMatches(env,s)||s.payment_status!=='paid')throw Error('Unrecognized paid order');
 const attempt=await db(env).prepare('SELECT * FROM checkout_attempts WHERE session_id=?').bind(s.id).first();
 if(attempt&&(attempt.quantity!==Number(s.metadata.quantity)||attempt.country!==s.metadata.country||attempt.body_color!==sessionColors(s).body||attempt.stand_color!==sessionColors(s).stand))throw Error('Checkout cart mismatch');
 const accepted=attempt?{terms_version:attempt.terms_version,seller_postal_address:attempt.seller_postal_address}:s.metadata;
 const hash=s.customer_details?.email?await emailHash(s.customer_details.email):null;
 await db(env).prepare("INSERT INTO orders (session_id, reference, payment_status, quantity, country, amount, created_at, email_hash, terms_version, seller_postal_address, body_color, stand_color) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(session_id) DO UPDATE SET payment_status=CASE WHEN orders.payment_status='paid' THEN 'paid' ELSE excluded.payment_status END, email_hash=COALESCE(orders.email_hash,excluded.email_hash), terms_version=COALESCE(orders.terms_version,excluded.terms_version), seller_postal_address=COALESCE(orders.seller_postal_address,excluded.seller_postal_address)").bind(s.id,reference(s),s.payment_status,Number(s.metadata.quantity),s.metadata.country,s.amount_total,s.created,hash,accepted.terms_version||null,accepted.seller_postal_address||null,sessionColors(s).body,sessionColors(s).stand).run();
 const {saved,claimed}=await claimConfirmation(env,s.id);
 return saved?.confirmed_at?true:claimed?deliverConfirmation(env,saved,s):false;
}
async function confirmationCounts(env,now){
 if(!env.DB)return {pending:0,due:0};
 const row=await db(env).prepare("SELECT COUNT(*) AS pending, COALESCE(SUM(CASE WHEN confirmation_next_attempt_at<=? THEN 1 ELSE 0 END),0) AS due FROM orders WHERE payment_status='paid' AND confirmed_at IS NULL").bind(now).first();
 return {pending:Number(row?.pending||0),due:Number(row?.due||0)};
}
async function retryConfirmations(env){
 let sent=0;const now=Math.floor(Date.now()/1000);
 if(env.DB&&mailReady(env)&&env.STRIPE_SECRET_KEY){
  const result=await db(env).prepare("SELECT session_id FROM orders WHERE payment_status='paid' AND confirmed_at IS NULL AND confirmation_next_attempt_at<=? ORDER BY confirmation_next_attempt_at,created_at LIMIT 20").bind(now).all();
  for(const row of result.results||[]){const {saved,claimed}=await claimConfirmation(env,row.session_id);if(claimed&&await deliverConfirmation(env,saved))sent++;}
 }
 return {sent,...await confirmationCounts(env,Math.floor(Date.now()/1000))};
}
async function createCheckout(env,x,origin){
 const now=Math.floor(Date.now()/1000),colors=cartColors(x);
 await db(env).prepare('INSERT INTO checkout_attempts (intent,quantity,country,terms_version,seller_postal_address,created_at,checkout_origin,body_color,stand_color) VALUES (?,?,?,?,?,?,?,?,?) ON CONFLICT(intent) DO NOTHING').bind(x.intent,x.quantity,x.country,CURRENT_TERMS_VERSION,env.SELLER_POSTAL_ADDRESS,now,origin,colors.body,colors.stand).run();
 const attempt=await db(env).prepare('SELECT * FROM checkout_attempts WHERE intent=?').bind(x.intent).first();
 if(attempt.quantity!==x.quantity||attempt.country!==x.country||attempt.body_color!==colors.body||attempt.stand_color!==colors.stand)throw new RequestError('Your cart changed. Please review it and try again.',409);
 if(attempt.session_id){
  const existing=await stripe(env,'checkout/sessions/'+encodeURIComponent(attempt.session_id));
  if(!validSession(existing,attempt.session_id)||!modeMatches(env,existing)||Number(existing.metadata.quantity)!==attempt.quantity||existing.metadata.country!==attempt.country||sessionColors(existing).body!==attempt.body_color||sessionColors(existing).stand!==attempt.stand_color)throw Error('Checkout verification unavailable');
  if(existing.status==='complete')return {completed:true,sessionId:existing.id};
  if(existing.status==='expired')return {restartCheckout:true,error:'Your checkout expired. Review your order and try again.'};
  if(!existing.url||new URL(existing.url).origin!=='https://checkout.stripe.com')throw Error('Checkout URL unavailable');
  return {url:existing.url,sessionId:existing.id};
 }
 // Stripe may prune idempotency keys after 24 hours. Never re-create an unresolved
 // attempt outside that window, even when the first response was lost.
 if(now-attempt.created_at>=23*3600)throw new RequestError('This checkout could not be recovered. Please contact support before paying again.',409);
 const params=sessionParams(x,attempt.checkout_origin,attempt.seller_postal_address);
 params.set('metadata[terms_version]',attempt.terms_version);params.set('client_reference_id',x.intent);
 const s=await stripe(env,'checkout/sessions',{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded','Idempotency-Key':'glimdock-'+x.intent},body:params});
 if(!SESSION_ID.test(s.id||'')||!s.url||new URL(s.url).origin!=='https://checkout.stripe.com')throw Error('Checkout response unavailable');
 await db(env).prepare('UPDATE checkout_attempts SET session_id=? WHERE intent=? AND (session_id IS NULL OR session_id=?)').bind(s.id,x.intent,s.id).run();
 return {url:s.url,sessionId:s.id};
}
export async function verifySignature(body,header,secret,now=Date.now()){const fields=(header||'').split(',').map(x=>x.split('='));const t=fields.find(x=>x[0]==='t')?.[1];const sigs=fields.filter(x=>x[0]==='v1').map(x=>x[1]);if(!/^\d+$/.test(t||'')||Math.abs(now/1000-Number(t))>300)return false;const key=await crypto.subtle.importKey('raw',new TextEncoder().encode(secret),{name:'HMAC',hash:'SHA-256'},false,['sign']);const result=await crypto.subtle.sign('HMAC',key,new TextEncoder().encode(t+'.'+body));const expected=[...new Uint8Array(result)].map(n=>n.toString(16).padStart(2,'0')).join('');return sigs.some(s=>{if(s.length!==expected.length)return false;let d=0;for(let i=0;i<s.length;i++)d|=s.charCodeAt(i)^expected.charCodeAt(i);return d===0;});}
async function bodyJSON(req){
 const reader=req.body?.getReader();let size=0;const chunks=[];
 if(reader)for(;;){const {done,value}=await reader.read();if(done)break;size+=value.byteLength;if(size>4096){await reader.cancel();throw new RequestError('Request is too large.',413);}chunks.push(value);}
 const bytes=new Uint8Array(size);let offset=0;for(const chunk of chunks){bytes.set(chunk,offset);offset+=chunk.byteLength;}
 try{return JSON.parse(new TextDecoder().decode(bytes));}catch{throw new RequestError('Invalid JSON request.',400);}
}
async function acknowledgement(env,w){
 await sendMail(env,'withdrawal-'+w.id,{to:[w.email],subject:'Glimdock withdrawal received — '+w.reference,text:'We received your notice of withdrawal for order '+w.reference+' at '+new Date(w.submitted_at*1000).toISOString()+'.\nRequest reference: '+w.id+'\nWe will contact you with any returns instructions.\nGlimdock — Georgi Kostadinov\n'+env.SELLER_POSTAL_ADDRESS+'\nsupport@glimdock.com'});
 await db(env).prepare('UPDATE withdrawals SET acknowledged_at=? WHERE id=?').bind(Math.floor(Date.now()/1000),w.id).run();
}
async function attemptAcknowledgement(env,w){
 if(w.acknowledged_at)return {sent:false,acknowledged:true};
 if(!mailReady(env))return {sent:false,acknowledged:false};
 const now=Math.floor(Date.now()/1000);
 const lease=await db(env).prepare('UPDATE withdrawals SET next_attempt_at=?, attempt_count=attempt_count+1 WHERE id=? AND acknowledged_at IS NULL AND next_attempt_at<=?').bind(now+300,w.id,now).run();
 if(!lease.meta?.changes){const saved=await db(env).prepare('SELECT acknowledged_at FROM withdrawals WHERE id=?').bind(w.id).first();return {sent:false,acknowledged:!!saved?.acknowledged_at};}
 try{await acknowledgement(env,w);return {sent:true,acknowledged:true};}
 catch{await db(env).prepare('UPDATE withdrawals SET next_attempt_at=? WHERE id=? AND acknowledged_at IS NULL').bind(now+Math.min(7200,60*2**Math.min(w.attempt_count,7)),w.id).run();return {sent:false,acknowledged:false};}
}
async function withdrawalCounts(env,now){
 if(!env.DB)return {pending:0,due:0};
 const row=await db(env).prepare('SELECT COUNT(*) AS pending, COALESCE(SUM(CASE WHEN next_attempt_at<=? THEN 1 ELSE 0 END),0) AS due FROM withdrawals WHERE acknowledged_at IS NULL').bind(now).first();
 return {pending:Number(row?.pending||0),due:Number(row?.due||0)};
}
async function retryWithdrawals(env){
 const now=Math.floor(Date.now()/1000);let sent=0;
 if(env.DB&&mailReady(env)){
  const result=await db(env).prepare('SELECT * FROM withdrawals WHERE acknowledged_at IS NULL AND next_attempt_at<=? ORDER BY next_attempt_at,submitted_at LIMIT 20').bind(now).all();
  for(const w of result.results||[]){if((await attemptAcknowledgement(env,w)).sent)sent++;}
 }
 return {sent,...await withdrawalCounts(env,Math.floor(Date.now()/1000))};
}
async function retryCommerce(env){return {confirmations:await retryConfirmations(env),withdrawals:await retryWithdrawals(env),interest:await interestService.retry(env)};}
async function api(req,env,url,ctx){const path=url.pathname;
 const interestResponse=await interestService.api(req,env,url,ctx);if(interestResponse)return interestResponse;
 if(['/api/admin/retry-withdrawals','/api/admin/retry-confirmations','/api/admin/retry-commerce'].includes(path)&&req.method==='POST'){
  if(typeof env.OPERATIONS_TOKEN!=='string'||env.OPERATIONS_TOKEN.length<32||req.headers.get('Authorization')!=='Bearer '+env.OPERATIONS_TOKEN)return json({error:'Unauthorized.'},401);
  return json(await (path.endsWith('retry-withdrawals')?retryWithdrawals(env):path.endsWith('retry-confirmations')?retryConfirmations(env):retryCommerce(env)));
 }
 if(path==='/api/commerce'&&req.method==='GET')return json({checkoutReady:ready(env),withdrawalReady:withdrawalReady(env),currency:'EUR',unitPrice:9900,shippingBG:500,shippingEU:1500,vatRegistered:false});
 if(path==='/api/checkout'&&req.method==='POST'){
  if(!ORIGINS.has(url.origin)||req.headers.get('Origin')!==url.origin)return json({error:'Please use the Glimdock checkout page.'},403);
  if(!ready(env))return json({error:'Checkout is not open yet. You have not been charged.'},503);
  if(!/^application\/json(?:\s*;|$)/i.test(req.headers.get('Content-Type')||''))return json({error:'Invalid request.'},415);
  const x=await bodyJSON(req);if(!validCart(x))return json({error:'Choose 1–5 monitors, valid color descriptions, an EU destination and agree to the terms.'},400);
  try{const result=await createCheckout(env,x,url.origin);return json(result,result.restartCheckout?409:200);}catch(error){if(error instanceof RequestError)throw error;return json({error:'Secure checkout is temporarily unavailable. Please try again.'},502);}
 }
 if(path==='/api/order-status'&&req.method==='GET'){
  const id=url.searchParams.get('session_id');if(!SESSION_ID.test(id||''))return json({error:'Invalid checkout reference.'},400);
  if(!env.STRIPE_SECRET_KEY||!env.DB)return json({error:'Payment verification unavailable.'},503);
  try{
   const s=await stripe(env,'checkout/sessions/'+encodeURIComponent(id));if(!validSession(s,id)||!modeMatches(env,s))return json({error:'Unknown order.'},404);
   const emailConfirmed=s.payment_status==='paid'?await recordOrder(env,s):false;
   return json({paymentStatus:s.payment_status,checkoutStatus:s.status,reference:reference(s),emailConfirmed,quantity:Number(s.metadata.quantity),country:s.metadata.country,bodyColor:sessionColors(s).body,standColor:sessionColors(s).stand,amount:s.amount_total,currency:s.currency});
  }catch{return json({error:'Could not verify payment. Please contact support before paying again.'},502);}
 }
 if(path==='/api/stripe-webhook'&&req.method==='POST'){
  if(!env.STRIPE_WEBHOOK_SECRET||!env.DB)return json({error:'Webhook unavailable.'},503);
  const body=await req.text();
  if(body.length>1000000||!await verifySignature(body,req.headers.get('stripe-signature'),env.STRIPE_WEBHOOK_SECRET))return json({error:'Invalid signature.'},400);
  let event;try{event=JSON.parse(body);}catch{return json({error:'Invalid webhook payload.'},400);}
  if(!event||typeof event!=='object'||Array.isArray(event)||typeof event.type!=='string')return json({error:'Invalid webhook payload.'},400);
  if(['checkout.session.completed','checkout.session.async_payment_succeeded','checkout.session.async_payment_failed'].includes(event.type)){
   const s=event.data?.object;
   if(!s||typeof s!=='object'||Array.isArray(s)||!/^cs_(test_|live_)?[A-Za-z0-9]+$/.test(s.id||''))return json({error:'Invalid checkout event.'},400);
   if(verifiedSession(s)){
    const current=await stripe(env,'checkout/sessions/'+encodeURIComponent(s.id));
    if(!validSession(current,s.id)||!modeMatches(env,current))return json({error:'Payment verification retry needed.'},503);
    if(current.payment_status==='paid'&&!await recordOrder(env,current))return json({error:'Order recorded; confirmation retry needed.'},503);
   }
  }
  return json({received:true});
 }
 if(path==='/api/withdrawal'&&req.method==='POST'){if(req.headers.get('Origin')!==url.origin||!ORIGINS.has(url.origin))return json({error:'Use the Glimdock withdrawal form.'},403);if(!withdrawalReady(env))return json({error:'Online submission is temporarily unavailable. Please email support.'},503);const x=await bodyJSON(req);if(!x||typeof x!=='object'||Array.isArray(x)||typeof x.name!=='string'||!x.name.trim()||x.name.length>120||typeof x.email!=='string'||x.email.length>254||!/^\S+@\S+\.\S+$/.test(x.email)||!/^GD-[A-Z0-9]{16}$/.test(x.reference||'')||x.confirm!==true||!/^withdraw-[a-f0-9-]{36}$/.test(x.id||''))return json({error:'Enter your name, order reference and checkout email, then confirm withdrawal.'},400);const row=await db(env).prepare('SELECT session_id, email_hash FROM orders WHERE reference=?').bind(x.reference).first();if(!row)return json({error:'We could not match those order details. Check the reference or contact support.'},404);if(!row.email_hash||row.email_hash!==await emailHash(x.email))return json({error:'We could not match those order details. Check the email or contact support.'},404);const w={id:x.id,reference:x.reference,email:x.email,name:x.name.trim(),submitted_at:Math.floor(Date.now()/1000)};await db(env).prepare('INSERT INTO withdrawals (id, reference, email, name, submitted_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT(id) DO NOTHING').bind(w.id,w.reference,w.email,w.name,w.submitted_at).run();const saved=await db(env).prepare('SELECT * FROM withdrawals WHERE id=?').bind(w.id).first();if(saved.reference!==w.reference||saved.email.trim().toLowerCase()!==w.email.trim().toLowerCase())return json({error:'This request reference is already in use. Please reload the form and try again.'},409);const result=await attemptAcknowledgement(env,saved);return json({submitted:true,reference:saved.id,submittedAt:saved.submitted_at,emailAcknowledged:result.acknowledged});}
 return json({error:'Not found.'},404);
}
function escapeHTML(s){return String(s).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));}
export default{async fetch(req,env,ctx){const url=new URL(req.url);if(req.method==='GET'&&env.DB&&mailReady(env)&&ctx?.waitUntil)ctx.waitUntil(retryCommerce(env).catch(()=>{}));try{if(url.pathname==='/tracking-config.json')return json({policyVersion:'2026-10-08.1:'+String(env.GA_MEASUREMENT_ID||'')+':'+String(env.GOOGLE_ADS_ID||''),analyticsId:/^G-[A-Z0-9]+$/.test(env.GA_MEASUREMENT_ID||'')?env.GA_MEASUREMENT_ID:'',advertisingId:/^AW-\d+$/.test(env.GOOGLE_ADS_ID||'')?env.GOOGLE_ADS_ID:''});if(url.pathname.startsWith('/api/'))return await api(req,env,url,ctx);if(!['GET','HEAD'].includes(req.method))return new Response('Method not allowed',{status:405,headers:HEADERS});let key=url.pathname;if(key.endsWith('/'))key+='index.html';else if(!key.split('/').pop().includes('.'))return Response.redirect(url.origin+key+'/',308);const f=FILES[key]||FILES['/404/index.html'];if(!f)return new Response('Not found',{status:404});let data=Uint8Array.from(atob(f.data),c=>c.charCodeAt(0));if(f.type.startsWith('text/html')&&env.SELLER_POSTAL_ADDRESS){let html=new TextDecoder().decode(data);html=html.replaceAll('The postal and returns address will appear here before payment is enabled.','Postal and returns address: '+escapeHTML(env.SELLER_POSTAL_ADDRESS)+'. Seller is an individual and is not VAT registered.').replaceAll('The full seller postal address and operational order retention schedule will be added before paid checkout opens.','Seller postal address: '+escapeHTML(env.SELLER_POSTAL_ADDRESS)+'. Order records are retained for the applicable statutory accounting and consumer claim periods.');if(ready(env))html=html.replace(/<div class="notice"><strong>Checkout is being connected<\/strong>[\s\S]*?<\/div>/,'');data=new TextEncoder().encode(html);}return new Response(req.method==='HEAD'?null:data,{status:FILES[key]?200:404,headers:{...HEADERS,'Permissions-Policy':key==='/flash/index.html'?HEADERS['Permissions-Policy'].replace('serial=()','serial=(self)'):HEADERS['Permissions-Policy'],...((key==='/demo/index.html'||key==='/agents/demo/index.html')?{'Content-Security-Policy':HEADERS['Content-Security-Policy'].replace("frame-ancestors 'none'","frame-ancestors 'self'").replace("style-src 'self'","style-src 'self' 'unsafe-inline'").replace("script-src 'self' https://www.googletagmanager.com","script-src 'self' 'wasm-unsafe-eval'").replace(/connect-src [^;]+;/,"connect-src 'self';").replace(/img-src [^;]+;/,"img-src 'self' data:;").replace("form-action 'self' https://checkout.stripe.com","form-action 'none'")}:{}),'Content-Type':f.type,'Cache-Control':f.type.startsWith('text/html')||key.endsWith('.json')?'no-cache':'public, max-age=3600'}});}catch(error){return error instanceof RequestError?json({error:error.message},error.status):json({error:'Temporarily unavailable. Please try again later.'},503);}},async scheduled(_event,env,ctx){const work=retryCommerce(env);if(ctx?.waitUntil)ctx.waitUntil(work);return work;}};
