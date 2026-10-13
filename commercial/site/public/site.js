const frame = document.querySelector('#demo-frame');
if (frame) {
  const image = document.querySelector('#screen-image');
  const reset = document.querySelector('#demo-reset');
  const status = document.querySelector('#demo-state');
  const badge = document.querySelector('#demo-badge');
  const caption = document.querySelector('#demo-caption');
  const instructions = document.querySelector('#demo-instructions');
  function ready() {
    image.hidden = true; frame.hidden = false; reset.disabled = false;
    badge.textContent = 'ACTUAL FIRMWARE';
    status.textContent = '4 example nodes · All readings simulated';
    caption.textContent = 'Production firmware in WebAssembly · 320 × 240 pixels';
    instructions.textContent = 'Tap a bento card to open a node. Drag to scroll a list. Use Nodes / Agents on the overview to switch workspaces. Tap the logo to return home. Use Pick system to choose a node or agent system, or Settings for display preferences.';
  }
  function unavailable() {
    image.hidden = false; frame.hidden = true; reset.disabled = true;
    badge.textContent = 'SCREEN PREVIEW';
    status.textContent = 'Interactive firmware unavailable';
    caption.textContent = 'Firmware screen preview · Synthetic example nodes';
    instructions.textContent = 'Try reloading the page to start the firmware demo.';
  }
  frame.addEventListener('load', () => {
    try { if (frame.contentDocument?.querySelector('#screen')?.dataset.ready === 'true') ready(); }
    catch { unavailable(); }
  });
  frame.addEventListener('error', unavailable);
  window.addEventListener('message', event => {
    if (event.source !== frame.contentWindow || event.origin !== location.origin) return;
    if (event.data?.type === 'glimdock-demo-ready') ready();
    if (event.data?.type === 'glimdock-demo-unavailable') unavailable();
  });
  reset.addEventListener('click', () => {
    reset.disabled = true; status.textContent = 'Resetting the example nodes…';
    frame.src = '/demo/index.html?reset=' + Date.now();
  });
}
