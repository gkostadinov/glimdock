// Exercise the production LVGL settings and shutdown state machine in WASM.
import assert from 'node:assert/strict';
import {readFile,writeFile,mkdtemp,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const assets=process.env.GLIMDOCK_EMULATOR_ASSETS||path.join(root,'collector-web/emulator');
const temporary=await mkdtemp(path.join(tmpdir(),'glimdock-power-ui-'));
try{
 await writeFile(path.join(temporary,'firmware.mjs'),await readFile(path.join(assets,'firmware.js')));
 const {default:createFirmware}=await import(pathToFileURL(path.join(temporary,'firmware.mjs')));
 const f=await createFirmware({locateFile:name=>path.join(assets,name)});
 const labels=()=>JSON.parse(f.UTF8ToString(f._glimdock_labels()));
 const texts=()=>labels().map(l=>l.text);
 let clock=1000;
 const tick=(delta=40)=>{clock+=delta;f._glimdock_tick(clock);};
 const pointer=(x,y,down)=>{f._glimdock_pointer(x,y,down);tick();};
 const tap=l=>{assert(l,'control is rendered');const x=l.x+Math.min(12,l.width/2),y=l.y+l.height/2;pointer(x,y,1);pointer(x,y,0);};
 const visible=text=>{for(let scroll=0;scroll<1800;scroll+=40){f._glimdock_scroll(scroll);const l=labels().find(l=>l.text===text&&l.y>=34&&l.y+l.height<200);if(l)return l;}assert.fail('Control is reachable: '+text);};
 // The startup press may last several seconds while the device boots.
 f._glimdock_power_button(1);tick();tick(5000);
 assert.equal(f._glimdock_powered_off(),0);assert(!texts().includes('Turning off'));
 f._glimdock_power_button(0);tick();tick(40);
 f._glimdock_show(7);tick();tap(visible('Turn off Glimdock'));
 assert.equal(f._glimdock_page(),35);assert(texts().includes('Turn off Glimdock?'));
 tap(visible('Keep on'));assert.equal(f._glimdock_page(),7);assert.equal(f._glimdock_powered_off(),0);
 tap(visible('Turn off Glimdock'));
 const off=visible('Turn off'),x=off.x+12,y=off.y+off.height/2;
 pointer(x,y,1);pointer(x,y+20,1);pointer(x,y,1);pointer(x,y,0);tick(1000);
 assert.equal(f._glimdock_powered_off(),0,'returning swipe does not shut down');
 tap(visible('Turn off'));tick(500);
 assert.equal(f._glimdock_powered_off(),1,'confirmed Settings shutdown reaches board power control');
 assert(texts().includes('Device off'));tap(labels().find(l=>l.text==='Device off'));tick(80);
 assert.equal(f._glimdock_powered_off(),0,'browser demo has a reversible wake action');
 // A short physical press is harmless. A long one waits for release.
 f._glimdock_power_button(1);tick();tick(40);tick(800);
 f._glimdock_power_button(0);tick();tick(40);
 assert.equal(f._glimdock_powered_off(),0);
 f._glimdock_power_button(1);tick();tick(40);tick(2000);
 assert(texts().includes('Release BAT_PWR to finish'));
 tick(1500);assert.equal(f._glimdock_powered_off(),0,'held button cannot cause an immediate wake loop');
 f._glimdock_power_button(0);tick();assert.equal(f._glimdock_powered_off(),0,'release is debounced');
 tick(40);assert.equal(f._glimdock_powered_off(),1);
 console.log(JSON.stringify({passed:true,checks:['startup hold ignored','reachable Settings power control','cancel','swipe guard','confirmed power off','preview wake','short press ignored','two-second hold','release before shutdown']}));
}finally{await rm(temporary,{recursive:true,force:true});}
