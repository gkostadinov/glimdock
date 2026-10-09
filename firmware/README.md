# Glimdock display firmware

The display pairs with the host-agnostic central collector and consumes its node
registry. It starts on **All nodes**, then opens a server/device, Proxmox or Klipper
dashboard when a card is selected. Node configuration belongs to the collector;
Wi-Fi, pairing, selected node and appearance belong to the display.

The supported hardware is **Waveshare ESP32-S3-Touch-LCD-2.8 V1**:
ST7789 display, CST328 capacitive touch, 16 MiB flash and 8 MiB OPI PSRAM.
V2/CST3530 needs a different touch driver and is not supported by the V1 image.
The current [Pebble Landscape](../enclosure/pebble-landscape/README.md) concept
uses USB-right landscape, rotation 3, at 320 × 240.

## Install through the collector

Open the collector's **Display & firmware** in desktop Chrome or Edge on HTTPS or
localhost. Connect the supported board by USB, select **Connect & update**, and
choose the board. The updater verifies the chip and all manifest/image hashes,
writes the required segments without erasing NVS, and restarts the display.
Saved pairing and preferences survive a normal update. A new unit opens setup.

The collector supplies the credential-free public firmware and its matching
source/relink materials. [SETUP.md](../SETUP.md) describes the Rust localhost
companion for a collector served over LAN HTTP. Browser USB programming is a
Web Serial workflow; OTA updating is not implemented.

## Build and flash from source

Pinned dependencies are PlatformIO Espressif32 6.13.0 / Arduino 2.0.17,
LVGL 9.3.0 and ArduinoJson 7.4.2. The `esp32s3box` profile provides the ESP32-S3
USB and QIO/OPI settings; this project's driver defines the display/touch pins.

```sh
./firmware/build_and_run.sh build homelab_s3_landscape3
./firmware/build_and_run.sh build homelab_s3_web
./firmware/build_and_run.sh devices
./firmware/build_and_run.sh upload /YOUR/USB/PORT homelab_s3_landscape3
./firmware/build_and_run.sh monitor /YOUR/USB/PORT
```

The build-only command never flashes a board. Upload requires its explicit
physical serial port. The script exports application, bootloader and partition
files under `firmware/build/ENVIRONMENT/`; PlatformIO supplies upload offsets.
If the board does not enter ROM upload, hold BOOT, tap RESET, release BOOT and
list devices again. The current case has no external BOOT/RESET access, so
physical-button recovery requires opening it. Release any serial monitor
before programming.

A redistributable image uses `GLIMDOCK_PUBLIC_FIRMWARE=1`, as in
`homelab_s3_web`. It excludes `include/local_credentials.h` and forces every
Wi-Fi, endpoint and token default blank. `python3 tools/build-web-firmware.py`
creates the audited four-segment public update and its source/relink archive.

| Rotation | Viewport | Environment |
| --- | --- | --- |
| 3 | 320 × 240 | `homelab_s3_landscape3`; current USB-right case |
| 1 | 320 × 240 | `homelab_s3_landscape1`; opposite landscape |
| 0 | 240 × 320 | `homelab_s3_portrait0` |
| 2 | 240 × 320 | `homelab_s3_portrait2`; opposite portrait |

`HOMELAB_ROTATION` and the shared `src/board.h` constants control display,
viewport, buffer size and touch transform together. Explicit environments take
precedence over the header default. Portrait reflows cards, tabs, forms and the
keyboard; it is available for other enclosures. Validate physical orientation,
all four corners and both swipe directions on an assembled board.

## Pair once with the collector

Choose **Configure on this display** or **Settings → Wi-Fi & collector setup**.
Enter the 2.4 GHz Wi-Fi name/password, central base snapshot URL and read-only
display token:

```text
http://COLLECTOR_ADDRESS:8765/api/v1/snapshot
```

Leave off node queries; the worker selects configured nodes itself. An optional
separate setup token enables **Nodes & monitoring**. Secrets remain masked.
Blank passwords are retained for the same Wi-Fi name; blank tokens are retained
for the same exact collector endpoint. **Clear saved Wi-Fi password** explicitly
selects an open network. **Save & connect** verifies saved NVS on the network
worker without blocking touch.

For the portal or an HTTPS CA, choose **Advanced browser setup / HTTPS CA**.
Join the password-protected **Homelab-Setup** network using the password shown
on screen, then open `http://192.168.4.1/`. Paste the correct CA PEM for HTTPS;
certificate verification and network-time validation remain enabled. Redirects
and insecure TLS fallback are disabled.

A Git-ignored `include/local_credentials.h` can provide private build defaults
for SSID, password, display/setup tokens and `HOMELAB_DEFAULT_ENDPOINT`.
These values are embedded in that private binary. Existing saved settings take
precedence as a complete set. With no saved pairing, a complete valid default set
is persisted only after its first accepted authenticated snapshot: other fields
are verified before the SSID commit marker, and existing pairing is preserved.
Incomplete defaults open setup. Public images reconnect using saved NVS rather
than compiled credentials. `tools/test-pairing-policy.py` verifies this policy.

A normal update keeps NVS; a full erase clears it. Settings are not encrypted at
rest by this development build. Serial diagnostics report status, sequences and
inventory counts without printing Wi-Fi credentials, tokens or snapshot bodies.

## All nodes and detail dashboards

**All nodes** shows up to four bento cards without scrolling in either orientation.
Cards contain name, platform/type, health and the available primary readings:
CPU/RAM for servers, guest count for Proxmox, job state/progress for Klipper,
and temperatures/sensor counts. Each node expires independently using its
summary age and TTL. A frozen central feed also makes the fleet stale.

Tap a card to open its dashboard. The Glimdock logo returns to All nodes; the
header picker includes **All nodes • summary**. Node selection immediately
clears the previous metrics and history, rejects late responses for an old
selection and keeps the stable selected ID. Removed selections fall back to the
configured default; no platform gets priority.

| Node capability | Views and details |
| --- | --- |
| Server / device | Overview, Storage, Sensors and Health; available CPU, RAM, filesystems, disks, GPU, OS, battery and interface details |
| Proxmox | Host views plus VMs/containers, separate guest OS/accounting memory, storage and assigned GPU details |
| Klipper | Overview, Job, Temps and Health; state, progress, filename, duration, available layers/heaters and labeled remaining-time estimate |

CPU/package-watts cards open CPU/power detail; RAM opens memory/load/I/O;
health opens source/alert details. Lists use tap for detail and drag for scrolling.
A drag cannot activate a row or toolbar control. Refresh and page rotation wait
until touch and momentum finish. Missing values stay `--`; known zero remains
zero. Initial/reset counter samples stay unknown, component watts are not wall
power, heater duty is not watts, and ZFS status is not SMART health.

**Settings → Nodes & monitoring** edits the central registry with its setup key.
The optional Hub host and remote Server/device, Proxmox and Klipper feeds share
the same management model. Names, endpoint, platform and polling settings are
editable; credentials are retained only for the same service origin. IDs stay
stable. Delete requires deliberate confirmation. Concurrent changes reload the
public configuration, and failed saves retain the draft. A zero-node collector
leaves Settings available.

Appearance, brightness, optional 15-second page rotation and **Dim after 2 min**
are local preferences. White is the fresh default and idle dimming is off by
default. With dimming enabled, the first touch wakes and consumes that gesture.
**Preview demo** is an explicit choice with a visible DEMO badge; failures never
silently switch to sample data.

## Board integration and bounds

SPI: MOSI 45, SCLK 40, CS 42, DC 41, RESET 39, backlight 5.
CST328: SDA 1, SCL 3, IRQ 4, RESET 2, address `0x1a`, 400 kHz.
V1 power latch GPIO 7 stays high. ST7789 RAMCTRL `0xb0 = 00 e8` selects
little-endian RGB565 with an unswapped pixel stream. See
[driver provenance](../docs/DRIVER_PROVENANCE.md) for protocol sources and credit.

LVGL runs in the UI task; Wi-Fi, portal and bounded HTTP/JSON parsing run on a
separate FreeRTOS worker. Three snapshots and parser/network data use PSRAM with
mutex publication. The fixed LVGL heap is 512 KiB in PSRAM; the internal
24-row draw buffer is 15,360 bytes in landscape or 11,520 in portrait.
Responses are capped at 48 KiB and JSON allocation at 192 KiB.

The bounded inventory supports 4 node descriptors, 48 guests, 64 sensors,
16 storage entries, 16 disks, 8 GPUs, 64 logical CPUs, 24 alerts, 16 sources,
16 recent faults, 8 printer heaters and 16 additional printer temperatures.
Omitted entries are reported. Monitoring edits have no VM lifecycle, printer
control or arbitrary command API.

## Shared browser and native verification

[`tools/web-emulator/`](../tools/web-emulator/README.md) builds the same UI,
fonts and parsers into WebAssembly for the collector console.
[`tools/native-preview/`](../tools/native-preview/README.md) renders the same
production code into PNGs and runs continuous pointer tests through LVGL.
The shims supply transport, clock, allocation and preferences; they do not
emulate the ESP32 CPU, Wi-Fi, touch controller, flash or physical USB.

Run the checks in [BUILDING.md](../public-docs/BUILDING.md). Native/WASM results
verify layout and software interactions; physical power, touch, radio, USB and
assembled enclosure fit require the supported board.
