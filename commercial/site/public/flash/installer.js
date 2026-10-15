import {flashGlimdock} from '/assets/usb-flasher.js';
import {validateFlashManifest} from './flash-plan.js';
import {mountInstaller} from './installer-controller.js';

mountInstaller({document,window,navigator,location,fetcher:fetch,flash:flashGlimdock,validateManifest:validateFlashManifest});
