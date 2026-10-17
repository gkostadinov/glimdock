#!/usr/bin/env python3
"""Measure real Up/Down navigation with one Flipper USB GUI stream.

Requires pyserial. Reuses the capture tool's bounded protobuf and stdlib PNG
helpers. Measures row-zero node pixels (y14..24), excluding footer feedback.
The Glimdock app must already be open and connected; no app/profile is launched.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import statistics
import time

_spec = importlib.util.spec_from_file_location(
    'flipper_capture', Path(__file__).with_name('flipper-capture-screen.py'))
capture = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(capture)

# Momentum mntm-012 assets/protobuf commit, verified against its pinned gui.proto:
# https://github.com/Next-Flip/flipperzero-protobuf/blob/ea4f185f5eaa265955c520eae2832887ee6aa5e4/gui.proto
# InputKey UP=0 DOWN=1 OK=4; InputType SHORT=2; nested fields key=1/type=2.
INPUT_SHORT = 2


class RpcStream:
    """Retain partial messages across measurement/settling deadlines."""
    def __init__(self, port):
        self.port, self.pending = port, bytearray()

    def __getattr__(self, name):
        return getattr(self.port, name)

    def read_message(self, deadline):
        while True:
            for index, byte in enumerate(self.pending[:5]):
                if not byte & 128:
                    size, prefix = capture.take_varint(self.pending, 0)
                    if not 0 < size <= 65536:
                        raise RuntimeError('Invalid RPC response size')
                    if len(self.pending) >= prefix + size:
                        body = bytes(self.pending[prefix:prefix + size])
                        del self.pending[:prefix + size]
                        return capture.fields(body)
                    break
            else:
                if len(self.pending) >= 5:
                    raise RuntimeError('Invalid RPC response prefix')
            if time.monotonic() >= deadline:
                raise TimeoutError('Timed out waiting for GUI stream')
            self.pending.extend(self.port.read(min(max(self.port.in_waiting, 1), 4096)))


def input_request(command, key, event_type=INPUT_SHORT):
    nested = b'\x08' + capture.varint(key) + b'\x10' + capture.varint(event_type)
    body = (b'\x08' + capture.varint(command) + capture.varint((23 << 3) | 2) +
            capture.varint(len(nested)) + nested)
    return capture.varint(len(body)) + body


def short_requests(command, key):
    # Pinned rpc_gui.c assigns a sequence on PRESS; gui.c rejects SHORT unless
    # that key remains pressed. RELEASE must follow SHORT, not precede it.
    return b''.join(input_request(command + offset, key, event_type)
                    for offset, event_type in enumerate((0, 2, 1)))


def node_pixels(frame):
    if len(frame) != 1024:
        raise ValueError('Expected the real 128x64 framebuffer')
    return bytes((frame[(y // 8) * 128 + x] >> (y % 8)) & 1
                 for y in range(14, 25) for x in range(128))


def fingerprint(frame):
    return hashlib.sha256(node_pixels(frame)).hexdigest()[:16]


def next_frame(port, deadline):
    while True:
        message = port.read_message(deadline)
        if message.get(2, 0):
            raise RuntimeError('RPC command rejected: id=%s status=%s' %
                               (message.get(1, 0), message[2]))
        if 22 in message:
            screen = capture.fields(message[22])
            frame, orientation = screen.get(1, b''), screen.get(2, 0)
            if len(frame) != 1024 or orientation != 0:
                raise RuntimeError('Expected an unrotated 128x64 Flipper screen')
            return time.perf_counter(), frame


def settle_frames(port, frame, duration):
    deadline = time.monotonic() + duration
    while time.monotonic() < deadline:
        try:
            _, frame = next_frame(port, deadline)
        except TimeoutError:
            break
    return frame


def measure(port, baseline, command, key, timeout, settle):
    baseline_pixels = node_pixels(baseline)
    written = time.perf_counter()
    port.write(short_requests(command, key))
    port.flush()
    deadline = time.monotonic() + timeout
    visual_ms = None
    changed = []
    first_change = None
    until = deadline
    while time.monotonic() < until:
        try:
            stamp, frame = next_frame(port, until)
        except TimeoutError:
            break
        latency = (stamp - written) * 1000
        if visual_ms is None and frame != baseline:
            visual_ms = latency
        if node_pixels(frame) != baseline_pixels:
            changed.append((latency, frame))
            if first_change is None:
                first_change = latency
                until = min(deadline, time.monotonic() + settle)
    if not changed:
        return {'ok': False, 'error': 'node-row-did-not-change',
                'first_visual_change_ms': visual_ms}, baseline, None
    final = changed[-1][1]
    final_pixels = node_pixels(final)
    # If a transient placeholder preceded the final observed name, report the
    # first matching final crop, while retaining the first-region-change time.
    confirmed_ms = next(ms for ms, frame in changed if node_pixels(frame) == final_pixels)
    return {'ok': True, 'node_name_change_ms': round(confirmed_ms, 2),
            'first_node_region_change_ms': round(first_change, 2),
            'first_visual_change_ms': None if visual_ms is None else round(visual_ms, 2),
            'before_node_fingerprint': fingerprint(baseline),
            'after_node_fingerprint': fingerprint(final),
            'intermediate_node_fingerprints': len({fingerprint(f) for _, f in changed})}, final, changed[0][1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--label', default='navigation')
    parser.add_argument('--samples', type=int, default=6)
    parser.add_argument('--timeout', type=float, default=8)
    parser.add_argument('--settle', type=float, default=.4)
    parser.add_argument('--skip-ok', action='store_true', help='Already on the System page')
    args = parser.parse_args()
    if not 1 <= args.samples <= 20 or not .1 <= args.settle <= 2 or not 1 <= args.timeout <= 30:
        parser.error('samples1..20, settle0.1..2s, timeout1..30s required')
    import serial
    from serial.tools import list_ports
    if not any(p.device == args.port and p.vid == 0x0483 and p.pid == 0x5740 and
               str(p.serial_number or '').lower().startswith('flip_') for p in list_ports.comports()):
        raise RuntimeError('Selected port is not a verified USB Flipper')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    frames = args.output.parent / (args.output.stem + '-frames')
    frames.mkdir(exist_ok=True)
    samples = []
    command = 1
    with serial.Serial(args.port, 115200, timeout=.05, write_timeout=2) as port:
        time.sleep(.3)
        port.reset_input_buffer()
        port = RpcStream(port)
        port.write(b'start_rpc_session\r')
        time.sleep(.4)
        port.reset_input_buffer()
        try:
            port.write(capture.request(command, 20))
            _, baseline = next_frame(port, time.monotonic() + args.timeout)
            if not args.skip_ok:
                command += 1
                port.write(short_requests(command, 4))
                command += 2
                _, baseline = next_frame(port, time.monotonic() + args.timeout)
            baseline = settle_frames(port, baseline, .5)
            (frames / 'baseline.png').write_bytes(capture.png(baseline))
            for index in range(args.samples):
                key = 1 if index % 2 == 0 else 0  # Down, Up, Down, Up ...
                command += 1
                result, after, first = measure(port, baseline, command, key,
                                                args.timeout, args.settle)
                command += 2
                result.update(index=index + 1, key='DOWN' if key else 'UP')
                (frames / ('%02d-before.png' % (index + 1))).write_bytes(capture.png(baseline))
                (frames / ('%02d-after.png' % (index + 1))).write_bytes(capture.png(after))
                if first is not None:
                    (frames / ('%02d-first-change.png' % (index + 1))).write_bytes(capture.png(first))
                samples.append(result)
                print(json.dumps(result), flush=True)
                baseline = after
                if not result['ok']:
                    break
        finally:
            command += 1
            port.write(capture.request(command, 21))
            end = time.monotonic() + 2
            try:
                while port.read_message(end).get(1) != command:
                    pass
            except (TimeoutError, ValueError):
                pass
            port.write(capture.request(command + 1, 19))
            port.flush()
            time.sleep(.2)
    values = [s['node_name_change_ms'] for s in samples if s['ok']]
    report = {'label': args.label, 'method': 'USB short-input write to first changed node-name crop',
              'node_region': {'x': [0, 127], 'y': [14, 24]},
              'schema_commit': 'ea4f185f5eaa265955c520eae2832887ee6aa5e4',
              'samples_requested': args.samples, 'samples': samples,
              'summary_ms': None if not values else {'min': min(values),
                  'median': round(statistics.median(values), 2), 'max': max(values)},
              'frames': str(frames.resolve()),
              'limitations': 'GUI stream/USB delivery adds measurement latency. Crops identify pixel changes, not OCR; review PNGs for placeholders.'}
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'report': str(args.output.resolve()), 'summary_ms': report['summary_ms']}), flush=True)


if __name__ == '__main__':
    main()
