import test from'node:test';import assert from'node:assert/strict';import{ready,mailReady,validCart,sessionParams,verifiedSession,verifySignature}from'../worker/index.js';
const cart={quantity:2,country:'BG',terms:true,intent:'f4e0a806-04e2-478f-a10f-2b610a0f3e24'};
test('server rejects invalid cart, unsupported destination and absent terms',()=>{assert(validCart(cart));for(const q of [0,6,-1,1.5,'1'])assert(!validCart({...cart,quantity:q}));assert(!validCart({...cart,country:'US'}));assert(!validCart({...cart,terms:false}));assert(!validCart({...cart,intent:'evil'}));});
test('Stripe uses server price and destination-bound flat shipping',()=>{const p=sessionParams({...cart,unitPrice:1},'https://glimdock.com');assert.equal(p.get('line_items[0][price_data][unit_amount]'),'9900');assert.equal(p.get('line_items[0][quantity]'),'2');assert.equal(p.get('shipping_options[0][shipping_rate_data][fixed_amount][amount]'),'500');assert.equal(p.get('shipping_address_collection[allowed_countries][0]'),'BG');assert.equal(sessionParams({...cart,country:'DE'},'https://glimdock.com').get('shipping_options[0][shipping_rate_data][fixed_amount][amount]'),'1500');assert(p.get('success_url').startsWith('https://glimdock.com/'));});
test('payment gate stays closed until every actual operational dependency is set',()=>{assert(!ready({}));const e={CHECKOUT_ENABLED:'true',STRIPE_SECRET_KEY:'sk_live_fake',STRIPE_WEBHOOK_SECRET:'whsec_fake',DB:{},RESEND_API_KEY:'re_fake',SELLER_POSTAL_ADDRESS:'test fixture address',SUPPORT_READY:'true',PRODUCTION_READY:'true',ORDER_CONFIRMATIONS_READY:'true',OPERATIONS_READY:'true',OPERATIONS_TOKEN:'fixture-operations-token-32-characters'};assert(ready(e));for(const k of Object.keys(e)){const c={...e};delete c[k];assert(!ready(c),k);}});
test('success query alone cannot establish recognized order or price',()=>{const s={mode:'payment',currency:'eur',metadata:{quantity:'2',country:'BG',product:'glimdock-desk-first-batch',terms_accepted:'true'},amount_total:20300};assert(verifiedSession(s));assert(!verifiedSession({...s,amount_total:200}));assert(!verifiedSession({...s,currency:'usd'}));assert(!verifiedSession({...s,metadata:{}}));});
test('webhook requires signed body and recent timestamp',async()=>{const body='{"type":"checkout.session.completed"}',t=Math.floor(Date.now()/1000),secret='whsec_test_fixture';const key=await crypto.subtle.importKey('raw',new TextEncoder().encode(secret),{name:'HMAC',hash:'SHA-256'},false,['sign']);const mac=[...new Uint8Array(await crypto.subtle.sign('HMAC',key,new TextEncoder().encode(t+'.'+body)))].map(n=>n.toString(16).padStart(2,'0')).join('');const h='t='+t+',v1='+mac;assert(await verifySignature(body,h,secret));assert(!await verifySignature(body+' ',h,secret));assert(!await verifySignature(body,h,secret,(t+301)*1000));assert(!await verifySignature(body,'',secret));});

test('Cloudflare mail selection requires its own complete credentials and never falls back to another provider',()=>{
 const cloudflare={MAIL_PROVIDER:'cloudflare',CLOUDFLARE_EMAIL_ACCOUNT_ID:'a'.repeat(32),CLOUDFLARE_EMAIL_API_TOKEN:'fixture-not-a-real-token'};
 assert(mailReady(cloudflare));assert(mailReady({...cloudflare,MAIL_FROM:'Support <support@glimdock.com>'}));assert(mailReady({...cloudflare,MAIL_FROM:'support@glimdock.com'}));
 for(const change of [{CLOUDFLARE_EMAIL_ACCOUNT_ID:''},{CLOUDFLARE_EMAIL_ACCOUNT_ID:'bad'},{CLOUDFLARE_EMAIL_API_TOKEN:''},{CLOUDFLARE_EMAIL_API_TOKEN:'   '},{MAIL_FROM:'invalid address'},{MAIL_FROM:'Support\n <support@glimdock.com>'}])assert(!mailReady({...cloudflare,RESEND_API_KEY:'present-but-not-selected',...change}));
 assert(!mailReady({MAIL_PROVIDER:'unsupported',RESEND_API_KEY:'present'}));assert(mailReady({RESEND_API_KEY:'fixture'}));assert(mailReady({MAIL_PROVIDER:'resend',RESEND_API_KEY:'fixture'}));
});

test('custom color requests accept bounded plain text and retain Purple/White historical defaults',()=>{
 assert(validCart({...cart,bodyColor:' Black ',standColor:'Warm white'}));
 for(const invalid of [null,1,'','   ','x'.repeat(65),'<red>','red\nblue','red\rblue','red\tblue','red\u0000blue','red\u007fblue']){
  assert(!validCart({...cart,bodyColor:invalid}),String(invalid));assert(!validCart({...cart,standColor:invalid}),String(invalid));
 }
 assert(validCart({...cart,bodyColor:'🟣'.repeat(64)}));assert(!validCart({...cart,bodyColor:'🟣'.repeat(65)}));
 const p=sessionParams({...cart,bodyColor:' Black ',standColor:'Warm white'},'https://glimdock.com');
 assert.equal(p.get('metadata[body_color]'),'Black');assert.equal(p.get('metadata[stand_color]'),'Warm white');assert(p.get('line_items[0][price_data][product_data][description]').includes('Body: Black. Stand: Warm white.'));assert.equal(p.get('line_items[0][price_data][unit_amount]'),'9900');
 const old=sessionParams(cart,'https://glimdock.com');assert.equal(old.get('metadata[body_color]'),'Purple');assert.equal(old.get('metadata[stand_color]'),'White');
});

test('current Stripe Checkout uses the dynamic card filter rather than removed static payment methods',()=>{
 const params=sessionParams(cart,'https://glimdock.com');
 assert.equal(params.get('allowed_payment_method_types[0]'),'card');
 assert.equal([...params.keys()].some(key=>/^payment_method_types(?:\[|$)/.test(key)),false);
});
