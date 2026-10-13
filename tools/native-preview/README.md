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
python3 tools/native-preview/render.py --snapshot examples/snapshots/firmware-inventory.json --rotation 3 --pages 19,20 --clock 1791799680 --output output/native-clock
python3 tools/native-preview/test_gestures.py --rotation 3 --output output/gestures-landscape
python3 tools/native-preview/test_gestures.py --rotation 0 --output output/gestures-portrait
python3 tools/native-preview/test_rotations.py --output output/rotation-checks
python3 tools/native-preview/test_clock.py
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
| 20 | Clock with node summary |

Server/device fixtures retain platform identity and use Overview, Storage,
Sensors and Health. They do not fabricate Proxmox guests. Optional OS, battery
and interface fields render when supplied. Klipper fixtures use printer views.
For fleet captures, provide a schema-1 snapshot with a `nodes` descriptor array
and per-node summaries. The schema-2 `all-nodes.json` file is a demo transport
fixture, not a native renderer input; the gesture suite supplies four-node
captures of both All nodes and Clock.

Saved timestamps are rebased together so a fixture can be inspected as a fresh
screen while retaining its relative source ages. Explicit ages advance with the
simulated clock. A labeled live capture still represents its saved input; it is
not evidence of a live board connection. Allocation failure aborts instead of
silently generating incomplete output.

`--clock UNIX_SECONDS` fixes wall time for reproducible clock captures. The
fixture timestamps are rebased to that epoch and time advances with simulated
UI ticks. Omitting it uses the host's current time. Timezone choices change only
display formatting; sample freshness continues to use UTC timestamps.
Clock mode is page 20 and also opens by tapping the header time or fleet Clock
tab. Settings cycles its saved display zone through UTC (default), Sofia,
London, Berlin, New York, Los Angeles and Tokyo. The Glimdock wordmark dot shows
feed status by color, with the full state available in Settings. Settings occupies
the header's right edge; the node-count strip has been removed.

`--config PATH` accepts a public `GET /api/v1/config` projection with configuration
and `has_secret` flags. Inputs containing secrets are rejected. Node-management
and preference adapters operate in memory; Wi-Fi examples are explicitly
simulated. No real credentials or remote configuration are accessed.

## Behavioral verification

The gesture suite feeds actual LVGL continuous pointer input using rendered
control geometry. It checks scrolling without accidental row/button activation,
detail Back routes, node selection/loading guards, keyboards, masked editors,
add/edit/delete confirmation, failed-save drafts, appearance, brightness, notification volume and
optional idle dimming. Volume checks cover drag previews, saving on release,
vertical scroll rejection, silence at zero and mute-independent speaker tests.
Fleet checks measure balanced card padding, aligned rows, consistent type sizes
and readable full-load percentages. It also checks All nodes cards, platform preservation,
source expiry, unknown readings, guest OS memory and GPU ownership semantics.
Clock checks cover header/tab navigation, separate header hit areas, compact
node selection, averages that exclude stale/offline/unknown readings and
content staying above the footer in both orientations.

Rotation checks compile each shared transform, verify corner mappings and reject
invalid raw coordinates, then check every raw panel pixel maps uniquely to the
viewport. This proves software geometry rather than the assembled USB direction.

The shims provide clock, text, allocation and preferences. Physical SPI/RGB byte
order, touch-controller reports, power, Wi-Fi and USB still require the supported
V1 board. See [firmware setup](../../firmware/README.md) and
[BUILDING.md](../../public-docs/BUILDING.md) for the complete verification flow.

The audio gain checks run without a board:

```sh
c++ -std=c++17 -Wall -Wextra -Werror -DHOMELAB_NATIVE_PREVIEW -Ifirmware/src tools/native-preview/audio_test.cpp -o /tmp/glimdock-audio-test
/tmp/glimdock-audio-test
```

The voltage-only battery filter and coarse-band checks run without hardware:

```sh
c++ -std=c++17 -Wall -Wextra -Werror -Ifirmware/src tools/native-preview/battery_test.cpp -o /tmp/glimdock-battery-test
/tmp/glimdock-battery-test
```

The gesture suite injects a local device battery fixture, separately from the
collector's laptop battery. It checks unknown voltage, approximate labeling,
header geometry, retained Settings updates and low-voltage color. ADC accuracy
and actual cell connection still require the physical V1 board.

Low-battery pointer checks exercise delayed confirmation, a brief banner and
chime while offline, persistent-low suppression, sensor loss, sustained recovery
and saved mute. Grouped-device checks cover four fleet cards with a fifth child,
group health, root averages, child navigation, port details, independent
freshness and grouping preserved through a device Settings save.
