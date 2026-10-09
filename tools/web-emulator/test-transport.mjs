// Exercise browser transport races against the real WASM, with a canvas adapter.
import assert from 'node:assert/strict';
import {mountFirmwareEmulator} from '../../collector-web/emulator/emulator.js';

let clock=0,nextFrame=1;
const frames=new Map();
Object.defineProperty(globalThis,'performance',{value:{now:()=>clock},configurable:true});
globalThis.requestAnimationFrame=callback=>{const id=nextFrame++;frames.set(id,callback);return id;};
globalThis.cancelAnimationFrame=id=>frames.delete(id);
globalThis.HTMLCanvasElement=class {
  style={};painted=0;
  setAttribute(){}addEventListener(){}setPointerCapture(){}
  getBoundingClientRect(){return {left:0,top:0,width:320,height:240};}
  getContext(){return {createImageData:(width,height)=>({data:new Uint8ClampedArray(width*height*4)}),putImageData:()=>{this.painted++;}};}
};
const canvas=new HTMLCanvasElement();
const deferred=()=>{let resolve;const promise=new Promise(value=>{resolve=value;});return {promise,resolve};};
const first=deferred();let blocked=null,removed=false,calls=0;
const nodes=['a','b','c'].map(letter=>({id:`test:${letter}`,type:'server',platform:'linux',name:`Device ${letter.toUpperCase()}`,address:'127.0.0.1',status:'healthy',summary:{cpu_percent:0,memory_percent:0,sensors:0,status:'healthy',generated_at:Date.now()/1000,age_s:0,ttl_s:15}}));
const snapshot=id=>{const node=nodes.find(node=>node.id===id)||nodes[0];return {schema:1,sequence:++calls,generated_at:Date.now()/1000,node,nodes,host:{name:node.name,ip:'127.0.0.1',cpu_pct:0,mem_used_bytes:0,mem_total_bytes:1000}};};
const selections=[];
const emulator=await mountFirmwareEmulator({canvas,endpoint:'http://127.0.0.1:8765/api/v1/snapshot',requestSnapshot:id=>{
  if(!calls){calls++;return first.promise;}
  if(id==='test:b'&&removed){const error=new Error('Node removed');error.status=404;throw error;}
  if(id==='test:b'&&blocked)return blocked.promise;
  return snapshot(id);
},onSelectNode:id=>selections.push(id)});
const settle=async()=>{for(let i=0;i<30;i++)await Promise.resolve();clock+=1000;const firstFrame=frames.entries().next().value;if(firstFrame){frames.delete(firstFrame[0]);firstFrame[1]();}};
try{
  assert.equal(emulator.selectNode('test:b'),true,'selection can queue before first snapshot');
  first.resolve(snapshot('test:a'));await settle();
  assert.equal(emulator.selectedNode,'test:b');assert.equal(emulator.page,0);
  assert(emulator.captureLabels().some(label=>label.text==='Device B'),'queued selection opened its actual firmware overview');
  assert(canvas.painted>0,'RGB565 framebuffer was painted by the canvas adapter');
  removed=true;await emulator.refresh();await settle();
  assert(selections.includes(''),'deleted selected node clears caller selection');
  assert.equal(emulator.selectedNode,'test:a','404 falls back to the collector default');
  removed=false;blocked=deferred();emulator.selectNode('test:b');await settle();
  emulator.selectNode('test:c');await settle();
  blocked.resolve(snapshot('test:b'));blocked=null;await settle();
  assert.equal(emulator.selectedNode,'test:c','late response cannot replace a newer selection');
  assert(emulator.captureLabels().some(label=>label.text==='Device C'));
  emulator.destroy();assert.equal(frames.size,0,'destroy cancels rendering');
  assert.equal(emulator.selectNode('test:a'),false,'destroyed emulator cannot select nodes');
  console.log(JSON.stringify({passed:true,checks:['canvas frame upload','selection before initial load','deleted-node default fallback','stale response discarded','render cleanup']}));
}finally{emulator.destroy();}
