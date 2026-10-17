import importlib.util
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("upload", Path(__file__).resolve().parents[1] / "upload.py")
upload = importlib.util.module_from_spec(spec)
spec.loader.exec_module(upload)


class UploadGuards(unittest.TestCase):
    def test_waits_for_application_teardown_before_upload(self):
        class Storage:
            def __init__(self, replies):
                self.replies, self.commands = iter(replies), []

            def send_and_wait_prompt(self, command):
                self.commands.append(command)
                return next(self.replies)

        storage = Storage([b"Application was closed", b"Application: Glimdock",
                           b"No application is running\r\n"])
        with patch.object(upload.time, "sleep"):
            upload.close_application(storage)
        self.assertEqual(storage.commands, ["loader close\r", "loader info\r", "loader info\r"])
        with self.assertRaises(RuntimeError):
            upload.close_application(Storage([b"Application was closed", b"Application: Glimdock"]), 0)
        with self.assertRaises(RuntimeError):
            upload.close_application(Storage([b"Close failed"]))

    def test_firmware_selects_isolated_reviewed_sdk(self):
        self.assertEqual(upload.artifact_directory("mntm-012"), upload.OUTPUT / "momentum-mntm-012")
        self.assertEqual(upload.artifact_directory("1.5.1"), upload.OUTPUT)
        for version in ("mntm-dev", "mntm-013", "../../sdk", "1.5.1;cmd", "unknown"):
            with self.assertRaises(RuntimeError):
                upload.artifact_directory(version)

    def test_usb_identity_requires_both_vid_pid_and_flipper_serial(self):
        self.assertTrue(upload.verified_port(SimpleNamespace(vid=0x0483, pid=0x5740, serial_number="flip_Test")))
        for vid, pid, serial in ((0x0483, 0x5740, "stm32"), (0x303A, 0x5740, "flip_Test"),
                                 (0x0483, 0xDF11, "flip_Test"), (0x0483, 0x5740, None)):
            self.assertFalse(upload.verified_port(SimpleNamespace(vid=vid, pid=pid, serial_number=serial)))

    def test_device_information_and_api_must_match(self):
        raw = (b"hardware_model : Flipper Zero\r\nhardware_uid : ABCD\r\n"
               b"firmware_target : 7\r\nfirmware_version : 1.5.1\r\n"
               b"firmware_api_major : 88\r\nfirmware_api_minor : 2\r\n")
        info = upload.parse_info(raw)
        upload.validate_api(info, (88, 2))
        upload.validate_api(info, (88, 1))
        for required in ((88, 3), (87, 2), (89, 0)):
            with self.assertRaises(RuntimeError):
                upload.validate_api(info, required)
        with self.assertRaises(RuntimeError):
            upload.parse_info(raw.replace(b"Flipper Zero", b"Other Device"))
        with self.assertRaises(RuntimeError):
            upload.parse_info(raw.replace(b"firmware_target : 7", b"firmware_target : 6"))
        with self.assertRaises(RuntimeError):
            upload.parse_info(raw.replace(b"hardware_uid : ABCD\r\n", b""))


if __name__ == "__main__":
    unittest.main()
