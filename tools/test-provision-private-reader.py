#!/usr/bin/env python3
"""Synthetic private-reader validation and serial checks; no device access."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

SOURCE = Path(__file__).with_name("provision-private-reader.py")
spec = importlib.util.spec_from_file_location("private_reader_provisioning", SOURCE)
helper = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = helper
spec.loader.exec_module(helper)

# Public certificate from an ephemeral, synthetic test-only CA. Its generated
# private key was discarded; no credential from any archive or device is used.
SYNTHETIC_CA = """-----BEGIN CERTIFICATE-----
MIIBnjCCAUWgAwIBAgIUSvQ1t65RUZ4yRKZPADT+5WkTsDMwCgYIKoZIzj0EAwIw
JTEjMCEGA1UEAwwaU3ludGhldGljIEdsaW1kb2NrIFRlc3QgQ0EwHhcNMjYxMDEz
MTcwNDI3WhcNMzYxMDEwMTcwNDI3WjAlMSMwIQYDVQQDDBpTeW50aGV0aWMgR2xp
bWRvY2sgVGVzdCBDQTBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABEuF2vlK75LD
ClulxIqVkWWo1FoTxgz/n3SdXeR9LmcqEfSOSP0LFAa+wUIv7s+Gc98B/rIlMiYu
bCf/+e3a5xujUzBRMB0GA1UdDgQWBBT/oR/i3lm2dsdjV2ujRqwJ1Sy2ODAfBgNV
HSMEGDAWgBT/oR/i3lm2dsdjV2ujRqwJ1Sy2ODAPBgNVHRMBAf8EBTADAQH/MAoG
CCqGSM49BAMCA0cAMEQCIEWL4X2b+aplzVMJSNIeWMzEhuQpKzeRszIUaRADYo4K
AiBtT3YIzTiuuVPKLqdmtqDwgDW+dxX6B1TKrS4eLm8/jg==
-----END CERTIFICATE-----
"""
SYNTHETIC_TOKEN = "synthetic-device-token-for-tests-only"
SYNTHETIC_ORIGIN = "https://192.0.2.7:8443"
NONCE = "synthetic_nonce_0123456789abcdef"


def sample(**changes):
    value = {"schema_version": helper.SCHEMA_VERSION, "device_id": "test-desk",
             "relay_url": SYNTHETIC_ORIGIN, "device_token": SYNTHETIC_TOKEN,
             "ca_pem": SYNTHETIC_CA}
    value.update(changes)
    return value


class FakeClock:
    def __init__(self, step=0.01):
        self.value = 0.0
        self.step = step

    def __call__(self):
        self.value += self.step
        return self.value


class FakeSerial:
    def __init__(self, replies=b"", short_write=None, write_error=None, read_error=None):
        self.replies = bytearray(replies)
        self.short_write = short_write
        self.write_error = write_error
        self.read_error = read_error
        self.writes = []
        self.events = []
        self.is_open = False
        self._dtr = True
        self._rts = True
        self.port = None
        self.settings = None

    def factory(self, port, **settings):
        self.events.append(("construct", port))
        self.settings = settings
        return self

    @property
    def dtr(self):
        return self._dtr

    @dtr.setter
    def dtr(self, value):
        self.events.append(("dtr", value, self.is_open))
        self._dtr = value

    @property
    def rts(self):
        return self._rts

    @rts.setter
    def rts(self, value):
        self.events.append(("rts", value, self.is_open))
        self._rts = value

    def open(self):
        self.events.append(("open", self.dtr, self.rts))
        self.is_open = True

    def write(self, value):
        if self.write_error:
            raise self.write_error
        count = min(len(value), self.short_write) if self.short_write is not None else len(value)
        self.writes.append(value[:count])
        return count

    def read(self, size):
        if self.read_error:
            raise self.read_error
        count = min(size, len(self.replies))
        value = bytes(self.replies[:count])
        del self.replies[:count]
        return value

    def close(self):
        self.events.append(("close",))
        self.is_open = False


class CredentialTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="glimdock-synthetic-provisioning-")
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name).resolve() / "synthetic.json"
        self.save(sample())

    def save(self, value):
        self.path.write_text(json.dumps(value), encoding="utf-8")
        self.path.chmod(0o600)

    def rejected(self, value):
        with self.assertRaises(helper.ProvisioningError):
            helper.validate_credentials(value)

    def test_private_file_round_trip_and_nonrevealing_representation(self):
        value = helper.load_credentials(str(self.path))
        self.assertEqual(value.device_id, "test-desk")
        self.assertEqual(value.relay_url, SYNTHETIC_ORIGIN)
        self.assertNotIn(SYNTHETIC_TOKEN, repr(value))
        self.assertNotIn(SYNTHETIC_ORIGIN, repr(value))

    def test_exact_fields_schema_and_device_id_bounds(self):
        for value in ({}, [], sample(extra="hidden"), sample(schema_version="wrong"),
                      sample(device_id=""), sample(device_id="x" * 65),
                      sample(device_id="desk device"), sample(device_id="desk\n"),
                      sample(device_id="dévice")):
            with self.subTest(value_type=type(value).__name__):
                self.rejected(value)
        helper.validate_credentials(sample(device_id="x" * 64))

    def test_token_bounds_and_ascii_only(self):
        for token in (None, 12, "x" * 23, "x" * 257, "x" * 24 + " ",
                      "x" * 24 + "\n", "x" * 24 + "\t", "x" * 24 + "\x7f",
                      "x" * 24 + "é"):
            self.rejected(sample(device_token=token))
        for token in ("x" * 24, "x" * 256):
            helper.validate_credentials(sample(device_token=token))

    def test_only_fixed_https_ipv4_origins(self):
        rejected = ("", "http://192.0.2.7", "https://", "https://user@192.0.2.7",
                    "https://user:pass@192.0.2.7", "https://192.0.2.7/path",
                    "https://192.0.2.7/?x=1", "https://192.0.2.7?", "https://192.0.2.7#",
                    "https://192.0.2.7/#fragment", "https://192.0.2.7:0",
                    "https://192.0.2.7:65536", "https://192.0.2.7:",
                    "https://192.0.2.7:abc", "https://192.0.2.7\\other",
                    " https://192.0.2.7", "\nhttps://192.0.2.7",
                    "https://192.0.\t2.7", "https://réley.invalid", "https://relay_invalid",
                    "https://relay.invalid", "https://localhost", "https://[::1]:8443",
                    "https://[::ffff:192.0.2.7]", "https://192.000.2.7", "https://192.0.2",
                    "https://3221225991", "https://0xc0000207", "https://192.0.2.7.",
                    "https://256.0.2.7")
        for origin in rejected:
            with self.subTest(origin=origin):
                self.rejected(sample(relay_url=origin))
        self.assertEqual(helper.validate_credentials(sample(relay_url=SYNTHETIC_ORIGIN + "/")).relay_url, SYNTHETIC_ORIGIN)
        for origin in ("https://192.0.2.7", "https://192.0.2.7:1", "https://192.0.2.7:65535"):
            self.assertEqual(helper.validate_credentials(sample(relay_url=origin)).relay_url, origin)
        self.assertEqual(helper.validate_credentials(sample(relay_url="https://192.0.2.7:08443/")).relay_url, SYNTHETIC_ORIGIN)

    def test_certificate_required_bounded_and_parsed(self):
        for value in (None, "", "x", "é", SYNTHETIC_CA + "x" * 8192,
                      "-----BEGIN PRIVATE KEY-----\nsynthetic\n-----END PRIVATE KEY-----\n" + SYNTHETIC_CA,
                      "prefix\n" + SYNTHETIC_CA, SYNTHETIC_CA + "\n-----BEGIN OTHER-----\nx\n-----END OTHER-----",
                      "-----BEGIN CERTIFICATE-----\ninvalid\n-----END CERTIFICATE-----"):
            self.rejected(sample(ca_pem=value))
        helper.validate_credentials(sample(ca_pem=SYNTHETIC_CA + SYNTHETIC_CA))

    def test_relative_missing_symlink_directory_fifo_and_mode_rejected(self):
        for path in ("relative.json", "", self.path.with_name("missing.json"), self.path.parent):
            with self.assertRaises(helper.ProvisioningError):
                helper.load_credentials(path)
        link = self.path.with_name("link.json")
        link.symlink_to(self.path)
        with self.assertRaises(helper.ProvisioningError):
            helper.load_credentials(link)
        parent_link = self.path.with_name("parent-link")
        parent_link.symlink_to(self.path.parent, target_is_directory=True)
        with self.assertRaises(helper.ProvisioningError):
            helper.load_credentials(parent_link / self.path.name)
        with self.assertRaises(helper.ProvisioningError):
            helper.load_credentials(self.path.parent / ".." / self.path.parent.name / self.path.name)
        fifo = self.path.with_name("fifo.json")
        os.mkfifo(fifo, 0o600)
        with self.assertRaises(helper.ProvisioningError):
            helper.load_credentials(fifo)
        for mode in (0o400, 0o640, 0o644, 0o700, 0o1600):
            self.path.chmod(mode)
            with self.assertRaises(helper.ProvisioningError):
                helper.load_credentials(self.path)

    def test_wrong_owner_rejected_without_opening(self):
        with patch.object(helper.os, "getuid", return_value=os.getuid() + 1), patch.object(helper.os, "open") as opened:
            with self.assertRaises(helper.ProvisioningError):
                helper.load_credentials(self.path)
            opened.assert_not_called()

    def test_descriptor_owner_and_path_swap_rechecked(self):
        opened = self.path.stat()
        changed_owner = types.SimpleNamespace(st_mode=opened.st_mode, st_uid=opened.st_uid + 1,
                                             st_size=opened.st_size, st_dev=opened.st_dev, st_ino=opened.st_ino)
        changed_inode = types.SimpleNamespace(st_mode=opened.st_mode, st_uid=opened.st_uid,
                                             st_size=opened.st_size, st_dev=opened.st_dev, st_ino=opened.st_ino + 1)
        for changed in (changed_owner, changed_inode):
            with patch.object(helper.os, "fstat", return_value=changed):
                with self.assertRaises(helper.ProvisioningError):
                    helper.load_credentials(self.path)

    def test_file_bounds_bad_json_and_duplicate_fields(self):
        for content in (b"", b"x" * (helper.MAX_FILE_BYTES + 1), b"not json", b"\xff",
                        ('{"schema_version":"wrong",' + json.dumps(sample())[1:]).encode()):
            self.path.write_bytes(content)
            self.path.chmod(0o600)
            with self.assertRaises(helper.ProvisioningError):
                helper.load_credentials(self.path)
        # File cap is bytes and includes harmless whitespace, not just JSON fields.
        payload = json.dumps(sample()).encode()
        self.path.write_bytes(payload + b" " * (helper.MAX_FILE_BYTES - len(payload)))
        helper.load_credentials(self.path)


class ProtocolTests(unittest.TestCase):
    def setUp(self):
        self.credentials = helper.validate_credentials(sample())

    def run_provision(self, device, clock=None):
        return helper.provision(self.credentials, "/synthetic/usb-uart", serial_factory=device.factory,
                                clock=clock or FakeClock(), nonce_factory=lambda size: NONCE)

    def test_chunked_frame_exact_fields_and_control_lines_before_open(self):
        device = FakeSerial(helper.ACK_OK_PREFIX + NONCE.encode() + b"\r\n")
        self.run_provision(device)
        self.assertEqual(device.events[:4], [("construct", None), ("dtr", False, False),
                                            ("rts", False, False), ("open", False, False)])
        self.assertEqual(device.events[-1], ("close",))
        self.assertEqual(device.settings["write_timeout"], 2.0)
        self.assertEqual(device.settings["timeout"], 0.1)
        self.assertTrue(all(0 < len(chunk) <= 128 for chunk in device.writes))
        frame = b"".join(device.writes)
        self.assertTrue(frame.startswith(b"GLIMDOCK_PRIVATE_READER "))
        self.assertEqual(frame.count(b"\n"), 1)
        value = json.loads(frame[len(helper.FRAME_PREFIX):])
        self.assertEqual(set(value), helper.FIELDS | {"nonce"})
        self.assertEqual(value["nonce"], NONCE)
        self.assertEqual(value["schema_version"], helper.SCHEMA_VERSION)
        self.assertEqual(value["device_token"], SYNTHETIC_TOKEN)
        self.assertEqual(value["ca_pem"], SYNTHETIC_CA)
        self.assertNotIn(b'": ', frame)

    def test_nonce_is_fresh_bounded_and_urlsafe(self):
        first = helper.secrets.token_urlsafe(24)
        second = helper.secrets.token_urlsafe(24)
        self.assertNotEqual(first, second)
        self.assertTrue(helper.NONCE_PATTERN.fullmatch(first))
        for nonce in ("short", "x" * 49, "x" * 20 + " ", "x" * 20 + "\n"):
            with self.assertRaises(helper.ProvisioningError):
                helper.build_frame(self.credentials, nonce)

    def test_wire_envelope_cap_checked_before_opening(self):
        self.assertEqual(helper.MAX_FRAME_BYTES, 12 * 1024)
        # Allowed PEM whitespace expands to escaped newlines in the wire JSON.
        ca = SYNTHETIC_CA + "\n" * (helper.MAX_CA_BYTES - len(SYNTHETIC_CA.encode("ascii")))
        credentials = helper.validate_credentials(sample(ca_pem=ca))
        self.assertLessEqual(len(json.dumps(sample(ca_pem=ca)).encode()), helper.MAX_FILE_BYTES)
        with patch.object(helper, "open_transport") as opened:
            with self.assertRaises(helper.ProvisioningError):
                helper.provision(credentials, "/synthetic/usb-uart", nonce_factory=lambda size: NONCE)
            opened.assert_not_called()

    def test_hostname_and_ipv6_rejected_before_opening(self):
        for origin in ("https://relay.invalid:8443", "https://[::1]:8443"):
            credentials = helper.Credentials("test-desk", origin, SYNTHETIC_TOKEN, SYNTHETIC_CA)
            with patch.object(helper, "open_transport") as opened:
                with self.assertRaises(helper.ProvisioningError):
                    helper.provision(credentials, "/synthetic/usb-uart", nonce_factory=lambda size: NONCE)
                opened.assert_not_called()

    def test_partial_writes_complete_without_exceeding_chunk_bound(self):
        device = FakeSerial(helper.ACK_OK_PREFIX + NONCE.encode() + b"\n", short_write=17)
        self.run_provision(device, FakeClock(step=0.0001))
        self.assertEqual(b"".join(device.writes), helper.build_frame(self.credentials, NONCE))

    def test_only_correlated_ack_accepted_and_other_lines_never_printed(self):
        replies = (SYNTHETIC_TOKEN.encode() + b"\n" + helper.ACK_OK_PREFIX + b"wrong-nonce\n"
                   + helper.ACK_ERROR_PREFIX + b"other-nonce\n"
                   + b"prefix " + helper.ACK_OK_PREFIX + NONCE.encode() + b"\n"
                   + helper.ACK_OK_PREFIX + NONCE.encode() + b"\n")
        output, errors = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            self.run_provision(FakeSerial(replies))
        self.assertEqual(output.getvalue() + errors.getvalue(), "")

    def test_oversize_noise_stays_discarded_until_next_line(self):
        suffix = helper.ACK_OK_PREFIX + NONCE.encode() + b"\n"
        device = FakeSerial(b"x" * 500 + suffix)
        with self.assertRaises(helper.ProvisioningError):
            self.run_provision(device)
        device = FakeSerial(b"x" * 500 + suffix + suffix)
        self.run_provision(device)

    def test_error_ack_timeout_missing_newline_and_bad_write_fail_closed(self):
        for device in (FakeSerial(helper.ACK_ERROR_PREFIX + NONCE.encode() + b"\n"),
                       FakeSerial(), FakeSerial(helper.ACK_OK_PREFIX + NONCE.encode()),
                       FakeSerial(short_write=0)):
            with self.assertRaises(helper.ProvisioningError):
                self.run_provision(device)
            self.assertEqual(device.events[-1], ("close",))
        with self.assertRaises(helper.ProvisioningError):
            self.run_provision(FakeSerial(), FakeClock(step=3))

    def test_final_write_crossing_deadline_is_rejected(self):
        clock = FakeClock(step=0)
        device = FakeSerial(helper.ACK_OK_PREFIX + NONCE.encode() + b"\n")
        write = device.write

        def slow_final_write(value):
            result = write(value)
            if value.endswith(b"\n"):
                clock.value = 3.0
            return result

        device.write = slow_final_write
        with self.assertRaises(helper.ProvisioningError):
            self.run_provision(device, clock)
        self.assertEqual(device.events[-1], ("close",))

    def test_ack_arriving_after_deadline_is_rejected(self):
        clock = FakeClock(step=0)
        device = FakeSerial(helper.ACK_OK_PREFIX + NONCE.encode() + b"\n")
        read = device.read

        def slow_read(size):
            result = read(size)
            clock.value = 9.0
            return result

        device.read = slow_read
        with self.assertRaises(helper.ProvisioningError):
            self.run_provision(device, clock)
        self.assertEqual(device.events[-1], ("close",))

    def test_exceptions_close_device_and_never_expose_details_in_cli(self):
        provision_real = helper.provision
        for operation in ("write_error", "read_error"):
            device = FakeSerial(**{operation: RuntimeError(SYNTHETIC_TOKEN + SYNTHETIC_ORIGIN)})
            output, errors = io.StringIO(), io.StringIO()
            with patch.object(helper, "load_credentials", return_value=self.credentials), \
                 patch.object(helper, "provision", side_effect=lambda credentials, port: provision_real(
                     credentials, port, serial_factory=device.factory, clock=FakeClock(),
                     nonce_factory=lambda size: NONCE)), \
                 contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
                result = helper.main(["--credentials-file", "/synthetic/secret.json", "--port", "/synthetic/usb-uart"])
            self.assertEqual(result, 1)
            self.assertEqual(output.getvalue(), "")
            self.assertEqual(errors.getvalue(), helper.FAILURE_MESSAGE + "\n")
            self.assertEqual(device.events[-1], ("close",))

    def test_cli_success_has_only_generic_output(self):
        output, errors = io.StringIO(), io.StringIO()
        with patch.object(helper, "load_credentials", return_value=self.credentials), \
             patch.object(helper, "provision") as provision, \
             contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            result = helper.main(["--credentials-file", "/synthetic/private.json", "--port", "/synthetic/usb-uart"])
        self.assertEqual(result, 0)
        self.assertEqual(output.getvalue(), helper.SUCCESS_MESSAGE + "\n")
        self.assertEqual(errors.getvalue(), "")
        provision.assert_called_once_with(self.credentials, "/synthetic/usb-uart")

    def test_cli_parse_validation_import_and_interrupt_failures_are_generic(self):
        for arguments in ([], ["--unknown-secret", SYNTHETIC_TOKEN], ["--credentials-file"],
                          ["--port"], ["--cred", SYNTHETIC_TOKEN], [SYNTHETIC_ORIGIN]):
            output, errors = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
                result = helper.main(arguments)
            self.assertEqual(result, 1)
            self.assertEqual(output.getvalue(), "")
            self.assertEqual(errors.getvalue(), helper.FAILURE_MESSAGE + "\n")
        for exception in (ImportError(SYNTHETIC_TOKEN), KeyboardInterrupt()):
            output, errors = io.StringIO(), io.StringIO()
            with patch.object(helper, "load_credentials", return_value=self.credentials), \
                 patch.object(helper, "provision", side_effect=exception), \
                 contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
                result = helper.main(["--credentials-file", "/synthetic/private.json", "--port", "/synthetic/usb-uart"])
            self.assertEqual(result, 1)
            self.assertEqual(errors.getvalue(), helper.FAILURE_MESSAGE + "\n")


if __name__ == "__main__":
    unittest.main()
