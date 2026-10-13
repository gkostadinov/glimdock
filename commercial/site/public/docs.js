const menu=document.querySelector('.menu-toggle');
const navigation=document.querySelector('#main-nav');
if(menu&&navigation){
  menu.addEventListener('click',()=>{
    const open=menu.getAttribute('aria-expanded')!=='true';
    menu.setAttribute('aria-expanded',String(open));navigation.classList.toggle('open',open);
  });
  document.addEventListener('keydown',event=>{
    if(event.key==='Escape'&&menu.getAttribute('aria-expanded')==='true'){
      menu.setAttribute('aria-expanded','false');navigation.classList.remove('open');menu.focus();
    }
  });
}
for(const preview of document.querySelectorAll('.agent-demo-frame')){
  window.addEventListener('message',event=>{
    if(event.source!==preview.contentWindow||event.origin!==location.origin||event.data?.type!=='glimdock-agent-demo-size')return;
    const height=event.data.height;
    if(!Number.isInteger(height)||height<180||height>720)return;
    if(preview.style.height!==height+'px')preview.style.height=height+'px';
  });
}
for(const block of document.querySelectorAll('[data-prompt-src]')){
  const button=document.querySelector('[data-copy="'+block.id+'"]');
  fetch(block.dataset.promptSrc,{credentials:'omit'}).then(response=>{
    if(!response.ok)throw new Error('Prompt unavailable');return response.text();
  }).then(text=>{block.textContent=text;if(button)button.disabled=false;}).catch(()=>{
    block.textContent='The inline prompt could not load. Use the Download text link.';
  });
}
for(const button of document.querySelectorAll('[data-copy]')){
  button.addEventListener('click',async()=>{
    const block=document.getElementById(button.dataset.copy);
    const status=button.parentElement.querySelector('.copy-status');
    try{
      await navigator.clipboard.writeText(block.textContent);
      status.textContent='Copied. Replace the bracketed placeholders.';
    }catch{
      const selection=window.getSelection(),range=document.createRange();
      range.selectNodeContents(block);selection.removeAllRanges();selection.addRange(range);
      status.textContent='Text selected. Copy it, or use Download text.';
    }
  });
}
