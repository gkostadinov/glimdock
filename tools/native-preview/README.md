# Actual firmware rendering and pointer checks

This tool compiles `firmware/src/main.cpp` against the same LVGL 9.3.0, fonts,
layout, RGB565 color format and fixed 512 KiB heap as the physical display.
It renders offscreen PNGs and runs continuous press/move/release interactions
without connecting to a board or changing a collector.

Use Python 3, CMake and a C/C++ compiler. Install the pinned LVGL dependency with
PlatformIO, or supply `--lvgl-source PATH` / `LVGL_SOURCE_DIR`:

```sh
pio pkg install --project-dir firmware --environment homelab_s3
python3 tools/native-preview/render.py --snapshot examples/snapshots/host.json --rotation 3 --output output/native-host
python3 tools/native-preview/render.py --snapshot examples/snapshots/all-nodes.json --rotation 3 --pages 19 --output output/native-fleet
python3 tools/native-preview/test_gestures.py --rotation 3 --output output/gestures-landscape
python3 tools/native-preview/test_gestures.py --rotation 0 --output output/gestures-portrait
python3 tools/native-preview/test_rotations.py --output output/rotation-checks
```

Rotation 0/2 renders 240 × 320; rotation 1/3 renders 320 × 240. The default follows
the firmware header, while an explicit rotation controls the CMake build,
viewport, captures and gallery together. Cache directories are separated by
rotation. A portrait screen is never stretched into a landscape capture.

## Captures

A render saves White/Dark screen PNGs, a gallery, the input snapshot, memory
reports and `provenance.json`. Full runs include scrolled forms/details and
simulated live, stale, offline and setup states. Use `--pages` for selected screens:

| Page | Screen |
| --- | --- |
| 0 | Selected-node Overview |
| 1 | Proxmox guests or Klipper job |
| 2 | Storage or Klipper temperatures |
| 3 | Sensors or Klipper health |
| 4 | First guest detail |
| 5 | CPU/power and logical cores |
| 6 | Alerts and source health |
| 7 | Settings |
| 9 | First disk detail |
| 10 | Memory, load and I/O |
| 11–12 | GPU inventory and detail |
| 13 | Node picker |
| 14–18 | Wi-Fi, node list/edit/delete and keyboard |
| 19 | All nodes bento overview |

Server/device fixtures retain platform identity and use Overview, Storage,
Sensors and Health. They do not fabricate Proxmox guests. Optional OS, battery
and interface fields render when supplied. Klipper fixtures use printer views.
The fleet fixture includes bounded summaries for the All nodes cards.

Saved timestamps are rebased together so a fixture can be inspected as a fresh
screen while retaining its relative source ages. Explicit ages advance with the
simulated clock. A labeled live capture still represents its saved input; it is
not evidence of a live board connection. Allocation failure aborts instead of
silently generating incomplete output.

`--config PATH` accepts a public `GET /api/v1/config` projection with configuration
and `has_secret` flags. Inputs containing secrets are rejected. Node-management
and preference adapters operate in memory; Wi-Fi examples are explicitly
simulated. No real credentials or remote configuration are accessed.

## Behavioral verification

The gesture suite feeds actual LVGL continuous pointer input using rendered
control geometry. It checks scrolling without accidental row/button activation,
detail Back routes, node selection/loading guards, keyboards, masked editors,
add/edit/delete confirmation, failed-save drafts, appearance, brightness and
optional idle dimming. It also checks All nodes cards, platform preservation,
source expiry, unknown readings, guest OS memory and GPU ownership semantics.

Rotation checks compile each shared transform, verify corner mappings and reject
invalid raw coordinates, then check every raw panel pixel maps uniquely to the
viewport. This proves software geometry rather than the assembled USB direction.

The shims provide clock, text, allocation and preferences. Physical SPI/RGB byte
order, touch-controller reports, power, Wi-Fi and USB still require the supported
V1 board. See [firmware setup](../../firmware/README.md) and
[BUILDING.md](../../public-docs/BUILDING.md) for the complete verification flow.
