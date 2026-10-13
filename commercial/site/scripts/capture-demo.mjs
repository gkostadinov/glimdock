// Capture the public WASM renderer with the website's synthetic transport.
import {writeFileSync,mkdirSync} from 'node:fs';
import path from 'node:path';
import createFirmware from '../public/demo/emulator/firmware.js';
import {createDemoTransport} from '../public/demo/data.js';
const assets=path.resolve('public/demo/emulator');
const firmware=await createFirmware({locateFile:name=>path.join(assets,name)});
const snapshot=createDemoTransport()('');
if(!firmware.ccall('glimdock_snapshot','number',['string'],[JSON.stringify(snapshot)]))throw Error('Production parser rejected example data.');
firmware._glimdock_tick(1600);
const pixels=firmware.HEAPU16.subarray(firmware._glimdock_pixels()>>>1,(firmware._glimdock_pixels()>>>1)+320*240),rgb=new Uint8Array(320*240*3);
for(let i=0;i<pixels.length;i++){const value=pixels[i];rgb[i*3]=Math.floor(((value>>>11)&31)*255/31);rgb[i*3+1]=Math.floor(((value>>>5)&63)*255/63);rgb[i*3+2]=Math.floor((value&31)*255/31);}
const output=path.resolve('../../output/website-central-qa');mkdirSync(output,{recursive:true});
writeFileSync(path.join(output,'all-nodes.ppm'),Buffer.concat([Buffer.from('P6\n320 240\n255\n'),rgb]));
writeFileSync(path.join(output,'snapshot.json'),JSON.stringify(snapshot,null,2)+'\n');
writeFileSync(path.join(output,'labels.json'),firmware.UTF8ToString(firmware._glimdock_labels())+'\n');
console.log('Rendered website demo: 320 × 240 production firmware pixels, four synthetic nodes.');
