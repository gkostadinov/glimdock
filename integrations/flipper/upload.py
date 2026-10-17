#!/usr/bin/env python3
"""Verify a USB Flipper and install only its Glimdock SD-card application."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "output" / "flipper-status"
TARGET = "/ext/apps/Tools/glimdock.fap"
VID, PID = 0x0483, 0x5740


def verified_port(port):
    return (port.vid, port.pid) == (VID, PID) and (port.serial_number or "").lower().startswith("flip_")


def parse_info(raw):
    info = {}
    for line in raw.decode("ascii", errors="replace").splitlines():
        match = re.fullmatch(r"\s*([a-zA-Z0-9_.]+)\s*:\s*(.*?)\s*", line)
        if match:
            info[match[1].replace(".", "_")] = match[2]
    if info.get("hardware_model") != "Flipper Zero" or info.get("firmware_target") != "7":
        raise RuntimeError("Device did not identify as a Flipper Zero target 7")
    for key in ("hardware_uid", "firmware_version", "firmware_api_major", "firmware_api_minor"):
        if not info.get(key):
            raise RuntimeError("Device information is missing " + key)
    int(info["firmware_api_major"])
    int(info["firmware_api_minor"])
    return info


def artifact_directory(version):
    if version == "mntm-012":
        return OUTPUT / "momentum-mntm-012"
    if re.fullmatch(r"\d+\.\d+\.\d+(?:-rc\d+)?", version):
        return OUTPUT
    raise RuntimeError("This firmware needs a matching reviewed SDK before installation: " + version)


def sdk_api(directory=OUTPUT):
    symbols = directory / "sdk/current/sdk_headers/f7_sdk/targets/f7/api_symbols.csv"
    match = re.search(r"^Version,\+,([0-9]+)\.([0-9]+),", symbols.read_text(), re.MULTILINE)
    if not match:
        raise RuntimeError("Cannot read installed SDK API version")
    return int(match[1]), int(match[2])


def validate_api(info, required):
    actual = int(info["firmware_api_major"]), int(info["firmware_api_minor"])
    if actual[0] != required[0] or actual[1] < required[1]:
        raise RuntimeError(f"Firmware API {actual[0]}.{actual[1]} cannot run this app's API "
                           f"{required[0]}.{required[1]}; no file was written")


def close_application(storage, timeout=10.0):
    # The loader acknowledges an exit signal before the app has finished
    # disconnecting and restoring its Bluetooth profile. Do not replace its
    # loaded FAP until the loader confirms that teardown has completed.
    result = storage.send_and_wait_prompt("loader close\r")
    if b"was closed" not in result and b"No application" not in result:
        raise RuntimeError("Cannot close the current Flipper app: " +
                           result.decode("ascii", "replace"))
    deadline = time.monotonic() + timeout
    while True:
        result = storage.send_and_wait_prompt("loader info\r")
        if b"No application is running" in result:
            return
        if time.monotonic() >= deadline:
            raise RuntimeError("The Flipper app is still stopping; no file was written")
        time.sleep(.1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", help="Exact port; automatic selection requires one verified Flipper")
    parser.add_argument("--inspect", action="store_true", help="Read identity/version only; do not upload")
    args = parser.parse_args()
    runtime = OUTPUT / "sdk/toolchain/current/bin/python3"
    if not runtime.exists():
        raise RuntimeError("Build the app first with python3 integrations/flipper/build.py")
    if Path(sys.executable).resolve() != runtime.resolve():
        os.execv(str(runtime), [str(runtime), str(Path(__file__).resolve()), *sys.argv[1:]])
    from serial.tools import list_ports
    sys.path.insert(0, str(OUTPUT / "sdk/current/scripts"))
    from flipper.storage import BufferedRead, FlipperStorage

    class BoundedRead(BufferedRead):
        def until(self, eol="\n", cut_eol=True, timeout=None):
            return super().until(eol, cut_eol, 10.0 if timeout is None else timeout)

    def choose_port():
        candidates = [port for port in list_ports.comports() if verified_port(port)]
        if args.port:
            candidates = [port for port in candidates if port.device == args.port]
        if len(candidates) != 1:
            raise RuntimeError("Expected one USB Flipper with VID:PID 0483:5740 and flip_ serial; "
                               "use --port if more than one is connected")
        return candidates[0]

    def connect(port):
        storage = FlipperStorage(port.device)
        storage.read = BoundedRead(storage.port)
        return storage

    port = choose_port()
    with connect(port) as storage:
        info = parse_info(storage.send_and_wait_prompt("device_info\r"))
    public = {key: info[key] for key in
              ("hardware_model", "firmware_version", "firmware_api_major", "firmware_api_minor")}
    print(json.dumps(public, indent=2), flush=True)
    if args.inspect:
        return
    version = info["firmware_version"]
    directory = artifact_directory(version)
    manifest_path = directory / "build-manifest.json"
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {}
    if manifest.get("firmware_sdk") != version:
        option = "--momentum" if version == "mntm-012" else "--sdk"
        subprocess.run([str(OUTPUT / "venv/bin/python"), str(Path(__file__).with_name("build.py")),
                        option, version], check=True)
        manifest = json.loads(manifest_path.read_text())
    if manifest.get("firmware_sdk") != version:
        raise RuntimeError("Built SDK does not match installed firmware")
    validate_api(info, sdk_api(directory))
    binary = directory / "dist/glimdock.fap"
    payload = binary.read_bytes()
    checksum = hashlib.sha256(payload).hexdigest()
    if checksum != manifest["sha256"]:
        raise RuntimeError("App binary does not match its build manifest")

    # Recheck after a possible SDK download/rebuild; a replug must not change the target.
    current = choose_port()
    if (current.device, current.serial_number) != (port.device, port.serial_number):
        raise RuntimeError("USB device changed while preparing the app")
    with connect(current) as storage:
        fresh = parse_info(storage.send_and_wait_prompt("device_info\r"))
        for key in ("hardware_uid", "firmware_version", "firmware_api_major", "firmware_api_minor"):
            if fresh[key] != info[key]:
                raise RuntimeError("Device identity or firmware changed before upload")
        close_application(storage)
        for directory in ("/ext/apps", "/ext/apps/Tools"):
            if not storage.exist_dir(directory):
                storage.mkdir(directory)
        if storage.exist_file(TARGET):
            (OUTPUT / "previous-glimdock.fap").write_bytes(storage.read_file(TARGET))
        storage.send_file(str(binary), TARGET)
        received = bytes(storage.read_file(TARGET))
        if hashlib.sha256(received).hexdigest() != checksum:
            raise RuntimeError("Application read-back verification failed")
    result = {
        **public, "device_path": TARGET, "bytes": len(payload), "sha256": checksum,
        "build_manifest": str(manifest_path.relative_to(ROOT)),
        "verified_readback": True, "firmware_flashing": False, "app_launched": False,
    }
    (OUTPUT / "upload-result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, ValueError, subprocess.CalledProcessError) as error:
        print("Upload stopped:", error, file=sys.stderr)
        sys.exit(1)
