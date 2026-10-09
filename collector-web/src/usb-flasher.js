import {ESPLoader,Transport} from 'esptool-js';
import {md5} from '@noble/hashes/legacy.js';
import {bytesToHex} from '@noble/hashes/utils.js';
import {verifiedImages,programVerifiedImages,validateFlashManifest} from './flash-plan.js';

export async function flashGlimdock({manifest,onLog=()=>{},onStatus=()=>{},onProgress=()=>{}}){
  if(!window.isSecureContext||!navigator.serial)throw new Error('Open this page in Chrome or Edge using HTTPS or localhost.');
  validateFlashManifest(manifest,location.origin);
  // Keep the chooser in the user's click activation, before any download awaits.
  const port=await navigator.serial.requestPort({filters:[{usbVendorId:0x303a},{usbVendorId:0x10c4},{usbVendorId:0x1a86},{usbVendorId:0x0403}]});
  let transport;
  try{
    onStatus('Downloading and checking firmware…');
    const images=await verifiedImages(manifest,{origin:location.origin,fetchFile:async(url,expected)=>{const response=await fetch(url,{cache:'no-store',signal:AbortSignal.timeout(30000)});if(!response.ok)throw new Error('Firmware could not be downloaded.');const declared=Number(response.headers.get('Content-Length'));if(declared&&declared!==expected)throw new Error('Firmware download has an unexpected size.');return new Uint8Array(await response.arrayBuffer());},sha256:async data=>bytesToHex(new Uint8Array(await crypto.subtle.digest('SHA-256',data)))});
    onStatus('Connecting to the display bootloader…');
    transport=new Transport(port,false);
    const loader=new ESPLoader({transport,baudrate:115200,terminal:{clean:()=>{},writeLine:onLog,write:onLog}});
    await loader.detectChip('default_reset',3);
    await programVerifiedImages(loader,images,{onLog,onStatus,onProgress,md5:image=>bytesToHex(md5(image))});
  }finally{if(transport)await transport.disconnect().catch(()=>{});}
}
