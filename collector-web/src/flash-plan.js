const NVS_START=0x9000,NVS_END=0xe000,FLASH_SIZE=0x1000000;
const EXPECTED=new Map([[0,0x8000],[0x8000,0x1000],[0xe000,0x2000],[0x10000,0x500000]]);
export function validateFlashManifest(manifest,origin){
  if(manifest?.schema!==1 || manifest.chip!=='ESP32-S3' || manifest.board!=='waveshare-v1' || manifest.rotation!==3 || manifest.flash_size!==FLASH_SIZE || !Array.isArray(manifest.parts) || manifest.parts.length!==4)throw new Error('This firmware bundle does not match the supported Glimdock hardware.');
  const seen=new Set();
  return manifest.parts.map(part=>{
    const address=part.offset,size=part.size;
    if(!Number.isSafeInteger(address)||!EXPECTED.has(address)||seen.has(address)||!Number.isSafeInteger(size)||size<1||size>EXPECTED.get(address))throw new Error('Firmware contains an invalid flash segment.');
    seen.add(address);
    const end=address+size,eraseStart=Math.floor(address/4096)*4096,eraseEnd=Math.ceil(end/4096)*4096;
    if(end>FLASH_SIZE || (eraseStart<NVS_END&&eraseEnd>NVS_START))throw new Error('Firmware would overwrite saved settings.');
    if(typeof part.sha256!=='string'||!/^[a-f0-9]{64}$/.test(part.sha256))throw new Error('Firmware segment has no valid verification hash.');
    const url=new URL(part.path,origin);
    if(url.origin!==origin||!url.pathname.startsWith('/firmware/')||!url.pathname.endsWith('.bin')||url.username||url.password||url.search||url.hash)throw new Error('Firmware files must come from this collector.');
    return {...part,url:url.href};
  }).sort((a,b)=>a.offset-b.offset);
}
export async function verifiedImages(manifest,{origin,fetchFile,sha256}){
  const parts=validateFlashManifest(manifest,origin);
  const images=[];
  for(const part of parts){
    const data=await fetchFile(part.url,part.size);
    if(!(data instanceof Uint8Array)||data.byteLength!==part.size)throw new Error('Firmware download size does not match its manifest.');
    if(await sha256(data)!==part.sha256)throw new Error('Firmware download failed SHA-256 verification.');
    images.push({address:part.offset,data});
  }
  return images;
}
export async function programVerifiedImages(loader,images,{onProgress=()=>{},onStatus=()=>{},onLog=()=>{},md5}){
  if(loader.chip?.CHIP_NAME!=='ESP32-S3'||loader.secureDownloadMode)throw new Error('The selected device is not a supported ESP32-S3.');
  const security=await loader.getSecurityInfo();
  if(security.parsedFlags?.SECURE_BOOT_EN||security.parsedFlags?.SECURE_DOWNLOAD_ENABLE||((security.flashCryptCnt||0).toString(2).replaceAll('0','').length%2)!==0)throw new Error('This device uses secure boot or encrypted flash; this firmware cannot be installed.');
  // Fixed ESP32-S3 GPIO7 power-latch sequence for the supported Waveshare V1.
  if(await loader.readReg(0x600080d8)&0x80 || await loader.readReg(0x60008094)&((1<<15)|(1<<11)|(1<<9)))throw new Error('The device security state prevents a normal firmware update.');
  await loader.writeReg(0x60004008,0x80);
  await loader.writeReg(0x60004090,0,0x4);
  await loader.writeReg(0x60004570,0x500,0xfff);
  await loader.writeReg(0x60009020,0x1200,0x7382);
  await loader.writeReg(0x60004024,0x80);
  await loader.writeReg(0x600084a0,0,0x80000);
  const latch=await loader.readReg(0x60004004),output=await loader.readReg(0x60004020),input=await loader.readReg(0x6000403c);
  if(!(latch&0x80)||!(output&0x80)||!(input&0x80))throw new Error('The display power latch could not be verified.');
  if(loader.chip.postConnect)await loader.chip.postConnect(loader);
  await loader.runStub();
  const size=await loader.detectFlashSize();
  if(!size || loader.flashSizeBytes(size)<FLASH_SIZE)throw new Error('This firmware requires a display with at least 16 MB of flash.');
  onLog('ESP32-S3 and power latch verified. Saved settings are outside all write ranges.');
  onStatus('Writing and verifying firmware…');
  const total=images.reduce((n,image)=>n+image.data.byteLength,0);
  await loader.writeFlash({fileArray:images,flashMode:'keep',flashFreq:'keep',flashSize:'keep',eraseAll:false,compress:true,calculateMD5Hash:md5,reportProgress:(index,written,size)=>{const completed=images.slice(0,index).reduce((n,image)=>n+image.data.byteLength,0);onProgress(Math.min(99,(completed+images[index].data.byteLength*(size?written/size:0))/total*100));}});
  onStatus('Verification complete. Restarting display…');
  await loader.after('hard_reset');
  onProgress(100);
}
