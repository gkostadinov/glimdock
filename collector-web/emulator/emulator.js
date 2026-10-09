import createFirmware from './firmware.js';

// Browser transport only. Every pixel and touch target is drawn by firmware C++.
export async function mountFirmwareEmulator({canvas, requestSnapshot, requestConfig, updateConfig, onSelectNode, onStatus, endpoint = `${location.origin}/api/v1/snapshot`, setupPaired = false}) {
  if (!(canvas instanceof HTMLCanvasElement)) throw new TypeError('A canvas is required for the firmware emulator.');
  if (typeof requestSnapshot !== 'function') throw new TypeError('A collector snapshot callback is required.');
  const firmware = await createFirmware({locateFile: name => new URL(name, import.meta.url).href});
  const width = firmware._glimdock_width(), height = firmware._glimdock_height();
  canvas.width = width; canvas.height = height;
  canvas.style.touchAction = 'none'; canvas.style.imageRendering = 'pixelated';
  canvas.setAttribute('role', 'img');
  canvas.setAttribute('aria-label', 'Interactive Glimdock firmware display. Touch or click the display to navigate.');
  const context = canvas.getContext('2d', {alpha: false});
  if (!context) throw new Error('Canvas rendering is unavailable.');
  const frame = context.createImageData(width, height);
  const events = new AbortController();
  const started = performance.now();
  let selected = '', generation = 0, disposed = false, pending = false, requestAgain = false, loaded = false, deferredSelection = '';
  let requestTimer, animation, previousFrame = -1, pressed = false;
  const callString = (name, ...args) => firmware.ccall(name, 'number', args.map(v => typeof v === 'string' ? 'string' : 'number'), args);
  const emit = (state, message) => onStatus?.({state, message, selectedNode: selected});
  const setConnection = ({endpoint: nextEndpoint = endpoint, setupPaired: paired = setupPaired} = {}) => {
    endpoint = nextEndpoint; setupPaired = paired;
    callString('glimdock_connection', endpoint, 1, setupPaired ? 1 : 0);
  };
  setConnection();

  async function responseValue(value, defaultStatus = 200) {
    if (value instanceof Response) return {status: value.status, body: await value.json()};
    if (value && typeof value === 'object' && 'body' in value && 'status' in value) return value;
    return {status: defaultStatus, body: value};
  }

  async function refresh() {
    if (disposed) return;
    if (pending) { requestAgain = true; return; }
    pending = true;
    const captured = generation, chosen = selected;
    try {
      const {status, body} = await responseValue(await requestSnapshot(chosen));
      if (disposed || captured !== generation) return;
      if (status !== 200) { const error = new Error(body?.error || `Collector returned HTTP ${status}.`); error.status = status; throw error; }
      if (chosen && body?.node?.id !== chosen) throw new Error('The collector returned a different node.');
      if (!callString('glimdock_snapshot', JSON.stringify(body))) throw new Error('Unsupported or oversized collector snapshot.');
      loaded = true;
      if (!selected && body?.node?.id) { selected = body.node.id; onSelectNode?.(selected); }
      if (deferredSelection) { const desired = deferredSelection; deferredSelection = ''; callString('glimdock_select', desired); }
      emit('connected', 'Live firmware connected to collector');
    } catch (error) {
      if (disposed || captured !== generation) return;
      if (error?.status === 404 && chosen) {
        selected = ''; generation++; onSelectNode?.(''); firmware._glimdock_default();
        requestAgain = true; emit('loading', 'Node removed; loading default'); return;
      }
      const message = error?.message || 'Collector unreachable.';
      callString('glimdock_error', message); emit('offline', message);
    } finally {
      pending = false;
      if (!disposed && requestAgain) { requestAgain = false; void refresh(); }
    }
  }

  async function configurationRequest(kind, payload) {
    try {
      if (typeof (kind === 'config' ? requestConfig : updateConfig) !== 'function') throw new Error('Pair a configuration token in the web interface to manage nodes.');
      const value = kind === 'config' ? await requestConfig() : await updateConfig(JSON.parse(payload));
      if (value == null) throw new Error('Pair a configuration token in the web interface to manage nodes.');
      const {body, status} = await responseValue(value, kind === 'config' ? 200 : 202);
      if (disposed) return;
      callString('glimdock_config', JSON.stringify(body), status);
      if (status === 202) {
        const nodes = body?.config?.nodes || body?.nodes;
        if (Array.isArray(nodes) && selected && !nodes.some(node => node.id === selected)) { selected = ''; generation++; onSelectNode?.(''); }
        requestAgain = true; void refresh();
      }
    } catch (error) {
      if (!disposed) callString('glimdock_config', JSON.stringify({error: error?.message || 'Collector settings request failed.'}), 503);
    }
  }
  firmware.onRequest = ({kind, payload}) => {
    if (disposed) return;
    if (kind === 'select') {
      selected = payload; generation++; onSelectNode?.(selected); emit('loading', 'Loading selected node');
      void refresh();
    } else if (kind === 'config' || kind === 'update') void configurationRequest(kind, payload);
  };

  const position = event => {
    const bounds = canvas.getBoundingClientRect();
    return {x: Math.floor((event.clientX - bounds.left) * width / bounds.width), y: Math.floor((event.clientY - bounds.top) * height / bounds.height)};
  };
  const input = (event, down) => {
    const {x, y} = position(event);
    firmware._glimdock_pointer(x, y, down ? 1 : 0);
    event.preventDefault();
  };
  canvas.addEventListener('pointerdown', event => { if (event.button !== 0 || pressed) return; pressed = true; canvas.setPointerCapture(event.pointerId); input(event, true); }, {signal: events.signal});
  canvas.addEventListener('pointermove', event => { if (pressed) input(event, true); }, {signal: events.signal});
  const release = event => { if (!pressed) return; pressed = false; input(event, false); };
  canvas.addEventListener('pointerup', release, {signal: events.signal});
  canvas.addEventListener('pointercancel', release, {signal: events.signal});
  canvas.addEventListener('lostpointercapture', release, {signal: events.signal});

  function draw() {
    if (disposed) return;
    firmware._glimdock_tick((Math.floor(performance.now() - started) + 1000) >>> 0);
    const revision = firmware._glimdock_frame();
    if (revision !== previousFrame) {
      const pixels = firmware.HEAPU16.subarray(firmware._glimdock_pixels() >>> 1, (firmware._glimdock_pixels() >>> 1) + width * height);
      for (let i = 0; i < pixels.length; i++) {
        const value = pixels[i], offset = i * 4;
        frame.data[offset] = Math.floor(((value >>> 11) & 31) * 255 / 31);
        frame.data[offset + 1] = Math.floor(((value >>> 5) & 63) * 255 / 63);
        frame.data[offset + 2] = Math.floor((value & 31) * 255 / 31);
        frame.data[offset + 3] = 255;
      }
      context.putImageData(frame, 0, 0); previousFrame = revision;
    }
    canvas.style.opacity = String(Math.min(1, firmware._glimdock_brightness() / 80));
    animation = requestAnimationFrame(draw);
  }
  draw(); emit('loading', 'Starting firmware'); void refresh();
  requestTimer = setInterval(() => { void refresh(); }, 3000);
  return {
    refresh,
    setConnection,
    selectNode(id) { if (disposed || typeof id !== 'string' || !/^[A-Za-z0-9:._-]{1,63}$/.test(id)) return false; if (!loaded) { deferredSelection = id; return true; } return Boolean(callString('glimdock_select', id)); },
    get selectedNode() { return selected; },
    get page() { return firmware._glimdock_page(); },
    captureLabels() { return JSON.parse(firmware.UTF8ToString(firmware._glimdock_labels())); },
    destroy() { disposed = true; generation++; clearInterval(requestTimer); cancelAnimationFrame(animation); events.abort(); firmware.onRequest = undefined; },
  };
}
