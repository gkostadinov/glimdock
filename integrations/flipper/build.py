#!/usr/bin/env python3
"""Stage the canonical protocol and build the Flipper FAP without flashing it."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import urllib.request
import venv

ROOT = Path(__file__).resolve().parents[2]
SOURCE = Path(__file__).resolve().parent
OUTPUT = ROOT / "output" / "flipper-status"
SDK_VERSION = "1.5.1"
MOMENTUM = {
    "mntm-012": {
        "url": "https://github.com/Next-Flip/Momentum-Firmware/releases/download/mntm-012/flipper-z-f7-sdk-mntm-012.zip",
        "sha256": "c90530c3e0aa7bc4356b8521df20d23d7b4a5435ab58a2530ffb696200408cc2",
        "api": "87.1",
    },
}


def run(command, **kwargs):
    print("+", " ".join(map(str, command)), flush=True)
    subprocess.run(list(map(str, command)), check=True, **kwargs)


def test(output=OUTPUT):
    output.mkdir(parents=True, exist_ok=True)
    binary = output / "test-status"
    run([os.environ.get("CC", "cc"), "-std=c99", "-Wall", "-Wextra", "-Werror",
         "-I" + str(ROOT / "firmware" / "src"), "-I" + str(SOURCE),
         SOURCE / "tests" / "test_status.c", "-o", binary])
    run([binary])
    run([sys.executable, ROOT / "tools" / "test-flipper-ble.py"])
    run([sys.executable, ROOT / "tools" / "test-flipper-cache.py"])
    run([sys.executable, "-m", "unittest", "discover", "-s", SOURCE / "tests", "-p", "test_*.py"])


def build(sdk_version=SDK_VERSION, momentum=None):
    preset = MOMENTUM.get(momentum) if momentum else None
    if momentum and not preset:
        raise ValueError("Unsupported Momentum release")
    if not momentum and not re.fullmatch(r"\d+\.\d+\.\d+(?:-rc\d+)?", sdk_version):
        raise ValueError("SDK must be an official release version, for example 1.5.1")
    output = OUTPUT / ("momentum-" + momentum) if momentum else OUTPUT
    sdk_version = momentum or sdk_version
    test(output)
    # The fixed Momentum preset uses the same official toolchain version 39.
    # Its SDK/build state and artifact remain isolated from the stock build.
    python = OUTPUT / "venv" / "bin" / "python"
    if not python.exists():
        venv.create(OUTPUT / "venv", with_pip=True)
    ufbt = python.parent / "ufbt"
    if not ufbt.exists():
        run([python, "-m", "pip", "install", "ufbt==0.2.6"])
    stage = output / "app"
    stage.mkdir(parents=True, exist_ok=True)
    app_sources = ("application.fam", "glimdock.c", "glimdock_ble.c", "glimdock_ble.h", "glimdock_hci.h", "node_cache.h", "status_view.h")
    for name in app_sources:
        shutil.copy2(SOURCE / name, stage / name)
    protocol = ROOT / "firmware" / "src" / "flipper_protocol.h"
    shutil.copy2(protocol, stage / protocol.name)
    ble_protocol = ROOT / "firmware" / "src" / "flipper_ble_protocol.h"
    shutil.copy2(ble_protocol, stage / ble_protocol.name)
    cache_protocol = ROOT / "firmware" / "src" / "flipper_cache_protocol.h"
    shutil.copy2(cache_protocol, stage / cache_protocol.name)
    environment = dict(os.environ, UFBT_HOME=str(output / "sdk"))
    if momentum:
        environment["FBT_TOOLCHAIN_PATH"] = str(OUTPUT / "sdk")
        archive = output / "sdk-archive.zip"
        if not archive.exists():
            temporary = archive.with_suffix(".download")
            request = urllib.request.Request(preset["url"], headers={"User-Agent": "Glimdock-Flipper-Build"})
            with urllib.request.urlopen(request, timeout=60) as source, temporary.open("wb") as target:
                shutil.copyfileobj(source, target)
            temporary.replace(archive)
        if hashlib.sha256(archive.read_bytes()).hexdigest() != preset["sha256"]:
            raise RuntimeError("Momentum SDK archive does not match its reviewed release SHA-256")
    marker = output / "sdk-version.txt"
    if not marker.exists() or marker.read_text().strip() != sdk_version:
        selection = "--local=" + str(archive) if momentum else "--branch=" + sdk_version
        run([ufbt, "update", selection, "--hw-target=f7"], cwd=stage, env=environment)
        marker.write_text(sdk_version + "\n")
    symbols = output / "sdk/current/sdk_headers/f7_sdk/targets/f7/api_symbols.csv"
    api = re.search(r"^Version,\+,([^,]+)", symbols.read_text(), re.MULTILINE)
    if not api or (momentum and api[1] != preset["api"]):
        raise RuntimeError("SDK API does not match the selected release")
    run([ufbt], cwd=stage, env=environment)
    destination = output / "dist"
    destination.mkdir(exist_ok=True)
    fap = destination / "glimdock.fap"
    shutil.copy2(stage / "dist" / "glimdock.fap", fap)
    manifest = {
        "firmware_sdk": sdk_version,
        "firmware_family": "momentum" if momentum else "official",
        "sdk_api": api[1],
        "ufbt": "0.2.6",
        "target": "f7",
        "binary": str(fap.relative_to(ROOT)),
        "bytes": fap.stat().st_size,
        "sha256": hashlib.sha256(fap.read_bytes()).hexdigest(),
        "canonical_protocol_sha256": hashlib.sha256(protocol.read_bytes()).hexdigest(),
        "canonical_ble_protocol_sha256": hashlib.sha256(ble_protocol.read_bytes()).hexdigest(),
        "canonical_cache_protocol_sha256": hashlib.sha256(cache_protocol.read_bytes()).hexdigest(),
        "source_sha256": {name: hashlib.sha256((stage / name).read_bytes()).hexdigest()
                          for name in (*app_sources, protocol.name, ble_protocol.name, cache_protocol.name)},
        "device_path": "/ext/apps/Tools/glimdock.fap",
        "transport": "native-gatt-cache-v2",
        "ble_service_uuid": re.search(r'#define GD_BLE_SERVICE_UUID "([^"]+)"', ble_protocol.read_text())[1],
        "ble_control_uuid": re.search(r'#define GD_BLE_CONTROL_UUID "([^"]+)"', ble_protocol.read_text())[1],
        "flashed": False,
    }
    if momentum:
        manifest["sdk_url"] = preset["url"]
        manifest["sdk_sha256"] = preset["sha256"]
    (output / "build-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test", action="store_true", help="Run host decoder/navigation tests only")
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--sdk", default=SDK_VERSION, help="Official firmware SDK release (default: 1.5.1)")
    selection.add_argument("--momentum", choices=sorted(MOMENTUM), help="Build for a reviewed Momentum release")
    args = parser.parse_args()
    test() if args.test else build(args.sdk, args.momentum)
