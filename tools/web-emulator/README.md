# Actual firmware UI in the browser

The collector's **Display & firmware** view runs the production
`firmware/src/main.cpp`, fonts, brand assets, snapshot/configuration parsers and
LVGL 9.3.0 through WebAssembly. Layout, RGB565 rasterization, UI ticks, keyboard
and pointer handling come from the same code as the physical display.
The larger Overview and Nodes web pages manage the collector directly.

Install and activate Emscripten SDK **6.0.12** in `output/toolchains/emsdk`, or
pass `--emsdk PATH`. Install the firmware's pinned PlatformIO dependencies first:

```sh
pio pkg install --project-dir firmware --environment homelab_s3
python3 tools/web-emulator/build.py
node tools/web-emulator/test.mjs
node tools/web-emulator/test-transport.mjs
```

The default viewport is rotation 3 at 320 × 240, matching the current USB-right
Pebble Landscape concept. `--rotation 0` through `3` chooses another supported
orientation. Public builds exclude private defaults. The generated manifest
records source/artifact hashes, viewport, toolchain and LVGL version; dependency
notices accompany the WASM bundle.

Output is written to `collector-web/emulator/`. Rebuild the Rust collector after
regenerating it so the embedded interface contains the new renderer.
[BUILDING.md](../../public-docs/BUILDING.md) covers the full asset build order.

## Collector integration

The console imports `mountFirmwareEmulator` from `emulator.js` and supplies
same-origin snapshot/configuration callbacks. The firmware opens **All nodes**
and receives bounded summaries from the central registry. Mouse/touch input
supports taps, swipes, forms and the firmware keyboard. `selectNode(id)` opens a
node from the larger web overview; `destroy()` stops polling and removes handlers.

Direct remote access keeps entered keys in browser session memory. The local
combined collector and loopback companion provide their scoped local access
without exposing private keys in downloaded assets. Node edits use the actual
collector management API and its separate role. No device Wi-Fi settings are
changed by this browser renderer.

Board transport is adapted to HTTP and browser input. This target does not
emulate the ESP32 CPU, radio, flash, USB or power latch. The Web Serial updater
is a separate module that programs the physical board only after the user
selects it. Pixel/behavior tests and a hardware flash transaction are recorded
separately in validation.
