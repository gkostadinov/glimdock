#!/usr/bin/env python3
"""Capture the real Flipper GUI through USB RPC without changing its application.

Uses the stock/mntm-012 PB.Main GUI tags (20/21/22), not virtual display.
Requires pyserial; PNG output uses only the Python standard library.
"""
import argparse
import json
from pathlib import Path
import struct
import time
import zlib


def varint(value):
    result = bytearray()
    while value >= 128:
        result.append((value & 127) | 128)
        value >>= 7
    result.append(value)
    return bytes(result)


def request(command, tag):
    body = b'\x08' + varint(command) + varint((tag << 3) | 2) + b'\x00'
    return varint(len(body)) + body


def take_varint(data, position):
    value = 0
    for shift in range(0, 70, 7):
        if position >= len(data):
            raise ValueError('Truncated protobuf varint')
        byte = data[position]
        position += 1
        value |= (byte & 127) << shift
        if not byte & 128:
            return value, position
    raise ValueError('Oversized protobuf varint')


def fields(data):
    result, position = {}, 0
    while position < len(data):
        key, position = take_varint(data, position)
        number, wire = key >> 3, key & 7
        if not number:
            raise ValueError('Invalid protobuf field')
        if wire == 0:
            value, position = take_varint(data, position)
        elif wire in (1, 2, 5):
            if wire == 2:
                size, position = take_varint(data, position)
            else:
                size = 8 if wire == 1 else 4
            if size > len(data) - position:
                raise ValueError('Truncated protobuf field')
            value = data[position:position + size]
            position += size
        else:
            raise ValueError('Unsupported protobuf wire type')
        result[number] = value
    return result


def read_message(port, deadline):
    prefix = bytearray()
    while time.monotonic() < deadline:
        byte = port.read(1)
        if not byte:
            continue
        prefix.extend(byte)
        if len(prefix) > 5:
            raise RuntimeError('Invalid RPC message prefix; USB is not in RPC mode')
        if not byte[0] & 128:
            size, _ = take_varint(prefix, 0)
            if not 0 < size <= 65536:
                raise RuntimeError('RPC response exceeds capture limit')
            body = bytearray()
            while len(body) < size and time.monotonic() < deadline:
                body.extend(port.read(size - len(body)))
            if len(body) != size:
                raise TimeoutError('Timed out reading RPC response')
            return fields(body)
    raise TimeoutError('Timed out waiting for Flipper screen')


def png(frame):
    if len(frame) != 1024:
        raise ValueError('Expected the Flipper 128x64 framebuffer')
    raw = bytearray()
    for y in range(64):
        raw.append(0)  # PNG filter None
        for x in range(128):
            raw.append(0 if frame[(y // 8) * 128 + x] & (1 << (y % 8)) else 255)
    def chunk(kind, payload):
        return struct.pack('>I', len(payload)) + kind + payload + struct.pack('>I', zlib.crc32(kind + payload))
    return (b'\x89PNG\r\n\x1a\n' +
            chunk(b'IHDR', struct.pack('>IIBBBBB', 128, 64, 8, 0, 0, 0, 0)) +
            chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b''))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--timeout', type=float, default=6)
    args = parser.parse_args()
    import serial
    from serial.tools import list_ports
    if not any(p.device == args.port and p.vid == 0x0483 and p.pid == 0x5740 and
               str(p.serial_number or '').lower().startswith('flip_') for p in list_ports.comports()):
        raise RuntimeError('Selected port is not a verified USB Flipper')
    with serial.Serial(args.port, 115200, timeout=.1, write_timeout=2) as port:
        # Starting an RPC session does not launch/close apps or change BLE profiles.
        time.sleep(.3)
        port.reset_input_buffer()
        port.write(b'start_rpc_session\r')
        time.sleep(.4)
        port.reset_input_buffer()  # CLI echo only; no RPC commands have been sent.
        streaming = False
        try:
            port.write(request(1, 20))
            deadline = time.monotonic() + max(1, min(args.timeout, 30))
            while True:
                message = read_message(port, deadline)
                if message.get(1) == 1:
                    if message.get(2, 0):
                        raise RuntimeError('Screen streaming rejected: status ' + str(message[2]))
                    streaming = True
                if 22 in message:
                    screen = fields(message[22])
                    frame, orientation = screen.get(1, b''), screen.get(2, 0)
                    image = png(frame)
                    args.output.parent.mkdir(parents=True, exist_ok=True)
                    args.output.write_bytes(image)
                    print(json.dumps({'path': str(args.output.resolve()), 'width': 128,
                                      'height': 64, 'orientation': orientation,
                                      'framebuffer_bytes': len(frame)}), flush=True)
                    return
        finally:
            if streaming:
                port.write(request(2, 21))
                end = time.monotonic() + 2
                try:
                    while read_message(port, end).get(1) != 2:
                        pass
                except (TimeoutError, ValueError):
                    pass
            port.write(request(3, 19))  # StopSession releases only this USB RPC session.
            port.flush()
            time.sleep(.2)


if __name__ == '__main__':
    main()
