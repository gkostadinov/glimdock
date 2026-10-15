export function mountInstaller({document,window,navigator,location,fetcher,flash,validateManifest}){
  const get=id=>document.getElementById(id);
  const button=get('install-firmware'),confirmed=get('board-confirmed'),status=get('install-status'),progress=get('install-progress'),log=get('install-log'),next=get('install-next');
  let manifest=null,busy=false;
  const supported=Boolean(window.isSecureContext&&navigator.serial&&typeof navigator.serial.requestPort==='function');
  const updateButton=()=>{button.disabled=!supported||!manifest||!confirmed.checked||busy;};
  const message=(text,state='ready')=>{status.textContent=text;status.dataset.state=state;};
  const protect=event=>{if(busy){event.preventDefault();event.returnValue='';}};
  window.addEventListener('beforeunload',protect);
  confirmed.addEventListener('change',updateButton);
  button.addEventListener('click',async()=>{
    if(button.disabled||busy||!confirmed.checked||!manifest)return;
    busy=true;confirmed.disabled=true;next.hidden=true;progress.hidden=false;progress.value=0;log.textContent='';updateButton();message('Choose your ESP32 USB port in the browser window.','busy');
    try{
      // The shared updater opens the chooser synchronously from this click.
      await flash({manifest,onStatus:text=>message(text,'busy'),onProgress:value=>{if(Number.isFinite(value))progress.value=Math.max(0,Math.min(100,value));},onLog:text=>{log.textContent=(log.textContent+String(text)+'\n').slice(-24000);}});
      progress.value=100;message('Firmware installed and verified. Your Glimdock is restarting.','success');next.hidden=false;
    }catch(error){
      progress.hidden=true;
      if(error?.name==='NotFoundError'||error?.name==='AbortError')message('No port selected. Connect your display and try again.');
      else if(error?.name==='NotAllowedError'||error?.name==='SecurityError')message('USB access was not allowed. Open this page directly in desktop Chrome or Edge and allow the selected port.','error');
      else message((error?.message||'Installation could not finish.')+' Close serial monitors, check the USB data cable and try again.','error');
    }finally{busy=false;confirmed.disabled=false;updateButton();}
  });
  updateButton();
  if(!supported)message(window.isSecureContext?'Open this page in desktop Chrome or Edge to install over USB. Safari, Firefox and embedded browsers may not support Web Serial.':'USB installation requires HTTPS or localhost. Open https://glimdock.com/flash/ in desktop Chrome or Edge.','unsupported');
  const ready=(async()=>{
    try{
      const response=await fetcher('/firmware/manifest.json',{cache:'no-store',signal:AbortSignal.timeout(15000)});
      if(!response.ok)throw Error('Firmware information is temporarily unavailable.');
      const candidate=await response.json();validateManifest(candidate,location.origin);
      const source=new URL(candidate.source_relink);
      if(source.origin!=='https://github.com'||!/^\/gkostadinov\/glimdock\/releases\/download\/v\d+\.\d+\.\d+\/source-relink\.tar\.gz$/.test(source.pathname)||source.search||source.hash)throw Error('Firmware source information could not be verified.');
      manifest=candidate;get('install-release').textContent=candidate.release;get('install-version').textContent=candidate.version;get('install-source').href=source.href;
      if(supported)message('Ready. Confirm your V1 board, then choose Flash firmware.');updateButton();
    }catch{manifest=null;updateButton();if(supported)message('Firmware information could not be verified. Reload this page to try again.','error');}
  })();
  return {ready};
}
