# Glimdock display firmware

The display pairs with the host-agnostic central collector and consumes its node
registry. Native OS and router/API adapter agents send readings to that
collector; optional local probes and polling feeds share the same registry. The
display keeps one central read connection and never needs a per-device agent
endpoint or publisher key. It starts on **All systems**, then opens a server/device, Proxmox or Klipper
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

## Flipper Zero Bluetooth screen

**Settings → Flipper Bluetooth screen** connects a nearby Flipper Zero directly
over Bluetooth LE. Install the [Glimdock Flipper app](../integrations/flipper/README.md)
at `/ext/apps/Tools/glimdock.fap`, then open it on Flipper to start the
status Bluetooth service. Disconnect its phone companion before connecting from Glimdock.

Bluetooth status sharing is off by default. Choose **Pair / reconnect Flipper**,
enter the six-digit PIN shown by Flipper into Glimdock. Firmware that uses
numeric comparison instead displays a code to confirm on both devices. Glimdock remembers
the bonded peer for later reconnections. **Forget Flipper** removes that pairing;
turning the feature off stops the connection. Keep the app open while using the status display. Exiting it restores
Flipper’s normal Bluetooth profile.

Flipper **Up/Down** changes the displayed node locally and immediately, wrapping
through the active registry. The app keeps summaries for up to eight nodes and
previously received complete detail pages in RAM, so browsing also works while
Bluetooth is disconnected. The latest selection is sent to Glimdock as the
requested node; details load in the background. **Left/Right** moves through
**System, Alerts, Agents and Printer**, **OK** returns to System and **Back**
exits. Closing the app clears its cache.

System and Alerts follow the selected node. Printer shows its registry summary
or job details when that printer is selected. Agent counts use fresh public
operations sources and indicate partial inventories. Pages without cached
details show the selected node's name and a loading message. Previously received
details remain available while new details load. Selected-node details refresh
about once per second, with navigation updates sent promptly; the registry's
summary list refreshes periodically.

Source ages continue advancing while offline. LIVE, SUMMARY, CACHED, LOADING,
STALE, NO UPDATES, DISCONNECTED and DEMO labels distinguish current readings,
cached data and unavailable sources. Repeated packets do not renew source age.
The bounded, versioned Bluetooth protocol stages each registry update atomically
and acknowledges complete packets; late detail replies cannot change local
focus. Private conversations, credentials and network addresses are excluded.

Pairing, six-node local browsing, offline page navigation, background details,
Wi-Fi/Bluetooth coexistence and saved-pair reconnect without another PIN were
physically validated on the V1 board with Flipper Momentum mntm-012. Local node
changes measured 19.71–24.59 ms across 14 samples; all 16 offline navigation
checks passed. See the [dated validation record](../public-docs/VALIDATION.md#flipper-zero-cache--17-october-2026).
The official 1.5.1 Flipper app also builds successfully; its hardware behavior has
not been validated by these tests.

## Device battery indication

The small indicator below the header clock shows the Glimdock device's voltage.
**Settings → Device battery** shows the voltage and a broad Very low / Low /
Medium / High band. Both label the voltage as approximate. This is separate from
any monitored laptop battery reported by a collector.

The supported V1 board uses GPIO8 (ADC1) with a 200k/100k divider, so the calibrated
ADC millivolts are multiplied by three. Nine readings are median-filtered every
two seconds, then smoothed; band hysteresis reduces flicker around thresholds.
No battery-specific calibration offset is assumed. Implausible ADC readings
show Unavailable, not 0% or 100%.

This is a voltage-only indication for a single 3.7 V LiPo, not a fuel gauge.
Load, charging, temperature and cell age affect it. The firmware does not infer
charging, full charge, USB power or remaining runtime from the voltage, and it
does not shut the device down based on this estimate. The V1 charger status LED
is not wired to an ESP32 input in the [manufacturer's schematic](https://files.waveshare.com/wiki/ESP32-S3-Touch-LCD-2.8/ESP32-S3-Touch-LCD-2.8.pdf).

A low-voltage episode produces one brief **Battery low** banner and alert chime
once Low or Very low persists for 30 seconds. It stays quiet while voltage is
low, including a change to Very low. It re-arms after the smoothed voltage stays
at or above 3.80 V for one minute. Missing readings and brief voltage rebounds
do not re-arm it. This latch lasts for the current boot; a new low-battery boot
may warn once after the same confirmation period. Existing banners and active
touches/text edits defer the notice, and the chime honors saved mute/volume.
The banner works even when the collector is offline.

## Speaker notifications

On the V1 board, the attached speaker receives short, quiet PCM5101 chimes through
I2S (data GPIO47, bit clock GPIO48, word clock GPIO38). Playback uses a separate
task and bounded queue; it requires no SD card or audio decoder and does not block
touch or networking. **Settings → Notification sounds** mutes/unmutes notices and
persists the choice. **Sound volume** adjusts chimes from 0–100%, shows the
percentage while dragging and saves on release. Vertical Settings swipes do not
change the saved level. The default 30% preserves the original quiet chime; 0%
is silent, including speaker tests. **Test speaker** previews the current volume
even when automatic notices are muted.
The board's physical volume control also affects the speaker. Each notice also
shows a small on-screen banner for about 4.5 seconds, with the connection state,
new alert message or completed printer name. Tap the banner to dismiss it sooner.
It remains visible across dashboard refreshes and does not pause monitoring;
visual notices still appear when notification sounds are muted. **Test speaker**
previews both the sound and the brief banner.

A chime confirms the first live collector connection. New warning/critical alerts
on the monitored node and observed Klipper print completion produce distinct
chimes; fresh fleet summaries also track printer completion while another node is
open. Existing alerts/completed jobs on initial connection, demo/stale readings,
reordered or repeated snapshots do not trigger alarm replay. New-event sounds are
limited to one per ten seconds. This does not enable music playback, gyro/RTC
drivers, charging changes or power-button shutdown.

## All systems and detail dashboards

**All systems** shows up to four bento cards without scrolling in either orientation.
Cards contain name, platform/type, health and the available primary readings:
CPU/RAM for servers, guest count for Proxmox, job state/progress for Klipper,
and temperatures/sensor counts. Each node expires independently using its
summary age and TTL. A frozen central feed also makes the fleet stale.

Tap a card to open its dashboard. The Glimdock logo returns to All systems; the
header picker includes **All systems • summary**. Node selection immediately
clears the previous metrics and history, rejects late responses for an old
selection and keeps the stable selected ID. Removed selections fall back to the
configured default; no platform gets priority.

Tap the header time, the fleet **Clock** tab or **Settings → Open clock mode**
for a large clock with date, healthy-node count and compact tappable node
summaries. Average CPU/RAM includes only known readings from fresh computing
nodes; printers show job progress. Clock mode stays selected when automatic
page rotation is enabled. The header places Settings at the right edge. The
Glimdock wordmark's dot indicates feed status: brand purple for live, amber for stale
or loading, red for offline and purple for demo. Tapping the dot opens connection
settings from fleet views or health details from a node view. Settings includes
the full feed status; the navigation has no extra dot or node-count strip.

Dark appearance uses lighter slate backgrounds, brighter labels and clearer
card borders for the small LCD. Selected controls and the purple status dot
also brighten in dark mode; the saved backlight setting is retained.

**Settings → Appearance → Auto** follows the selected clock time zone: White
from 07:00 until 19:00 and Dark overnight. The choice is saved across updates
and restarts. Until time is synchronized, it keeps the current appearance.
Theme changes wait for touch/scrolling and text editing to finish; White and
Dark remain manual overrides.

**Settings → Clock time zone** cycles through UTC (default), Sofia, London,
Berlin, New York, Los Angeles and Tokyo and saves the selection on the display.
European and US zones follow daylight-saving changes. Only clock formatting
changes; collector timestamps and freshness remain UTC. Hardware starts NTP
after Wi-Fi connects and shows `--:--` until time is available. A synchronized
clock keeps running during a connection loss; after a restart it needs a new
time sync.

| Node capability | Views and details |
| --- | --- |
| Server / device | Overview, Storage, Sensors and Health; available CPU, RAM, filesystems, disks, GPU, OS, battery and interface details |
| Proxmox | Host views plus VMs/containers, separate guest OS/accounting memory, storage and assigned GPU details |
| Klipper | Overview, Job, Temps and Health; state, progress, filename, duration, available layers/heaters and labeled remaining-time estimate |

CPU/package-watts cards open CPU/power detail; RAM opens memory/load/I/O;
health opens source/alert details. Overview uses matching secondary cards for
CPU temperature and power. A third card rotates through network traffic,
guests or uptime, disk temperature, fan speed and available graphics load every
six seconds. Secondary cards use consistent 10-pixel captions and 16-pixel
values; their text is centered using the actual font heights, keeping top and
bottom padding balanced as readings change. Its label, value and frame stay aligned with the other cards, and
tapping it opens the relevant detail page. Lists use tap for detail and drag for scrolling.
A drag cannot activate a row or toolbar control. Refresh and page rotation wait
until touch and momentum finish. Missing values stay `--`; known zero remains
zero. Initial/reset counter samples stay unknown, component watts are not wall
power, heater duty is not watts, and ZFS status is not SMART health.

**Settings → Nodes & monitoring** edits the central registry with its setup key.
The optional Hub host, paired push agents and remote Server/device, Proxmox and
Klipper feeds share the registry. Paired nodes can be renamed, paused, resumed or
removed without a per-device endpoint. Agent enrollment/re-pairing and credential
revocation use the web console; removing a paired node revokes its publication
access. Polling feed endpoints, platform and polling settings remain editable;
credentials are retained only for the same service origin. IDs stay stable. Delete requires deliberate confirmation. Concurrent changes reload the
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

Settings and its setup/editing screens preserve the active All systems or Clock context, including fleet navigation and collector status. A node dashboard is selected only by an explicit node choice. The landscape header balances the visible gaps around the selector, clock, and right-edge Settings icon.

The All systems cards use four aligned text rows with consistent font sizes per
orientation and a centered text block, leaving balanced top and bottom padding
for both computer and printer summaries.

## Router subnodes

The collector supports four enabled root nodes and eight enabled devices total.
An optional `parent_id` groups a device under an existing enabled root. The All
nodes and Clock screens show roots; a stale/offline subnode marks its group as
Degraded. Fleet CPU/RAM averages include computing roots only.

Open the router and tap its conditional **Subnodes** tab (or subnode health
link), then tap the MikroTik row for CPU, RAM, storage, sensors and Ethernet-port details. Rows match the Proxmox Guests layout, with status, CPU, RAM, temperature and
RAM usage bar. Parents with children use Overview / Subnodes / Storage / Sensors,
matching Proxmox navigation; their Health page remains accessible via the
wordmark status dot. Devices without children retain Overview / Storage /
Sensors / Health. Navigation always has four tabs. Its Overview and Health views provide **Back to Router**. Each child uses its own
source freshness even if its parent is offline. Node settings preserve the
collector-owned parent identifier. The node picker also shows parent context.

SNMP devices can use a read-only adapter on the collector host; the monitored
device does not need to run a Glimdock binary. Pair and set `parent_id` in the
collector configuration/API. Parents cannot be disabled or deleted while an
enabled child depends on them.

## Private conversations

Agents → Tasks → a task → Open conversation opens that task’s linked chats.
Settings → Private conversations opens the complete optional authenticated archive.
Pair over USB with the [private-reader guide](../docs/private-conversations.md).
The runtime reader is separate from node/agent telemetry and clears text on exit;
public builds contain no pairing credential or archive content. The browser
renderer accepts only explicitly synthetic private-chat pages.

## Install from the public website

Open [glimdock.com/flash/](https://glimdock.com/flash/) in desktop Chrome or Edge to install the verified public Waveshare V1 images directly over USB. No local collector is needed for installation. Confirm the board, select its serial port, and keep the cable connected until verification and restart finish. The updater preserves saved Wi-Fi, pairing and display preferences. New boards require connection setup afterwards; monitoring uses a running collector.


### Diagnose resets and dropped connections

The firmware bounds both response size and the total HTTP body read time. A
stalled or partial reply closes the socket and returns to polling. Wi-Fi gets
an initial reassociation grace period and retries with backoff; recovery retains
saved pairing and does not reboot the display. Wi-Fi modem sleep remains enabled
for ESP32-S3 Bluetooth coexistence.

A USB serial monitor at 115200 baud accepts `GLIMDOCK_DIAGNOSTICS` followed by a
newline. The response contains uptime, internal heap/free-block sizes, PSRAM,
UI stack headroom, Wi-Fi status and the last eight boot reasons. It contains no
network credentials, monitoring keys, conversation text or pairing PINs. Some
USB serial adapters reset the ESP32 when opened; the recorded ROM reason helps
distinguish those resets. Boot history is saved once per boot; running uptime
checkpoints use RTC RAM, with no periodic flash writes.

For a deliberate recovery check over physical USB, send
`GLIMDOCK_DIAGNOSTICS_WIFI_DROP`. This temporarily disconnects the station without
erasing its settings. Keep the monitor open and confirm that live data resumes
without a new boot. The portable regression suite is
`python3 tools/test-network-recovery.py`.
