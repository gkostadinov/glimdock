#!/usr/bin/env python3
"""Compile the production Glimdock UI and its JSON parser into WebAssembly."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def function(source: str, signature: str) -> str:
    """Extract complete C++ functions, respecting quoted braces and comments."""
    start = source.index(signature)
    brace = source.index("{", start)
    depth = 0
    state = "code"
    i = brace
    while i < len(source):
        ch = source[i]
        nxt = source[i:i+2]
        if state in ("single", "double"):
            if ch == "\\": i += 2; continue
            if ch == ("'" if state == "single" else '"'): state = "code"
        elif state == "line":
            if ch == "\n": state = "code"
        elif state == "block":
            if nxt == "*/": state = "code"; i += 2; continue
        elif nxt == "//": state = "line"; i += 2; continue
        elif nxt == "/*": state = "block"; i += 2; continue
        elif ch in ("'", '"'): state = "single" if ch == "'" else "double"
        elif ch == "{": depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0: return source[start:i+1]
        i += 1
    raise ValueError(f"Unclosed C++ function: {signature}")


def parser_header(source: str) -> str:
    signatures = ("void copyText(", "template<size_t N>void text(", "double number(JsonVariantConst", "bool descriptor(", "bool parse(JsonDocument", "bool parseConfiguration(")
    return "// Generated from firmware/src/network.cpp; do not edit.\nnamespace firmware_json {\n" + "\n".join(function(source, s) for s in signatures) + "\n}\n" + function(source, "void makeDemo(Snapshot &s)") + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--emsdk", type=Path, default=ROOT/"output/toolchains/emsdk")
    parser.add_argument("--build-dir", type=Path, default=ROOT/"output/web-emulator-build")
    parser.add_argument("--output", type=Path, default=ROOT/"collector-web/emulator")
    parser.add_argument("--rotation", type=int, choices=range(4), default=3)
    args = parser.parse_args()
    emscripten = args.emsdk.resolve()/"upstream/emscripten"
    emcmake = emscripten/"emcmake"
    if not emcmake.is_file(): parser.error("Install the official Emscripten SDK and activate 6.0.12, or provide --emsdk")
    dependency_candidates = [ROOT/"firmware/.pio/libdeps/homelab_s3_landscape3", ROOT/"firmware/.pio/libdeps/homelab_s3"]
    deps = next((p for p in dependency_candidates if (p/"lvgl/CMakeLists.txt").exists() and (p/"ArduinoJson/src/ArduinoJson.h").exists()), None)
    if deps is None: parser.error("Run the firmware dependency install first (LVGL 9.3.0 and ArduinoJson 7)")
    args.build_dir.mkdir(parents=True, exist_ok=True)
    args.output.mkdir(parents=True, exist_ok=True)
    source = (ROOT/"firmware/src/network.cpp").read_text()
    (args.build_dir/"firmware_parser.h").write_text(parser_header(source))
    env = dict(os.environ)
    env["PATH"] = str(emscripten)+os.pathsep+env.get("PATH", "")
    configure = [str(emcmake),"cmake","-S",str(Path(__file__).parent),"-B",str(args.build_dir.resolve()),"-DCMAKE_BUILD_TYPE=Release",f"-DHOMELAB_ROTATION={args.rotation}",f"-DLVGL_SOURCE_DIR={(deps/'lvgl').resolve()}",f"-DARDUINOJSON_SOURCE_DIR={(deps/'ArduinoJson/src').resolve()}"]
    with (args.build_dir/"build.log").open("w") as log:
        for command in (configure,["cmake","--build",str(args.build_dir),"--target","firmware","-j","8"]):
            result = subprocess.run(command, env=env, stdout=log, stderr=subprocess.STDOUT)
            if result.returncode:
                print((args.build_dir/"build.log").read_text()[-16000:])
                return result.returncode
    assets = {}
    for name in ("firmware.js", "firmware.wasm"):
        shutil.copy2(args.build_dir/name,args.output/name)
        assets[name] = {"sha256":hashlib.sha256((args.output/name).read_bytes()).hexdigest(),"bytes":(args.output/name).stat().st_size}
    notices = [ROOT/"LICENSE", *(ROOT/"LICENSES"/name for name in ("LVGL-9.3.0-MIT.txt", "LVGL-9.3.0-SPRINTF.txt", "LVGL-9.3.0-TLSF.txt", "ArduinoJson-7.4.2-MIT.txt", "Montserrat-OFL-1.1.txt", "Space-Grotesk-OFL-1.1.txt", "Font-Awesome-5-NOTICES.txt")), emscripten/"LICENSE", emscripten/"system/lib/libc/musl/COPYRIGHT", *(emscripten/"system/lib"/name/"LICENSE.TXT" for name in ("libcxx", "libcxxabi", "libunwind", "compiler-rt"))]
    (args.output/"LICENSES.txt").write_text("Glimdock firmware browser emulator — license notices\n\n" + "\n\n".join(p.name+"\n"+"="*72+"\n"+p.read_text() for p in notices))
    tracked = [ROOT/"firmware/src/main.cpp",ROOT/"firmware/src/model.h",ROOT/"firmware/src/network.cpp",ROOT/"firmware/include/lv_conf.h",ROOT/"firmware/src/brand_assets.h",Path(__file__).parent/"emulator.cpp"]
    sources = {str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in tracked}
    sdk = subprocess.check_output([str(emscripten/"emcc"),"--version"],env=env,text=True).splitlines()[0]
    manifest = {"format":1,"implementation":"production firmware UI, LVGL software renderer, production snapshot/config parser","public_build":True,"width":240 if args.rotation%2==0 else 320,"height":320 if args.rotation%2==0 else 240,"rotation":args.rotation,"lvgl":"9.3.0","toolchain":sdk,"sources":sources,"assets":assets,"licenses":"LICENSES.txt"}
    (args.output/"manifest.json").write_text(json.dumps(manifest,indent=2)+"\n")
    print(json.dumps({"output":str(args.output),"viewport":f"{manifest['width']}x{manifest['height']}","assets":assets}))
    return 0


if __name__ == "__main__": raise SystemExit(main())
