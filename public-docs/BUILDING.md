# Build and verify

The repository is a Rust workspace with two executables: `glimdock-collector`
for the Linux/macOS central server and `glimdock-agent` for monitored
Linux/macOS/Windows devices. v0.3.0 uses explicit enrollment and agent push;
existing polling adapters remain compatible. The web console, actual firmware WebAssembly renderer
and public USB update bundle are embedded in the collector at compile time.

## Rust workspace

Use Rust 1.88 or newer. Run commands from the repository root; the root
`Cargo.lock` pins the shared dependency graph.

```sh
cargo build --workspace --locked --release
cargo test --workspace --locked
target/release/glimdock-collector --help
target/release/glimdock-agent --help
target/release/glimdock-collector validate-config --config examples/collector.json
```

Build a Windows device agent with `cargo build --locked --release -p glimdock-agent`
on Windows. It produces `target/release/glimdock-agent.exe`; the Unix central
configuration service is not a Windows hub implementation. Build binaries for the
OS and architecture on which they will run.

Keep `collector-rs/` and `device-agent-rs/` together: the collector reuses the
agent's portable native host library. Their directory names are retained for
source and deployment compatibility, while build outputs now share `target/`.

## Build a complete collector release

Install Node.js 20.19 or newer, Python 3.11+, CMake and a C/C++ compiler.
Use an isolated Python environment for the firmware build tools:

```sh
python3 -m venv output/toolchains/firmware-python
. output/toolchains/firmware-python/bin/activate
python -m pip install platformio==6.1.19 intelhex==2.3.0
```

IntelHex is required by the pinned ESP32 image conversion tool; a fresh
PlatformIO installation does not supply it. These tools build firmware assets;
the collector and device agents run as native Rust executables.

Install and activate Emscripten SDK **6.0.12**, normally under
`output/toolchains/emsdk`. The emulator builder also accepts `--emsdk PATH`.

Build in this order:

```sh
pio pkg install --project-dir firmware --environment homelab_s3
npm ci --prefix collector-web
npm run build --prefix collector-web
python3 tools/build-web-firmware.py
python3 tools/web-emulator/build.py
cargo build --workspace --locked --release
```

`tools/build-release.sh full` coordinates the public-asset and Rust build.
`tools/build-release.sh rust` rebuilds only Rust against the existing public
assets. Supply an optional Rust target as the second argument, for example
`tools/build-release.sh rust aarch64-apple-darwin`, after installing its toolchain.
A plain Cargo build embeds the web files currently present, so regenerate assets
when changing their source.

The emulator compiles the production `firmware/src/main.cpp`, fonts, brand assets,
snapshot/configuration parsers and LVGL into WebAssembly. Its default output
matches USB-right landscape: rotation 3, 320 × 240. `--rotation 0` through `3`
selects another orientation. The UI and touch behavior are shared with the
physical device; browser transport replaces the board peripherals.

The public firmware builder compiles `homelab_s3_web`, excludes private pairing
defaults, audits the images and produces a manifest, four required flash
segments, notices and a source/relink archive. It never opens a USB port.
A public image opens setup on a new board and retains saved NVS on an ordinary
update. The relink archive includes the matching sources, libraries, application
objects and helper needed to modify applicable library code and relink.

The Rust executable embeds only the allowlisted public `collector-web/index.html`,
`assets/`, `emulator/` and `firmware/` files. Build sources, dependencies,
private configuration, tests and keys are excluded. The running server needs no
Python/Node.js runtime and no separate web asset directory.

## Run without installing services

```sh
target/release/glimdock-collector run --state-dir ./glimdock-state --bind 127.0.0.1 --port 8765
```

Open `http://127.0.0.1:8765/`. Collection, managed configuration and the web console
run in one process. Local browser access is available directly; remote clients
use the generated role-specific keys. See [SETUP.md](../SETUP.md) for state,
credentials, LAN listening, display pairing and the optional systemd deployment.

For a device-agent sample:

```sh
target/release/glimdock-agent --config examples/agents/host.json --once
```

`--once` initializes counter baselines and prints one local sample without a
listener or token. Agents, SNMP and JSON adapters publish the same bounded snapshot contract
through authenticated push. `--serve-http` retains an optional read-only polling
endpoint. [DEVICES.md](DEVICES.md) covers normal service commands and explicit
`--re-enroll` recovery with a fresh key and retained private state.

## Linux release targets

For static musl executables, install Zig and cargo-zigbuild, then build the desired
architecture. Cross compilation proves build compatibility, not runtime testing
on that hardware.

```sh
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
cargo install cargo-zigbuild --locked
cargo zigbuild --locked --release --workspace --target x86_64-unknown-linux-musl
cargo zigbuild --locked --release --workspace --target aarch64-unknown-linux-musl
```

Put the Zig executable on PATH. The `ring` dependency compiles C/assembly; Zig
provides the musl linker and sysroot. Rustls avoids a dynamic OpenSSL requirement.
Target executables appear under `target/TARGET/release/`.

Ship checksums, matching configuration examples, source and dependency notices
with distributed executables. `tools/cargo-notices.py` regenerates locked Rust
license inventories; use `--manifest` and `--targets` when selecting a graph.
Keep the actual tested OS/architecture separate from build-only targets.
Frozen `release/` artifacts retain their original hashes and documentation;
create a new version for this architecture rather than editing an old release.

## Behavioral and visual checks

```sh
cargo test --workspace --locked
npm test --prefix collector-web
node tools/web-emulator/test.mjs
node tools/web-emulator/test-transport.mjs
python3 tools/test-pairing-policy.py
python3 tools/native-preview/test_gestures.py --rotation 3
python3 tools/native-preview/test_gestures.py --rotation 0
python3 tools/native-preview/test_rotations.py
python3 tools/native-preview/render.py --snapshot examples/snapshots/host.json --rotation 3 --output output/native-host
python3 tools/native-preview/render.py --snapshot examples/snapshots/all-nodes.json --rotation 3 --pages 19 --output output/native-fleet
```

Use PlatformIO-installed LVGL 9.3.0 sources or supply the native renderer's
`--lvgl-source` path. It saves White/Dark PNGs, memory reports, input/provenance
and a gallery. The continuous pointer suite checks real LVGL press/move/release
behavior, including scrolling, keyboards, node management and All nodes cards.
The browser tests exercise the compiled firmware and transport; web tests cover
data handling, authentication/enrollment/node-management transitions and the
verified USB flash plan.

Native/WASM verification establishes software behavior and pixel layout.
Physical touch, power, Wi-Fi, USB and enclosure fit require the supported board.
Current validation records distinguish a guarded command-line flash of the
public update from an actual Chrome hardware flash transaction.
Archived Python reference tests under `legacy/python/` are historical
compatibility checks, not the primary runtime validation.

## Enclosure development

The current design is [Pebble Landscape](../enclosure/pebble-landscape/README.md).
Its guide describes regeneration, source-bound STEP/STL outputs, removable
accessories, service order and fit checks. Manufacturer reference geometry and
personal slicer projects are separate from authored enclosure licensing.
A body print or a render does not establish assembled USB, glass or mounting fit.
Do not replace frozen case exports when producing a new revision.
