import {createPrivateChatDemo} from './emulator/private-demo.js';
import {mountFirmwareEmulator} from './emulator/emulator.js?v=20261013-agents';
import {createOperationsDemo} from './agent-data.js';
import {createDemoTransport} from './data.js';

const screen = document.querySelector('#screen');
const send = type => parent.postMessage({type}, location.origin);
try {
  await mountFirmwareEmulator({
    canvas:document.querySelector('canvas'), requestSnapshot:createDemoTransport(),requestPrivateChat:createPrivateChatDemo(), requestOperations:createOperationsDemo(),
    initialWorkspace:(location.pathname.startsWith('/agents/demo') || new URLSearchParams(location.search).get('workspace') === 'agents') ? 'agents' : 'nodes',
    endpoint:'https://demo.invalid/api/v1/snapshot', setupPaired:false,
    onStatus:({state}) => {
      if (state === 'connected' && screen.dataset.ready !== 'true') {
        screen.dataset.ready = 'true';
        requestAnimationFrame(() => send('glimdock-demo-ready'));
      }
    },
  });
} catch {
  screen.dataset.ready = 'false';
  send('glimdock-demo-unavailable');
}
