import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import vm from 'node:vm';
const script=readFileSync(process.env.SITE_SCRIPT||path.resolve('public/site.js'),'utf8');
function element(properties={}){const callbacks=new Map();return Object.assign({callbacks,addEventListener(type,callback){callbacks.set(type,callback);}},properties);}
function homepage(ready=false){
  const frame=element({hidden:true,contentWindow:{},contentDocument:{querySelector(){return {dataset:{ready:String(ready)}};}}});
  const image=element({hidden:false}),reset=element({disabled:true}),status=element({textContent:''});
  const badge=element(),caption=element(),instructions=element();
  const elements={'#demo-frame':frame,'#screen-image':image,'#demo-reset':reset,'#demo-state':status,'#demo-badge':badge,'#demo-caption':caption,'#demo-instructions':instructions};
  const window=element();
  vm.runInNewContext(script,{document:{querySelector(selector){return elements[selector]||null;}},window,location:{origin:'https://glimdock.com'}});
  return {frame,image,reset,status,badge,caption,instructions,window};
}
test('shared script loads safely on a page without the homepage demo',()=>{
  const window=element();assert.doesNotThrow(()=>vm.runInNewContext(script,{document:{querySelector(){return null;}},window,location:{origin:'https://glimdock.com'}}));assert.equal(window.callbacks.size,0);
});
test('iframe load alone cannot claim the firmware started',()=>{
  const h=homepage();h.frame.callbacks.get('load')();assert.equal(h.image.hidden,false);assert.equal(h.frame.hidden,true);assert.equal(h.reset.disabled,true);
});
test('trusted ready message reveals the actual firmware and Reset waits for startup',()=>{
  const h=homepage(),message=h.window.callbacks.get('message');
  message({source:{},origin:'https://glimdock.com',data:{type:'glimdock-demo-ready'}});
  message({source:h.frame.contentWindow,origin:'https://example.com',data:{type:'glimdock-demo-ready'}});assert.equal(h.frame.hidden,true);
  message({source:h.frame.contentWindow,origin:'https://glimdock.com',data:{type:'glimdock-demo-ready'}});
  assert.equal(h.image.hidden,true);assert.equal(h.frame.hidden,false);assert.equal(h.reset.disabled,false);assert.equal(h.badge.textContent,'ACTUAL FIRMWARE');assert.equal(h.status.textContent,'4 example nodes · All readings simulated');
  h.reset.callbacks.get('click')();assert.equal(h.reset.disabled,true);assert.match(h.frame.src,/^\/demo\/index\.html\?reset=\d+$/);
  message({source:h.frame.contentWindow,origin:'https://glimdock.com',data:{type:'glimdock-demo-ready'}});assert.equal(h.reset.disabled,false);
});
test('an unavailable WASM renderer returns to the labelled static firmware capture',()=>{
  const h=homepage(true);h.frame.callbacks.get('load')();assert.equal(h.frame.hidden,false);
  h.window.callbacks.get('message')({source:h.frame.contentWindow,origin:'https://glimdock.com',data:{type:'glimdock-demo-unavailable'}});
  assert.equal(h.frame.hidden,true);assert.equal(h.image.hidden,false);assert.equal(h.reset.disabled,true);assert.equal(h.status.textContent,'Interactive firmware unavailable');
});
