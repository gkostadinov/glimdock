#!/usr/bin/env python3
"""Publish only the verified public firmware and the shared browser USB updater."""
import argparse
import hashlib
import json
from pathlib import Path
import re

SITE = Path(__file__).resolve().parents[1]
ROOT = SITE.parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--release', required=True, help='Existing immutable release, e.g. v0.5.6')
    args = parser.parse_args()
    if not re.fullmatch(r'v\d+\.\d+\.\d+', args.release):
        parser.error('Expected a published semantic version tag')
    canonical = ROOT / 'collector-web'
    manifest = json.loads((canonical / 'firmware/manifest.json').read_text())
    assert (manifest['schema'], manifest['chip'], manifest['board'], manifest['rotation'], manifest['flash_size']) == (1, 'ESP32-S3', 'waveshare-v1', 3, 16777216)
    assert [(p['offset'], p['path']) for p in manifest['parts']] == [(0, '/firmware/bootloader.bin'), (32768, '/firmware/partitions.bin'), (57344, '/firmware/boot_app0.bin'), (65536, '/firmware/firmware.bin')]
    files = {}
    for part in manifest['parts']:
        data = (canonical / part['path'].lstrip('/')).read_bytes()
        assert len(data) == part['size'] and hashlib.sha256(data).hexdigest() == part['sha256']
        start = part['offset'] & ~4095
        end = (part['offset'] + len(data) + 4095) & ~4095
        assert not (start < 0xe000 and end > 0x9000), 'Saved settings overlap an erase sector'
        files[part['path'].lstrip('/')] = data
    source = (canonical / manifest['source_relink'].lstrip('/')).read_bytes()
    assert len(source) == manifest['source_relink_size'] and hashlib.sha256(source).hexdigest() == manifest['source_relink_sha256']
    manifest['release'] = args.release
    manifest['source_relink'] = f'https://github.com/gkostadinov/glimdock/releases/download/{args.release}/source-relink.tar.gz'
    files['firmware/manifest.json'] = (json.dumps(manifest, indent=2) + '\n').encode()
    for name in ['usb-flasher.js', 'usb-flasher.js.LEGAL.txt']:
        files['assets/' + name] = (canonical / 'assets' / name).read_bytes()
    for path in (canonical / 'assets/licenses').glob('*.txt'):
        files['assets/licenses/' + path.name] = path.read_bytes()
    files['flash/flash-plan.js'] = (canonical / 'src/flash-plan.js').read_bytes()
    for name, data in files.items():
        destination = SITE / 'public' / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
    print(json.dumps({'firmware': manifest['version'], 'release': args.release, 'files': len(files), 'source_relink': 'Immutable GitHub release; source hash retained in manifest'}))


if __name__ == '__main__':
    main()
