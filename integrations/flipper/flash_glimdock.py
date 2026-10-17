#!/usr/bin/env python3
"""Flash a verified Glimdock V1 while retaining and checking its NVS bytes."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', required=True)
    parser.add_argument('--mac', required=True, help='Expected USB and eFuse MAC, with colons')
    parser.add_argument('--images', type=Path, default=ROOT / 'output/flipper-pio/dist')
    args = parser.parse_args()
    from serial.tools import list_ports
    expected = args.mac.replace(':', '').upper()
    if len(expected) != 12 or any(c not in '0123456789ABCDEF' for c in expected):
        raise RuntimeError('Expected MAC must contain twelve hexadecimal digits')
    ports = [p for p in list_ports.comports() if p.device == args.port and p.vid == 0x303a
             and str(p.serial_number or '').replace(':', '').upper() == expected]
    if len(ports) != 1:
        raise RuntimeError('The explicitly selected ESP32 USB identity is not present')
    manifest = json.loads((args.images / 'build-manifest.json').read_text())
    if (manifest['board'], manifest['environment'], manifest['rotation']) != (
            'Waveshare ESP32-S3-Touch-LCD-2.8 V1', 'homelab_s3_web', 3):
        raise RuntimeError('Unexpected board or firmware environment')
    images = []
    if [i['offset'] for i in manifest['images']] != [0, 0x8000, 0xe000, 0x10000]:
        raise RuntimeError('Unexpected image layout')
    for image in manifest['images']:
        path = args.images / image['file']
        if path.parent != args.images or not path.is_file():
            raise RuntimeError('Invalid image path')
        data = path.read_bytes()
        if len(data) != image['bytes'] or hashlib.sha256(data).hexdigest() != image['sha256']:
            raise RuntimeError('Firmware image does not match build manifest')
        start = image['offset'] & ~0xfff
        end = (image['offset'] + len(data) + 0xfff) & ~0xfff
        if (start < 0xe000 and end > 0x9000) or end > 0x510000:
            raise RuntimeError('Image overlaps saved settings or exceeds application partition')
        images.append((image['offset'], path))
    sys.path.insert(0, str(Path.home() / '.platformio/packages/tool-esptoolpy'))
    import esptool
    from esptool.targets.esp32s3 import ESP32S3ROM
    evidence = ROOT / 'output/flipper-pio/hardware'
    evidence.mkdir(parents=True, exist_ok=True)
    esp = ESP32S3ROM(args.port, 115200)
    try:
        esp.connect(mode='default_reset', attempts=3)
        if esp.CHIP_NAME != 'ESP32-S3' or esp.secure_download_mode:
            raise RuntimeError('Unsupported chip or security mode')
        if bytes(esp.read_mac()).hex().upper() != expected:
            raise RuntimeError('eFuse identity differs from the selected USB device')
        if esp.read_reg(0x600080d8) & 0x80 or esp.read_reg(0x60008094) & ((1 << 15) | (1 << 11) | (1 << 9)):
            raise RuntimeError('Encrypted flash or secure boot requires a different update path')
        # Existing V1 power-latch sequence, also used by the public Web Serial updater.
        for address, value, mask in [(0x60004008,0x80,0xffffffff), (0x60004090,0,0x4),
                (0x60004570,0x500,0xfff), (0x60009020,0x1200,0x7382),
                (0x60004024,0x80,0xffffffff), (0x600084a0,0,0x80000)]:
            esp.write_reg(address, value, mask=mask)
        if any(not esp.read_reg(address) & 0x80 for address in (0x60004004,0x60004020,0x6000403c)):
            raise RuntimeError('V1 GPIO7 power latch did not verify high')
        esp = esp.run_stub()
        if (esp.flash_id() >> 16) & 0xff != 0x18:
            raise RuntimeError('Expected 16 MiB flash')
        existing_table = esp.read_flash(0x8000, 0x1000)
        new_table = (args.images / 'partitions.bin').read_bytes()
        if existing_table[:len(new_table)] != new_table:
            raise RuntimeError('Installed partition table differs; refusing an implicit migration')
        before = esp.read_flash(0x9000, 0x5000)
        stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
        backup = evidence / ('nvs-before-' + stamp + '.bin')
        with os.fdopen(os.open(backup, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'wb') as f:
            f.write(before)
        print('Verified ESP32-S3 V1 identity, 16 MiB flash, partition table and power latch.', flush=True)
        # The stub is already resident for backup/read-back. Do not load it twice.
        command = ['--chip','esp32s3','--port',args.port,'--baud','921600','--no-stub','--after','no_reset_stub',
                   'write_flash','-z','--flash_mode','keep','--flash_freq','keep','--flash_size','keep']
        for offset, path in images:
            command.extend([hex(offset), str(path)])
        esptool.main(command, esp=esp)
        after = esp.read_flash(0x9000, 0x5000)
        if before != after:
            raise RuntimeError('NVS differs after programming; device has not been restarted')
        receipt = {'flashed': True, 'nvs_unchanged_before_reboot': True,
                   'nvs_backup': str(backup.relative_to(ROOT)), 'nvs_backup_mode': '0600',
                   'images': manifest['images'], 'hardware': manifest['board'],
                   'flash_verified_by_esptool': True}
        (evidence / 'flash-result.json').write_text(json.dumps(receipt, indent=2) + '\n')
        esp.hard_reset()
        print('Firmware verified; NVS bytes unchanged. Glimdock restarted.', flush=True)
    finally:
        esp._port.close()

if __name__ == '__main__':
    main()
