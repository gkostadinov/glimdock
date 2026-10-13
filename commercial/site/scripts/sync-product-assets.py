#!/usr/bin/env python3
"""Refresh reviewed CAD images and public firmware renderer before a site release.

Optional asset tooling only: Pillow is needed to encode WebP. Site builds and
runtime use Node.js and a Worker, with no Python dependency.
"""
from pathlib import Path
import hashlib, json, shutil, subprocess
from PIL import Image

root = Path(__file__).resolve().parents[3]
public = root / 'commercial/site/public'
case = root / 'enclosure/public/pebble-r26-prototype-v2'
renders = case / 'assets'
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
case_manifest=json.loads((case/'manifest.json').read_text())
provenance = {'schema':2, 'case_revision':'r25 body and cradle / r24 internals / r26 print trial', 'design_sha256':sha(case/'manifest.json'), 'geometry':case_manifest['geometry_status'], 'screen_artwork':'Synthetic illustration on nominal CAD, not current installed firmware or a physical product photo', 'renders':{}}
for name, source in {'hero':'hero.png','side':'side.png','usb-detail':'usb-detail.png','cradle-detail':'cradle-detail.png','exploded':'exploded.png'}.items():
    src, target = renders / source, public / f'assets/{name}.webp'
    with Image.open(src) as im:
        assert im.width>=1000 and im.height>=800, (src,im.size)
        width,height=im.size
        im.convert('RGB').save(target, 'WEBP', quality=86, method=6)
    provenance['renders'][name] = {'source':str(src.relative_to(root)), 'source_sha256':sha(src), 'public':str(target.relative_to(root)), 'sha256':sha(target), 'width':width,'height':height}
for obsolete in ['rear','ears','antennas','eyes']:
    (public/f'assets/{obsolete}.webp').unlink(missing_ok=True)
renderer = root/'collector-web/emulator'
shutil.copytree(renderer, public/'demo/emulator', dirs_exist_ok=True)
provenance['renderer'] = json.loads((renderer/'manifest.json').read_text())
subprocess.run(['node','scripts/capture-demo.mjs'],cwd=root/'commercial/site',check=True)
capture = root/'output/website-central-qa/all-nodes.ppm'
with Image.open(capture) as im:
    assert im.size == (320,240)
    im.save(public/'assets/screens/all-nodes.png')
provenance['fallback'] = {'renderer_sha256':provenance['renderer']['assets']['firmware.wasm']['sha256'], 'transport':'commercial/site/public/demo/data.js', 'transport_sha256':sha(public/'demo/data.js'), 'sha256':sha(public/'assets/screens/all-nodes.png'), 'data':'Four synthetic nodes; no live collector or LAN telemetry', 'width':320,'height':240}
(public/'assets/product-provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
print(json.dumps({'case_revision':provenance['case_revision'], 'renderer_sha256':provenance['renderer']['assets']['firmware.wasm']['sha256'], 'images':len(provenance['renders'])}))
