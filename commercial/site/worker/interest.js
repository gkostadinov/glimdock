// Interest requests are unpaid and never create a payment session or an order.
export const INTEREST_CONSENT_VERSION='2026-10-17.1';
export function createInterestService({db,mailReady,sendMail,json,bodyJSON,origins,countries,color}){
 const pendingLifetime=7*86400,unsubscribeLifetime=373*86400,resendCooldown=3600;
 const genericAccepted={accepted:true,message:'If this address can be added, check your email to confirm your interest.'};
 const invalidLink={error:'This link is invalid or expired. Please submit the interest form again.'};
 const encoder=new TextEncoder();
 const tokenReady=env=>typeof env.WAITLIST_TOKEN_SECRET==='string'&&env.WAITLIST_TOKEN_SECRET.length>=32;
 const ready=env=>!!env.DB&&tokenReady(env)&&mailReady(env)&&env.INTEREST_ENABLED!=='false';
 const nowSeconds=()=>Math.floor(Date.now()/1000);
 const tokenInput=(row,purpose)=>'glimdock-interest-v1\n'+purpose+'\n'+row.id+'\n'+row.created_at+'\n'+(row.created_at+(purpose==='confirm'?pendingLifetime:unsubscribeLifetime));
 async function signingKey(env){return crypto.subtle.importKey('raw',encoder.encode(env.WAITLIST_TOKEN_SECRET),{name:'HMAC',hash:'SHA-256'},false,['sign','verify']);}
 async function sign(env,value){const bytes=await crypto.subtle.sign('HMAC',await signingKey(env),encoder.encode(value));return [...new Uint8Array(bytes)].map(x=>x.toString(16).padStart(2,'0')).join('');}
 async function token(env,row,purpose){return sign(env,tokenInput(row,purpose));}
 async function validToken(env,row,purpose,value,now){
  if(!/^[a-f0-9]{64}$/.test(value||'')||now>=row.created_at+(purpose==='confirm'?pendingLifetime:unsubscribeLifetime))return false;
  const bytes=Uint8Array.from(value.match(/../g),part=>parseInt(part,16));
  return crypto.subtle.verify('HMAC',await signingKey(env),bytes,encoder.encode(tokenInput(row,purpose)));
 }
 async function purge(env,now){
  if(!env.DB)return;
  await db(env).prepare('DELETE FROM interest_signups WHERE expires_at<=?').bind(now).run();
  await db(env).prepare('DELETE FROM interest_rate_limits WHERE expires_at<=?').bind(now).run();
 }
 async function allowRequest(req,env,now){
  // Cloudflare supplies this header at its edge. Never retain an IP or trust
  // X-Forwarded-For; keyed hourly hashes expire after at most 24 hours.
  const supplied=req.headers.get('CF-Connecting-IP')||'';
  const address=/^[a-f0-9:.]{3,45}$/i.test(supplied)?supplied.toLowerCase():'unavailable';
  const start=Math.floor(now/3600)*3600;
  const hash=await sign(env,'glimdock-interest-rate-v1\n'+start+'\n'+address);
  await db(env).prepare('INSERT INTO interest_rate_limits (ip_hash,request_count,expires_at) VALUES (?,0,?) ON CONFLICT(ip_hash) DO NOTHING').bind(hash,start+86400).run();
  const result=await db(env).prepare('UPDATE interest_rate_limits SET request_count=request_count+1 WHERE ip_hash=? AND request_count<10').bind(hash).run();
  return !!result.meta?.changes;
 }
 function normalize(x){
  if(!x||typeof x!=='object'||Array.isArray(x)||x.consent!==true||typeof x.email!=='string'||!Number.isInteger(x.quantity)||x.quantity<1||x.quantity>5||!countries.has(x.country))return null;
  const email=x.email.trim().toLowerCase();
  if(email.length>254||!/^[^\s<>@\u0000-\u001f\u007f]+@[^\s<>@\u0000-\u001f\u007f]+\.[^\s<>@\u0000-\u001f\u007f]+$/.test(email))return null;
  const optionalColor=(value,fallback)=>typeof value==='string'&&!value.trim()&&!/[<>\u0000-\u001f\u007f]/.test(value)?fallback:color(value,fallback);
  const bodyColor=optionalColor(x.bodyColor,'Purple'),standColor=optionalColor(x.standColor,'White');
  if(!bodyColor||!standColor)return null;
  return {email,quantity:x.quantity,country:x.country,bodyColor,standColor};
 }
 async function attemptConfirmation(env,row){
  const now=nowSeconds();
  if(!row||!ready(env)||row.confirmed_at||row.confirmation_sent_at||row.expires_at<=now)return false;
  const lease=await db(env).prepare('UPDATE interest_signups SET next_attempt_at=?,attempt_count=attempt_count+1 WHERE id=? AND confirmed_at IS NULL AND confirmation_sent_at IS NULL AND expires_at>? AND next_attempt_at<=?').bind(now+300,row.id,now,now).run();
  if(!lease.meta?.changes)return false;
  try{
   // Opt-out can delete a queued request; do not resurrect it or send after a
   // deletion observed before dispatch. An already in-flight email cannot be recalled.
   const current=await db(env).prepare('SELECT * FROM interest_signups WHERE id=? AND confirmed_at IS NULL AND confirmation_sent_at IS NULL AND expires_at>?').bind(row.id,nowSeconds()).first();
   if(!current)return false;
   const confirm='https://glimdock.com/interest/confirm/#'+new URLSearchParams({id:row.id,token:await token(env,row,'confirm')});
   const unsubscribe='https://glimdock.com/interest/unsubscribe/#'+new URLSearchParams({id:row.id,token:await token(env,row,'unsubscribe')});
   await sendMail(env,'interest-'+row.id+'-'+row.last_requested_at,{to:[current.email],subject:'Confirm your interest in Glimdock',text:'You asked to hear about Glimdock product availability.\nThis is an unpaid expression of interest, not an order or a reservation. No payment is requested.\n\nYour preferences:\nQuantity: '+current.quantity+'\nCountry: '+current.country+'\nBody color: '+current.body_color+'\nStand color: '+current.stand_color+'\n\nConfirm before '+new Date((row.created_at+pendingLifetime)*1000).toISOString()+' by opening this link and choosing Confirm:\n'+confirm+'\n\nOnly requested Glimdock product availability updates will follow confirmation. You can unsubscribe at any time.\nIf you did not request this, ignore this email; the unconfirmed request is deleted seven days after the original signup. Remove the request now:\n'+unsubscribe+'\n\nGlimdock — support@glimdock.com\nConsent version: '+current.consent_version});
   await db(env).prepare('UPDATE interest_signups SET confirmation_sent_at=?,next_attempt_at=0 WHERE id=? AND confirmed_at IS NULL').bind(nowSeconds(),row.id).run();
   return true;
  }catch{
   const next=nowSeconds()+Math.min(7200,60*2**Math.min(row.attempt_count,7));
   await db(env).prepare('UPDATE interest_signups SET next_attempt_at=? WHERE id=? AND confirmed_at IS NULL AND confirmation_sent_at IS NULL').bind(next,row.id).run();
   return false;
  }
 }
 async function counts(env,now){
  if(!env.DB)return {pending:0,due:0};
  const row=await db(env).prepare('SELECT COUNT(*) AS pending,COALESCE(SUM(CASE WHEN next_attempt_at<=? THEN 1 ELSE 0 END),0) AS due FROM interest_signups WHERE confirmed_at IS NULL AND confirmation_sent_at IS NULL AND expires_at>?').bind(now,now).first();
  return {pending:Number(row?.pending||0),due:Number(row?.due||0)};
 }
 async function retry(env){
  const now=nowSeconds();let sent=0;await purge(env,now);
  if(ready(env)){
   const result=await db(env).prepare('SELECT * FROM interest_signups WHERE confirmed_at IS NULL AND confirmation_sent_at IS NULL AND expires_at>? AND next_attempt_at<=? ORDER BY next_attempt_at,created_at LIMIT 5').bind(now,now).all();
   const results=await Promise.all((result.results||[]).map(row=>attemptConfirmation(env,row)));
   sent=results.filter(Boolean).length;
  }
  return {sent,...await counts(env,nowSeconds())};
 }
 async function summary(env){
  const now=nowSeconds();await purge(env,now);
  const row=await db(env).prepare('SELECT COUNT(CASE WHEN confirmed_at IS NOT NULL THEN 1 END) AS confirmed,COUNT(CASE WHEN confirmed_at IS NULL THEN 1 END) AS pending,COALESCE(SUM(CASE WHEN confirmed_at IS NOT NULL THEN quantity ELSE 0 END),0) AS requestedUnits FROM interest_signups').first();
  const grouped=await db(env).prepare('SELECT country,COUNT(CASE WHEN confirmed_at IS NOT NULL THEN 1 END) AS confirmed,COUNT(CASE WHEN confirmed_at IS NULL THEN 1 END) AS pending,COALESCE(SUM(CASE WHEN confirmed_at IS NOT NULL THEN quantity ELSE 0 END),0) AS requestedUnits FROM interest_signups GROUP BY country ORDER BY country').all();
  return {confirmed:Number(row?.confirmed||0),pending:Number(row?.pending||0),requestedUnits:Number(row?.requestedUnits||0),countries:(grouped.results||[]).map(x=>({country:x.country,confirmed:Number(x.confirmed),pending:Number(x.pending),requestedUnits:Number(x.requestedUnits)}))};
 }
 async function api(req,env,url,ctx){
  const path=url.pathname;
  if(path==='/api/interest-config'&&req.method==='GET')return json({enabled:ready(env),requiresEmailConfirmation:true});
  if(path==='/api/admin/interest-summary'&&req.method==='GET'){
   if(typeof env.OPERATIONS_TOKEN!=='string'||env.OPERATIONS_TOKEN.length<32||req.headers.get('Authorization')!=='Bearer '+env.OPERATIONS_TOKEN)return json({error:'Unauthorized.'},401);
   if(!env.DB)return json({error:'Interest storage unavailable.'},503);
   return json(await summary(env));
  }
  if(!['/api/interest','/api/interest/confirm','/api/interest/unsubscribe'].includes(path)||req.method!=='POST')return;
  if(!origins.has(url.origin)||req.headers.get('Origin')!==url.origin)return json({error:'Please use the Glimdock interest form.'},403);
  if(!/^application\/json(?:\s*;|$)/i.test(req.headers.get('Content-Type')||''))return json({error:'Invalid request.'},415);
  const x=await bodyJSON(req);
  if(path==='/api/interest'){
   if(!ready(env))return json({error:'The interest form is temporarily unavailable. Please try again later.'},503);
   // A filled honeypot receives the same response and causes no storage or mail.
   if(x&&typeof x==='object'&&x.website!==undefined&&x.website!=='')return json(genericAccepted,202);
   const input=normalize(x);if(!input)return json({error:'Enter a valid email, 1–5 monitors, an EU country and valid color preferences, then agree to product availability emails.'},400);
   const now=nowSeconds();await purge(env,now);
   if(!await allowRequest(req,env,now))return json(genericAccepted,202);
   await db(env).prepare('INSERT INTO interest_signups (id,email,quantity,country,body_color,stand_color,consent_version,consented_at,created_at,last_requested_at,expires_at) VALUES (?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(email) DO NOTHING').bind(crypto.randomUUID(),input.email,input.quantity,input.country,input.bodyColor,input.standColor,INTEREST_CONSENT_VERSION,now,now,now,now+pendingLifetime).run();
   // Public re-submission never changes preferences or extends retention. A
   // confirmed address receives no mail; pending addresses can request a resend.
   await db(env).prepare('UPDATE interest_signups SET confirmation_sent_at=NULL,attempt_count=0,next_attempt_at=0,last_requested_at=? WHERE email=? AND confirmed_at IS NULL AND confirmation_sent_at IS NOT NULL AND last_requested_at<=?').bind(now,input.email,now-resendCooldown).run();
   const row=await db(env).prepare('SELECT * FROM interest_signups WHERE email=?').bind(input.email).first();
   // Deferring provider work makes the public response independent of membership
   // and provider latency; the persisted queue also survives missing waitUntil.
   if(ctx?.waitUntil&&row&&!row.confirmed_at)ctx.waitUntil(attemptConfirmation(env,row).catch(()=>{}));
   return json(genericAccepted,202);
  }
  if(!env.DB||!tokenReady(env))return json({error:'This action is temporarily unavailable. Please try again later.'},503);
  if(!x||typeof x!=='object'||Array.isArray(x)||typeof x.id!=='string'||typeof x.token!=='string'||!/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(x.id||'')||!/^[a-f0-9]{64}$/.test(x.token||''))return json(invalidLink,410);
  const now=nowSeconds();await purge(env,now);
  const row=await db(env).prepare('SELECT * FROM interest_signups WHERE id=?').bind(x.id).first();
  const purpose=path.endsWith('/unsubscribe')?'unsubscribe':'confirm';
  if(!row)return purpose==='unsubscribe'?json({unsubscribed:true}):json(invalidLink,410);
  if(!await validToken(env,row,purpose,x.token,now))return json(invalidLink,410);
  if(purpose==='unsubscribe'){
   await db(env).prepare('DELETE FROM interest_signups WHERE id=?').bind(row.id).run();
   return json({unsubscribed:true});
  }
  if(!row.confirmed_at){
   const expiry=new Date(now*1000);expiry.setUTCMonth(expiry.getUTCMonth()+12);
   const result=await db(env).prepare('UPDATE interest_signups SET confirmed_at=?,expires_at=?,next_attempt_at=0 WHERE id=? AND confirmed_at IS NULL AND expires_at>?').bind(now,Math.floor(expiry.getTime()/1000),row.id,now).run();
   if(!result.meta?.changes){const current=await db(env).prepare('SELECT confirmed_at FROM interest_signups WHERE id=?').bind(row.id).first();if(!current?.confirmed_at)return json(invalidLink,410);}
  }
  return json({confirmed:true});
 }
 return {ready,api,retry};
}
