#!/usr/bin/env python3
"""Build an unpaired Web Serial image and its corresponding source/relink bundle.

Only the dedicated public environment is cleaned. No USB port is opened. The
output is staged and audited before replacing collector-web/firmware assets.
"""
from __future__ import annotations

import argparse
import ast
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]
ENVIRONMENT = "homelab_s3_web"
FLASH_SIZE = 16 * 1024 * 1024
NVS_START, NVS_END = 0x9000, 0xE000
APP_LIMIT = 0x510000
FIRMWARE_FILES = (
    "platformio.ini", "partitions.csv", "build_and_run.sh", "README.md",
    "include/config.h", "include/config.example.h", "include/lv_conf.h", "include/lv_psram.h",
    "src/main.cpp", "src/model.h", "src/network.cpp", "src/board.cpp", "src/board.h",
    "src/touch_state.h", "src/pairing_policy.h", "src/brand.h", "src/brand_assets.h", "src/brand_portal.h",
)
PRIVATE_NAMES = {"local_credentials.h", "private_defaults.h", "secrets.h"}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def image_part(path: Path, offset: int, name: str) -> dict:
    data = path.read_bytes()
    rounded_end = offset + ((len(data) + 4095) // 4096) * 4096
    if not data or offset % 4096 or rounded_end > FLASH_SIZE:
        raise ValueError(f"Invalid flash range for {name}")
    if offset < NVS_END and rounded_end > NVS_START:
        raise ValueError(f"Rounded flash range intersects preserved NVS: {name}")
    if name == "firmware.bin" and rounded_end > APP_LIMIT:
        raise ValueError("Application exceeds the configured factory partition")
    return {"offset": offset, "size": len(data), "sha256": digest(data), "path": f"/firmware/{name}"}


def private_values() -> list[bytes]:
    """Read local defaults privately; their values never enter output or errors."""
    values = []
    pattern = re.compile(r'^\s*#define\s+HOMELAB_DEFAULT_(?:SSID|PASSWORD|TOKEN|SETUP_TOKEN|ENDPOINT)\s+("(?:[^"\\]|\\.)*")', re.M)
    for name in PRIVATE_NAMES:
        path = ROOT / "firmware/include" / name
        if not path.is_file():
            continue
        for match in pattern.finditer(path.read_text()):
            value = ast.literal_eval(match.group(1))
            if value:
                values.append(value.encode())
    return list(dict.fromkeys(values))


def audit_bytes(data: bytes, secrets: list[bytes]) -> None:
    if any(secret in data for secret in secrets):
        raise ValueError("Public bundle contains a private build default; no assets were published")


def add_file(files: dict[str, bytes], destination: str, source: Path) -> None:
    if source.is_symlink() or not source.is_file():
        raise ValueError(f"Required regular source file missing: {destination}")
    if source.name in PRIVATE_NAMES:
        raise ValueError("Private header rejected")
    files[destination] = source.read_bytes()


def add_tree(files: dict[str, bytes], destination: str, source: Path) -> None:
    if not source.is_dir():
        raise ValueError(f"Required source directory missing: {destination}")
    for path in sorted(source.rglob("*")):
        if path.is_symlink():
            resolved = path.resolve()
            if not resolved.is_file() or not resolved.is_relative_to(source.resolve()):
                raise ValueError(f"Unsafe symlink rejected in source package: {destination}")
            # Some toolchain license files share one canonical license by a
            # contained symlink. Materialize bytes; the archive has no links.
            add_file(files, f"{destination}/{path.relative_to(source).as_posix()}", resolved)
            continue
        if path.is_file() and path.name != ".DS_Store" and "__pycache__" not in path.parts:
            add_file(files, f"{destination}/{path.relative_to(source).as_posix()}", path)


def sanitize(text: str, framework: Path, packages: Path) -> str:
    replacements = [(str(ROOT / "firmware"), "${PROJECT_DIR}"), (str(ROOT), "${BUNDLE_PROJECT}"),
                    (str(framework), "${FRAMEWORK_DIR}"), (str(packages), "${PACKAGES_DIR}"),
                    (str(Path.home()), "${USER_HOME}")]
    for old, new in sorted(replacements, key=lambda item: len(item[0]), reverse=True):
        text = text.replace(old, new)
    return text


REBUILD_SCRIPT = '''#!/usr/bin/env python3
"""Rebuild with the supplied library sources; optionally reuse original app objects."""
import argparse,json,os,shutil,subprocess
from pathlib import Path

root=Path(__file__).resolve().parent
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument("--pio",default=shutil.which("pio") or shutil.which("platformio"))
parser.add_argument("--preserve-application-objects",action="store_true")
args=parser.parse_args()
if not args.pio:parser.error("Install the pinned PlatformIO Core listed in build-inventory.json")
project=root/"firmware"
cache=root/"rebuild-cache"
environment=dict(os.environ,PLATFORMIO_CORE_DIR=str(cache/"pio-core"))
def run(*arguments):subprocess.run([args.pio,*arguments],env=environment,check=True)
# This is an isolated package cache, so modified sources never overwrite a
# framework used by an unrelated project on this computer.
run("pkg","install","--project-dir",str(project),"--environment","homelab_s3_web")
framework=cache/"pio-core/packages/framework-arduinoespressif32"
inventory=json.loads((root/"build-inventory.json").read_text())
installed=json.loads((framework/"package.json").read_text())
if installed["version"]!=inventory["framework"]["version"]:
    raise SystemExit("Framework version differs from the exact bundle inventory")
for name,key in [("toolchain-xtensa-esp32s3","toolchain"),("tool-esptoolpy","esptool")]:
    installed=json.loads((cache/"pio-core/packages"/name/"package.json").read_text())
    if installed["version"]!=inventory[key]["version"]:
        raise SystemExit("Tool version differs from the exact bundle inventory")
for name in ["cores","libraries","variants"]:
    shutil.copytree(root/"dependencies/arduino-framework"/name,framework/name,dirs_exist_ok=True)
# Vendored MIT dependencies are compiled from their supplied sources, too.
for name in ["lvgl","ArduinoJson"]:
    shutil.copytree(root/"dependencies"/name,project/"lib"/name,dirs_exist_ok=True)
configuration=(project/"platformio.ini").read_text()
start=configuration.index("lib_deps =")
end=configuration.index("build_unflags =",start)
configuration=configuration[:start]+configuration[end:]
if args.preserve_application_objects:
    configuration+="\\n[env:homelab_s3_relink]\\nextends = env:homelab_s3_web\\nextra_scripts = pre:../relink-application.py\\n"
(project/"platformio-relink.ini").write_text(configuration)
run("run","--project-dir",str(project),"--project-conf",str(project/"platformio-relink.ini"),
    "--environment","homelab_s3_relink" if args.preserve_application_objects else "homelab_s3_web")
print("Rebuilt image is in firmware/.pio/build/<environment>/firmware.bin")
'''

RELINK_SCRIPT = '''# Invoked by PlatformIO/SCons only for --preserve-application-objects.
Import("env")
from pathlib import Path
import shutil
def original_application(source,target,env):
    root=Path(env.subst("$PROJECT_DIR")).parent
    build=Path(env.subst("$BUILD_DIR"))
    for path in (root/"relink/objects/src").glob("*.o"):
        destination=build/"src"/path.name
        destination.parent.mkdir(parents=True,exist_ok=True)
        shutil.copyfile(path,destination)
# A modified Arduino core/library is rebuilt normally. Only the unchanged
# original MIT application objects replace their freshly compiled equivalents.
env.AddPreAction("$BUILD_DIR/${PROGNAME}.elf",original_application)
env.AlwaysBuild("$BUILD_DIR/${PROGNAME}.elf")
'''


def source_archive(stage: Path, build: Path, framework: Path, packages: Path, log: str,
                   pio: str, secrets: list[bytes]) -> dict:
    files: dict[str, bytes] = {}
    prefix = "source-relink"
    for name in FIRMWARE_FILES:
        add_file(files, f"{prefix}/firmware/{name}", ROOT / "firmware" / name)
    for name in ("LICENSE", "LICENSE_POLICY.md", "THIRD_PARTY_NOTICES.md", "docs/DRIVER_PROVENANCE.md", "branding/Space-Grotesk-OFL.txt"):
        add_file(files, f"{prefix}/{name}", ROOT / name)
    for path in sorted((ROOT / "LICENSES").glob("*")):
        if path.is_file() and not path.name.startswith(("Rust", "Musl", "CERN")):
            add_file(files, f"{prefix}/LICENSES/{path.name}", path)
    for name in ("cores", "libraries"):
        add_tree(files, f"{prefix}/dependencies/arduino-framework/{name}", framework / name)
    add_tree(files, f"{prefix}/dependencies/arduino-framework/variants/esp32s3box", framework / "variants/esp32s3box")
    add_file(files, f"{prefix}/dependencies/arduino-framework/package.json", framework / "package.json")
    add_file(files, f"{prefix}/dependencies/arduino-framework/LICENSE.md", ROOT / "LICENSES/Arduino-ESP32-2.0.17-LGPL-2.1.md")
    for path in sorted((framework / "tools").glob("platformio-build*.py")):
        add_file(files, f"{prefix}/dependencies/arduino-framework/tools/{path.name}", path)
    for path in sorted((framework / "tools/partitions").glob("*.csv")):
        add_file(files, f"{prefix}/dependencies/arduino-framework/tools/partitions/{path.name}", path)
    add_file(files, f"{prefix}/dependencies/arduino-framework/tools/gen_esp32part.py", framework / "tools/gen_esp32part.py")
    add_file(files, f"{prefix}/dependencies/arduino-framework/sdk/esp32s3/sdkconfig", framework / "tools/sdk/esp32s3/sdkconfig")
    for path in sorted((framework / "tools/sdk/esp32s3").rglob("*.ld")):
        add_file(files, f"{prefix}/dependencies/arduino-framework/sdk/esp32s3/{path.relative_to(framework/'tools/sdk/esp32s3').as_posix()}", path)
    dependency_root = ROOT / "firmware/.pio/libdeps" / ENVIRONMENT
    lvgl = dependency_root / "lvgl"
    for name in ("src", "libs", "env_support", "scripts/built_in_font/font_license"):
        add_tree(files, f"{prefix}/dependencies/lvgl/{name}", lvgl / name)
    for path in sorted(lvgl.iterdir()):
        if path.is_file() and path.name != ".piopm":
            add_file(files, f"{prefix}/dependencies/lvgl/{path.name}", path)
    add_tree(files, f"{prefix}/dependencies/ArduinoJson", dependency_root / "ArduinoJson")
    toolchain = packages / "toolchain-xtensa-esp32s3"
    add_tree(files, f"{prefix}/LICENSES/toolchain", toolchain / "share/licenses")
    for path in sorted((build / "src").glob("*.o")):
        add_file(files, f"{prefix}/relink/objects/src/{path.name}", path)
    add_file(files, f"{prefix}/relink/objects/libFrameworkArduino.a", build / "libFrameworkArduino.a")
    for path in sorted(build.glob("lib*/*.a")):
        add_file(files, f"{prefix}/relink/objects/{path.relative_to(build).as_posix()}", path)
    files[f"{prefix}/relink/firmware.map"] = sanitize((build / "firmware.map").read_text(), framework, packages).encode()
    files[f"{prefix}/relink/build-commands.txt"] = sanitize(log, framework, packages).encode()
    link_lines = [line for line in log.splitlines() if "xtensa-esp32s3-elf-g++ " in line and "firmware.elf" in line and " -c " not in line]
    if len(link_lines) != 1:
        raise ValueError("Exact public link command missing")
    linked_libraries = sorted({argument[2:] for argument in shlex.split(link_lines[0]) if argument.startswith("-l") and len(argument) > 2})
    inventory = {"schema": 1, "environment": ENVIRONMENT,
                 "platform": "espressif32@6.13.0", "framework": json.loads((framework / "package.json").read_text()),
                 "platformio_core": subprocess.check_output([pio, "--version"], text=True).strip(),
                 "toolchain": json.loads((toolchain / "package.json").read_text()),
                 "esptool": json.loads((packages / "tool-esptoolpy/package.json").read_text()),
                 "dependencies": {"lvgl": "9.3.0", "ArduinoJson": "7.4.2"}, "linked_sdk_libraries": linked_libraries,
                 "public_defaults": True, "private_defaults_included": False, "application_objects": 3,
                 "source_scope": "Exact installed Arduino cores, libraries and selected board variant; complete LVGL source/libs and notices; ArduinoJson; original firmware; SDK linker/configuration and notices. Pinned PlatformIO packages supply the remaining ESP-IDF SDK and toolchain."}
    files[f"{prefix}/build-inventory.json"] = (json.dumps(inventory, indent=2) + "\n").encode()
    files[f"{prefix}/rebuild.py"] = REBUILD_SCRIPT.encode()
    files[f"{prefix}/relink-application.py"] = RELINK_SCRIPT.encode()
    files[f"{prefix}/README.md"] = b'''# Corresponding firmware source and practical relink materials

This bundle accompanies the downloadable Glimdock ESP32-S3 Waveshare V1 image.
Original application source is MIT licensed. Arduino core/libraries retain
LGPL-2.1-or-later and individual notices; ESP-IDF, LVGL, fonts, ArduinoJson and
compiler runtime terms are retained under LICENSES and dependencies.

Install the PlatformIO Core version in build-inventory.json, then run:

    python3 rebuild.py

The helper creates an isolated package cache, installs the pinned platform and
checks its exact Arduino framework version. It overlays the supplied Arduino
source (including any modifications you make), builds the vendored LVGL and
ArduinoJson source, and links a public unpaired application. No flash action runs.

To rebuild the LGPL libraries and relink the original application object files:

    python3 rebuild.py --preserve-application-objects

The relink hook replaces only original application objects immediately before
linking. Arduino core and libraries are rebuilt from your modified sources.
Original object/archive files, a linker map and the actual compile/link commands
are supplied under relink/. Paths in command/map text are represented by
${PROJECT_DIR}, ${FRAMEWORK_DIR}, ${PACKAGES_DIR}, ${BUNDLE_PROJECT} and
${USER_HOME}; the helper
uses current absolute paths automatically. The original core archive is a
reference; it is not substituted for your rebuilt, modified core.

The package excludes private Wi-Fi/token defaults and runtime snapshots. The
GLIMDOCK_PUBLIC_FIRMWARE build flag deliberately starts new displays unpaired.
Saved display pairing in NVS survives USB updates. USB flashing is available
using esptool or the collector Web Serial interface; no signing key or locked
bootloader prevents installation of a modified image. Modification and reverse
engineering for debugging library changes are permitted by the retained terms.

ESP-IDF SDK/toolchain executables are fetched as pinned PlatformIO packages,
not duplicated here. Exact installed package metadata, SDK library names, SDK
configuration and linker scripts accompany the sources. The included notices
distinguish linked libraries from an overinclusive SDK license inventory.
'''
    for name, data in files.items():
        if any(part in PRIVATE_NAMES for part in Path(name).parts) or ".." in Path(name).parts:
            raise ValueError("Unsafe source archive entry")
        audit_bytes(data, secrets)
    ledger = {name.removeprefix(prefix + "/"): {"size": len(data), "sha256": digest(data)} for name, data in sorted(files.items())}
    files[f"{prefix}/files.json"] = (json.dumps(ledger, indent=2) + "\n").encode()
    destination = stage / "source-relink.tar.gz"
    with destination.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0, compresslevel=9) as compressed:
            with tarfile.open(fileobj=compressed, mode="w|", format=tarfile.PAX_FORMAT) as archive:
                for name, data in sorted(files.items()):
                    info = tarfile.TarInfo(name)
                    info.size = len(data)
                    info.mode = 0o755 if name.endswith("/rebuild.py") else 0o644
                    info.mtime = 0
                    archive.addfile(info, io.BytesIO(data))
    with tarfile.open(destination, "r:gz") as archive:
        members = archive.getmembers()
        if len(members) != len(files) or any(not member.isfile() for member in members):
            raise ValueError("Source archive audit failed")
        for member in members:
            data = archive.extractfile(member).read()
            if digest(data) != digest(files[member.name]):
                raise ValueError("Source archive fingerprint mismatch")
            audit_bytes(data, secrets)
    return {"size": destination.stat().st_size, "sha256": digest(destination.read_bytes()), "files": len(files), "private_default_matches": 0}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pio", default=shutil.which("pio") or shutil.which("platformio") or str(Path.home()/".miniconda3/bin/pio"))
    parser.add_argument("--output", type=Path, default=ROOT/"collector-web/firmware")
    args = parser.parse_args()
    pio = str(Path(args.pio).resolve())
    if not Path(pio).is_file():
        parser.error("PlatformIO executable was not found")
    log_path = ROOT / "output/hardware/web-firmware-build.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    build_inputs = {name: digest((ROOT/"firmware"/name).read_bytes()) for name in FIRMWARE_FILES}
    subprocess.run([pio, "run", "--project-dir", str(ROOT/"firmware"), "--environment", ENVIRONMENT, "--target", "clean"], stdout=subprocess.DEVNULL, check=True)
    with log_path.open("w") as output:
        result = subprocess.run([pio, "run", "--project-dir", str(ROOT/"firmware"), "--environment", ENVIRONMENT, "--verbose"], stdout=output, stderr=subprocess.STDOUT)
    if result.returncode:
        # CI must expose the underlying compiler/tool failure, not only a
        # CalledProcessError for the command whose output was saved privately.
        tail = "\n".join(log_path.read_text(errors="replace").splitlines()[-100:])[-20000:]
        for secret in private_values():
            tail = tail.replace(secret.decode(), "<private-build-default>")
        print(f"Public firmware build failed with exit status {result.returncode}. Last build output:", flush=True)
        print(tail, flush=True)
        if "No module named 'intelhex'" in tail:
            print("Install intelhex==2.3.0 in the Python environment used by PlatformIO, then retry.", flush=True)
        print(f"Full build log: {log_path.relative_to(ROOT)}", flush=True)
        return result.returncode
    log = log_path.read_text()
    if build_inputs != {name: digest((ROOT/"firmware"/name).read_bytes()) for name in FIRMWARE_FILES}:
        raise ValueError("Firmware source changed during the public build; retry after edits finish")
    for name in ("board", "main", "network"):
        lines = [line for line in log.splitlines() if "xtensa-esp32s3-elf-g++ " in line and " -c " in line and f"src/{name}.cpp.o" in line]
        if len(lines) != 1 or "-DGLIMDOCK_PUBLIC_FIRMWARE=1" not in lines[0]:
            raise ValueError("Public macro not present in each actual application compiler command")
    build = ROOT / "firmware/.pio/build" / ENVIRONMENT
    app_command = next(line for line in log.splitlines() if "xtensa-esp32s3-elf-g++ " in line and " -c " in line and "src/main.cpp.o" in line)
    matches = re.findall(r'(-I[^ ]*packages/framework-arduinoespressif32)/cores/esp32', app_command)
    if not matches:
        raise ValueError("Actual framework path missing from compiler command")
    framework = Path(matches[0][2:]).resolve()
    packages = framework.parent
    secrets = private_values()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="glimdock-web-firmware-", dir=args.output.parent) as directory:
        stage = Path(directory)
        parts = []
        for name, source, offset in (("bootloader.bin",build/"bootloader.bin",0), ("partitions.bin",build/"partitions.bin",0x8000), ("boot_app0.bin",framework/"tools/partitions/boot_app0.bin",0xE000), ("firmware.bin",build/"firmware.bin",0x10000)):
            audit_bytes(source.read_bytes(), secrets)
            parts.append(image_part(source, offset, name))
            shutil.copyfile(source, stage/name)
        source_hash = hashlib.sha256()
        for name in FIRMWARE_FILES:
            source_hash.update(name.encode()+b"\0"+(ROOT/"firmware"/name).read_bytes())
        source = source_archive(stage, build, framework, packages, log, pio, secrets)
        if build_inputs != {name: digest((ROOT/"firmware"/name).read_bytes()) for name in FIRMWARE_FILES}:
            raise ValueError("Firmware source changed while packaging; retry after edits finish")
        if source["size"] > 32*1024*1024:
            raise ValueError("Corresponding-source archive exceeds the collector download limit")
        manifest = {"schema": 1, "chip": "ESP32-S3", "board": "waveshare-v1", "rotation": 3, "flash_size": FLASH_SIZE,
                    "version": "glimdock-"+source_hash.hexdigest()[:12], "source_sha256": source_hash.hexdigest(),
                    "source_relink": "/firmware/source-relink.tar.gz", "source_relink_size": source["size"],
                    "source_relink_sha256": source["sha256"], "parts": parts}
        (stage/"manifest.json").write_text(json.dumps(manifest, indent=2)+"\n")
        audit_bytes((stage/"manifest.json").read_bytes(), secrets)
        args.output.mkdir(parents=True, exist_ok=True)
        for name in ("bootloader.bin", "partitions.bin", "boot_app0.bin", "firmware.bin", "source-relink.tar.gz", "manifest.json"):
            os.replace(stage/name, args.output/name)
        report = {"schema": 1, "public_compile_macro_verified": True, "preserved_nvs": [NVS_START,NVS_END],
                  "rounded_ranges_safe": True, "private_default_values_checked": len(secrets), "private_default_matches": 0,
                  "source_archive": source, "manifest": manifest}
        (ROOT/"output/hardware/web-firmware-bundle.json").write_text(json.dumps(report,indent=2)+"\n")
        print(json.dumps({"version":manifest["version"], "application_sha256":parts[-1]["sha256"], "source_archive_bytes":source["size"], "source_files":source["files"], "private_default_matches":0},indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
