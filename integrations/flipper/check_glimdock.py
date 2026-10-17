#!/usr/bin/env python3
"""Observe paired status acknowledgments over a single verified Glimdock USB session."""
import argparse
import json
from pathlib import Path
import time

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', required=True)
    parser.add_argument('--mac', required=True)
    parser.add_argument('--duration', type=float, default=120)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    import serial
    from serial.tools import list_ports
    identity = args.mac.replace(':', '').upper()
    if not any(p.device == args.port and p.vid == 0x303a and
               str(p.serial_number or '').replace(':', '').upper() == identity
               for p in list_ports.comports()):
        raise RuntimeError('The selected Glimdock USB identity is not present')
    history, errors = [], []
    started = time.monotonic()
    last_request, last_report, last_key = 0.0, 0.0, None
    snapshots, boot_lines = 0, 0
    prefix = 'GLIMDOCK_FLIPPER_STATUS '
    with serial.Serial(args.port, 115200, timeout=.2, write_timeout=2) as port:
        while time.monotonic() - started < args.duration:
            now = time.monotonic()
            if now - last_request >= 2:
                port.write(b'GLIMDOCK_FLIPPER_STATUS\n')
                last_request = now
            raw = port.readline(2048).decode('utf-8', 'replace').strip()
            if prefix in raw:
                try:
                    status = json.loads(raw[raw.index(prefix) + len(prefix):])
                except json.JSONDecodeError:
                    continue
                # The firmware intentionally excludes PINs, credentials and snapshots.
                allowed = ('ready','enabled','connected','pairing','passkey_entry','peer','message',
                           'revision','uptime_ms','free_heap','min_free_heap','acked_frames','last_ack_ms',
                           'phase','security_start_rc','security_event_rc','security_flags','security_key_bytes')
                sample = {k: status[k] for k in allowed if k in status}
                sample['elapsed_s'] = round(now - started, 2)
                history.append(sample)
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.write_text(json.dumps({'samples': history, 'errors': errors,
                    'snapshot_receipts': snapshots, 'boot_lines': boot_lines}, indent=2) + '\n')
                key = tuple(sample.get(k) for k in ('enabled','connected','pairing','passkey_entry','message',
                                                   'phase','security_event_rc','security_flags'))
                if key != last_key or now - last_report >= 10:
                    print(json.dumps(sample), flush=True)
                    last_report, last_key = now, key
            elif raw.startswith('Snapshot received:'):
                snapshots += 1
            elif 'Glimdock boot:' in raw:
                boot_lines += 1
            elif any(s in raw.lower() for s in ('guru meditation','panic','brownout','assert failed','corrupt heap')):
                errors.append({'elapsed_s': round(now - started, 2), 'line': raw[:256]})
                print(json.dumps({'error': errors[-1]}), flush=True)
    print(json.dumps({'completed': True, 'samples': len(history), 'snapshot_receipts': snapshots,
                      'boot_lines': boot_lines, 'errors': len(errors)}), flush=True)

if __name__ == '__main__':
    main()
