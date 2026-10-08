# Actual firmware UI preview

This renderer compiles `firmware/src/main.cpp` against LVGL 9.3.0 and the firmware's fonts, layout, RGB565 color format and 512 KiB memory pool. LVGL draws into a 320 × 240 offscreen framebuffer; the runner saves PNGs without connecting to an ESP32 or the homelab.

It requires Python 3, CMake and a C/C++ compiler. A completed PlatformIO firmware build supplies the pinned LVGL dependency automatically:

```sh
python3 tools/native-preview/render.py --snapshot agent/demo.json --output output/native-demo
python3 tools/native-preview/render.py --snapshot output/live/snapshot.json --output output/native-live
```

If LVGL is not installed under `firmware/.pio/libdeps/homelab_s3/lvgl`, supply its source directory with `--lvgl-source /path/to/lvgl-9.3.0`, or set `LVGL_SOURCE_DIR`.

The output covers Overview, guests, guest details, storage, disk details, sensors, CPU/power details, memory details, GPU inventory/device details, alerts, settings and scrolled views. Each main screen is also captured in Dark appearance. A full run also captures stale, offline and setup states. Use `--pages 0,3,6` to capture selected screens only:

| Page | Screen |
| --- | --- |
| 0 | Overview |
| 1 | Guests |
| 2 | Storage |
| 3 | Sensors |
| 4 | First guest details |
| 5 | CPU/power, logical CPU usage and C-states |
| 6 | Alerts, faults and source health |
| 7 | Settings |
| 9 | First disk details |
| 10 | Memory, load, host uptime and I/O |
| 11 | GPU inventory and ownership |
| 12 | GPU telemetry, clocks, identity and errors |

The output also includes an `index.html` gallery, the exact saved input snapshot and a memory report. Each capture reports LVGL memory use and the largest free block. A failed LVGL allocation aborts with an assertion instead of silently producing an incomplete screenshot. This makes large saved inventories useful for capacity checks.

The saved snapshot's collection timestamp and nested guest-memory/GPU timestamps are rebased by the same offset to the current time so its values can be inspected as a fresh screen while retaining their original sample ages. Explicit `mem_age_s`/`age_s` values also advance with the simulated clock. The demo fixture uses sample history; a real fixture begins with one sample. `provenance.json` records the input and rendering method. The native Arduino and board shims provide only clock, text, preferences and allocation behavior; HTTP, SPI, touchscreen rotation and physical RGB byte order still require device verification.

Run `python3 tools/native-preview/test_gestures.py` for continuous press/move/release checks through LVGL's real pointer state machine. These cover swipe rejection, toolbar/list taps, theme and settings switches, brightness dragging, and Back returning to the originating tab for CPU, Memory and Health detail. GPU tests cover CPU/Overview/assigned-guest routes, list/button drag rejection, deliberate taps, Back and stable selection after inventory reordering; memory checks cover unknown guest OS values without Proxmox fallback and the NAS estimate/ARC distinction. Sensor checks distinguish healthy empty Amps from a failed sensor source and verify measured CPU/GPU watts in the Power category. The renderer captures White/Dark Power and Amps filters as well as the other sensor categories. The fixture fingerprint is verified, and gesture builds use a separate cache from image rendering. Rendering guest details also captures the first three guest identities and their memory breakdowns; GPU detail captures cover every bounded device. Preferences use an in-memory adapter; no hardware, credentials or network requests are accessed. Optional idle dimming defaults off for desk use.

Multi-node pages retain the production node-picker/loading guard and render printer Overview/Job/Temps/Health when the fixture node type is Klipper. `--config PATH` accepts a saved public GET `/api/v1/config` projection containing only configuration fields and `has_secret` flags; secret-bearing inputs are rejected. Management page numbers14–18 capture native Wi-Fi, node list, edit, delete confirmation and keyboard. These pages use in-memory management adapters; Wi-Fi settings are explicitly simulated and no remote configuration or device credentials are accessed. The pointer suite also covers keyboard release typing/drag rejection, masked secret editors, form draft accept/cancel, node add/edit, deliberate delete confirmation, failed-save draft retention and a resumed feed arriving before the UI acknowledges a successful save.
