#!/usr/bin/env python3
"""Provision the separate private reader over USB from a restricted local file.

The file is the only credential input. Device replies and exception details are
never displayed. This command does not reboot the device or change its public
monitoring settings. No credentials are bundled with this tool.
"""
import argparse
from dataclasses import dataclass
import ipaddress
import json
import os
from pathlib import Path
import re
import secrets
import ssl
import stat
import sys
import time
from urllib.parse import urlsplit

SCHEMA_VERSION = "private-chat-pairing/v1"
MAX_FILE_BYTES = 16 * 1024
MAX_CA_BYTES = 8192
MAX_URL_BYTES = 256
MAX_FRAME_BYTES = 12 * 1024
CHUNK_BYTES = 128
MAX_ACK_LINE_BYTES = 128
WRITE_TIMEOUT_S = 2.0
ACK_TIMEOUT_S = 8.0
READ_TIMEOUT_S = 0.1
FRAME_PREFIX = b"GLIMDOCK_PRIVATE_READER "
ACK_OK_PREFIX = b"GLIMDOCK_PRIVATE_READER_OK "
ACK_ERROR_PREFIX = b"GLIMDOCK_PRIVATE_READER_ERROR "
SUCCESS_MESSAGE = "Private reader provisioned."
FAILURE_MESSAGE = "Private reader provisioning failed."
NONCE_PATTERN = re.compile(r"[A-Za-z0-9_-]{16,48}\Z")
CERTIFICATE_PATTERN = re.compile(
    r"-----BEGIN CERTIFICATE-----\r?\n"
    r"[A-Za-z0-9+/=\r\n]+"
    r"-----END CERTIFICATE-----"
)
FIELDS = {"schema_version", "device_id", "relay_url", "device_token", "ca_pem"}


class ProvisioningError(Exception):
    """A deliberately detail-free failure safe to display."""


def fail():
    raise ProvisioningError(FAILURE_MESSAGE) from None


@dataclass(frozen=True, repr=False)
class Credentials:
    device_id: str
    relay_url: str
    device_token: str
    ca_pem: str


def printable_ascii(value, minimum, maximum):
    return (isinstance(value, str) and minimum <= len(value) <= maximum
            and all(33 <= ord(char) <= 126 for char in value))


def validate_origin(value):
    if not printable_ascii(value, 1, MAX_URL_BYTES) or "\\" in value:
        fail()
    try:
        parsed = urlsplit(value)
        if (parsed.scheme != "https" or not parsed.netloc or not parsed.hostname
                or parsed.username is not None or parsed.password is not None
                or parsed.path not in ("", "/") or parsed.query or parsed.fragment
                or "?" in value or "#" in value):
            fail()
        # Accessing port also rejects malformed or out-of-range values.
        if parsed.port is not None and not 1 <= parsed.port <= 65535:
            fail()
        if parsed.netloc.endswith(":"):
            fail()
        # Initial ESP transport is deliberately limited to IPv4 literals, so
        # provisioning cannot introduce an unbounded SDK DNS lookup.
        host = parsed.hostname
        address = ipaddress.IPv4Address(host)
        if str(address) != host:
            fail()
        return "https://" + str(address) + (":" + str(parsed.port) if parsed.port is not None else "")
    except (ValueError, UnicodeError):
        fail()


def validate_ca(value):
    if not isinstance(value, str):
        fail()
    try:
        encoded = value.encode("ascii")
    except UnicodeError:
        fail()
    if not encoded or len(encoded) > MAX_CA_BYTES or "PRIVATE KEY" in value:
        fail()
    # Permit a CA chain but no unrelated PEM blocks, comments or other text.
    position = 0
    count = 0
    for match in CERTIFICATE_PATTERN.finditer(value):
        if value[position:match.start()].strip():
            fail()
        position = match.end()
        count += 1
    if not count or value[position:].strip():
        fail()
    try:
        ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT).load_verify_locations(cadata=value)
    except (ssl.SSLError, ValueError, TypeError):
        fail()
    return value


def no_duplicate_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            fail()
        result[key] = value
    return result


def validate_credentials(value):
    if not isinstance(value, dict) or set(value) != FIELDS:
        fail()
    if value["schema_version"] != SCHEMA_VERSION:
        fail()
    if not printable_ascii(value["device_id"], 1, 64):
        fail()
    if not printable_ascii(value["device_token"], 24, 256):
        fail()
    return Credentials(value["device_id"], validate_origin(value["relay_url"]),
                       value["device_token"], validate_ca(value["ca_pem"]))


def file_is_private(metadata):
    return (stat.S_ISREG(metadata.st_mode) and metadata.st_uid == os.getuid()
            and stat.S_IMODE(metadata.st_mode) == 0o600
            and 0 < metadata.st_size <= MAX_FILE_BYTES)


def load_credentials(path):
    """Read once through a non-following descriptor and validate that descriptor."""
    if not isinstance(path, (str, os.PathLike)) or not Path(path).is_absolute():
        fail()
    parts = Path(path).parts
    if len(parts) < 2 or ".." in parts:
        fail()
    descriptor = None
    directory = None
    try:
        before = os.lstat(path)
        if not file_is_private(before):
            fail()
        # Traverse by directory descriptor so no symlinked ancestor or swapped
        # path component can be followed, including during the final open.
        directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | getattr(os, "O_CLOEXEC", 0)
        directory = os.open(parts[0], directory_flags)
        for component in parts[1:-1]:
            next_directory = os.open(component, directory_flags, dir_fd=directory)
            os.close(directory)
            directory = next_directory
        # O_NONBLOCK prevents a replacement FIFO from blocking before fstat.
        descriptor = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
                             | getattr(os, "O_CLOEXEC", 0), dir_fd=directory)
        opened = os.fstat(descriptor)
        if not file_is_private(opened) or (before.st_dev, before.st_ino) != (opened.st_dev, opened.st_ino):
            fail()
        data = bytearray()
        while len(data) <= MAX_FILE_BYTES:
            chunk = os.read(descriptor, min(4096, MAX_FILE_BYTES + 1 - len(data)))
            if not chunk:
                break
            data.extend(chunk)
        after = os.fstat(descriptor)
        if (not file_is_private(after) or len(data) > MAX_FILE_BYTES
                or len(data) != after.st_size):
            fail()
        decoded = json.loads(data.decode("utf-8"), object_pairs_hook=no_duplicate_keys)
        return validate_credentials(decoded)
    except (OSError, ValueError, UnicodeError, TypeError):
        fail()
    finally:
        if descriptor is not None:
            os.close(descriptor)
        if directory is not None:
            os.close(directory)


def build_frame(credentials, nonce):
    if not isinstance(credentials, Credentials) or not isinstance(nonce, str) or not NONCE_PATTERN.fullmatch(nonce):
        fail()
    # Revalidate before transmission even when called directly by another tool.
    value = {
        "schema_version": SCHEMA_VERSION,
        "device_id": credentials.device_id,
        "relay_url": credentials.relay_url,
        "device_token": credentials.device_token,
        "ca_pem": credentials.ca_pem,
    }
    validate_credentials(value)
    value["nonce"] = nonce
    frame = FRAME_PREFIX + json.dumps(value, separators=(",", ":"), ensure_ascii=True).encode("ascii") + b"\n"
    # The physical reader's line buffer includes the prefix and JSON envelope.
    if len(frame) > MAX_FRAME_BYTES:
        fail()
    return frame


def open_transport(port, serial_factory=None):
    if not isinstance(port, str) or not port or len(port) > 512 or any(ord(char) < 32 or ord(char) == 127 for char in port):
        fail()
    if serial_factory is None:
        # Validation and synthetic tests do not require pyserial to be installed.
        import serial
        serial_factory = serial.Serial
    transport = serial_factory(None, baudrate=115200, timeout=READ_TIMEOUT_S,
                               write_timeout=WRITE_TIMEOUT_S)
    try:
        # Set inactive control lines while closed; opening is the only port action.
        transport.dtr = False
        transport.rts = False
        transport.port = port
        transport.open()
        return transport
    except Exception:
        try:
            transport.close()
        except Exception:
            pass
        fail()


def send_frame(transport, frame, clock):
    deadline = clock() + WRITE_TIMEOUT_S
    position = 0
    while position < len(frame):
        if clock() >= deadline:
            fail()
        end = min(position + CHUNK_BYTES, len(frame))
        written = transport.write(frame[position:end])
        if clock() >= deadline:
            fail()
        if not isinstance(written, int) or isinstance(written, bool) or not 0 < written <= end - position:
            fail()
        position += written


def await_ack(transport, nonce, clock):
    expected_ok = ACK_OK_PREFIX + nonce.encode("ascii")
    expected_error = ACK_ERROR_PREFIX + nonce.encode("ascii")
    deadline = clock() + ACK_TIMEOUT_S
    line = bytearray()
    discard = False
    while True:
        remaining = deadline - clock()
        if remaining <= 0:
            fail()
        transport.timeout = min(READ_TIMEOUT_S, remaining)
        chunk = transport.read(CHUNK_BYTES)
        if clock() >= deadline:
            fail()
        if not isinstance(chunk, bytes) or len(chunk) > CHUNK_BYTES:
            fail()
        for byte in chunk:
            if byte == 10:
                if not discard:
                    candidate = bytes(line)
                    if candidate.endswith(b"\r"):
                        candidate = candidate[:-1]
                    if candidate == expected_ok:
                        return
                    if candidate == expected_error:
                        fail()
                line.clear()
                discard = False
            elif not discard:
                if len(line) >= MAX_ACK_LINE_BYTES:
                    line.clear()
                    discard = True
                else:
                    line.append(byte)


def provision(credentials, port, serial_factory=None, clock=None, nonce_factory=None):
    clock = clock or time.monotonic
    nonce_factory = nonce_factory or secrets.token_urlsafe
    nonce = nonce_factory(24)
    frame = build_frame(credentials, nonce)
    transport = open_transport(port, serial_factory)
    try:
        send_frame(transport, frame, clock)
        await_ack(transport, nonce, clock)
    finally:
        try:
            transport.close()
        except Exception:
            pass


class PrivateArgumentParser(argparse.ArgumentParser):
    def error(self, message):
        # argparse's normal error includes unrecognized argv and their values.
        fail()


def main(argv=None):
    parser = PrivateArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument("--credentials-file", default="", metavar="FILE",
                        help="absolute local JSON file, owned by this user and mode 0600; relay origin must use a literal IPv4 address")
    parser.add_argument("--port", default="", metavar="USB_UART",
                        help="USB UART device; no automatic reboot")
    try:
        args = parser.parse_args(argv)
        credentials = load_credentials(args.credentials_file)
        provision(credentials, args.port)
    except (Exception, KeyboardInterrupt):
        print(FAILURE_MESSAGE, file=sys.stderr)
        return 1
    print(SUCCESS_MESSAGE)
    return 0


if __name__ == "__main__":
    sys.exit(main())
