# Glimdock Flipper status display

A small Flipper Zero application receives four compact pages from Glimdock over
a dedicated paired Bluetooth service: **System, Alerts, Agents, Printer**.
It uses the Flipper's monochrome 128 × 64 display and physical buttons.

- **Down / Up:** next / previous monitored node, wrapping through the registry.
- **Right / Left:** next / previous page.
- **OK:** return to System.
- **Short Back:** disconnect Bluetooth and exit to the Flipper home screen.

The footer shows the source age and LIVE, SUMMARY, CACHED, LOADING, STALE,
NO UPDATES or DISCONNECTED. An explicit DEMO
label identifies sample data. Receiving a repeated sequence does not renew the
source age. A paused stream becomes NO UPDATES after ten seconds.
The app caches summaries for the full active registry (up to eight nodes) in RAM.
After the initial list arrives, Up/Down changes the displayed node locally,
including while Bluetooth is disconnected. Glimdock refreshes the selected
node's details in the background and updates the summary list periodically.
Previously received details remain available by node ID; pages without cached
details show a loading message. Cached readings retain their source age and
become stale normally. Closing the app clears its RAM cache.
Opening the app starts its own Bluetooth service and advertising. Keep it open
while viewing status; Back disconnects and restores Flipper’s normal Bluetooth
profile and key storage. USB is used only to install the app.

## Build

From the repository root:

```sh
python3 integrations/flipper/build.py --test
python3 integrations/flipper/build.py
```

The helper stages the app and canonical status/transport headers from
`firmware/src` under `output/flipper-status/app`. It creates
an isolated virtual environment and SDK under `output/flipper-status`, pins
uFBT 0.2.6 and official firmware SDK **1.5.1**, runs host checks, and writes
`output/flipper-status/dist/glimdock.fap` plus `build-manifest.json`.
It performs no upload or firmware flash.
Use `--sdk VERSION` to build against another official firmware release.
The reviewed Momentum **mntm-012** preset builds API 87.1 separately:

```sh
python3 integrations/flipper/build.py --momentum mntm-012
```

Its SDK archive comes from the [Momentum mntm-012 release](https://github.com/Next-Flip/Momentum-Firmware/releases/tag/mntm-012)
and is checked against the release's SHA-256 before use. The matching FAP,
manifest and SDK are under `output/flipper-status/momentum-mntm-012/`; the stock
1.5.1 artifact stays available. Both SDKs use toolchain version 39.

The host checks cover decoder rejection without replacing displayed data,
navigation wrapping, sequence replay freshness, source age, source flags and
clock wraparound. Hardware Bluetooth pairing, rendering and button behavior
still require a connected Flipper. Check its installed firmware before upload;
older or custom firmware may require a matching SDK build.

## Install and connect

After connecting the Flipper by USB, inspect it and then install the app:

```sh
python3 integrations/flipper/upload.py --inspect
python3 integrations/flipper/upload.py
```

The helper requires USB VID:PID `0483:5740`, a `flip_` serial identity and an
actual Flipper Zero target-7 response. It reads the installed firmware/API,
rebuilds against that official version or the reviewed Momentum `mntm-012`
SDK if needed, checks ABI compatibility,
returns to home, and writes only `/ext/apps/Tools/glimdock.fap`. It preserves an
existing copy locally, verifies the uploaded file by SHA-256 read-back, and
writes `output/flipper-status/upload-result.json`. Multiple connected Flippers
require an explicit `--port`. Other unsupported custom releases stop before upload.
An app upload does not replace Flipper firmware or launch the app.

## Glimdock firmware and USB diagnostics

The sender is part of the `homelab_s3_web` firmware. Build it and the Flipper app
from the same source revision; the cache protocol requires matching versions.
Existing public USB installer downloads remain pinned to their published release.
The guarded local updater
requires an explicit USB port and matching USB/eFuse MAC, verifies the V1 power
latch and existing partition table, backs up NVS with mode 0600, verifies every
written image and checks that NVS remains byte-for-byte unchanged before reboot:

```sh
python3 integrations/flipper/flash_glimdock.py --port /dev/cu.usbmodem... --mac EXPECTED_MAC
```

Use the SDK toolchain's Python or another environment with pyserial and the
installed PlatformIO esptool package. Images and hashes come from
`output/flipper-pio/dist/build-manifest.json`.

For a bounded live observation, `check_glimdock.py --port PORT --mac MAC
--duration 120 --output LOCAL_JSON` keeps one serial session open and records
connection state, heap and acknowledged status-frame counts. Local diagnostics
also identify the connection phase, encryption-event result and security flags;
an accepted session requires flags 7 and a 16-byte key. Opening a USB serial
session may reset this board; keep that session open throughout a pairing test.
The firmware's local USB dispatcher also accepts `GLIMDOCK_FLIPPER_STATUS`,
`GLIMDOCK_FLIPPER_ENABLE 0|1`, `GLIMDOCK_FLIPPER_PAIR`,
`GLIMDOCK_FLIPPER_PIN <six digits>` and `GLIMDOCK_FLIPPER_CANCEL`, each followed
by a newline. It never echoes PINs; replies exclude pairing codes, keys and private
conversation data. These commands are not exposed over the network.

## Pairing

Alternatively, copy the matching FAP to **SD Card/apps/Tools/glimdock.fap** using
qFlipper.

Open **Applications → Tools → Glimdock** on Flipper. The app starts its own
Bluetooth advertising, including when Flipper’s saved Bluetooth preference is
off. On Glimdock open **Settings → Flipper Bluetooth screen → Pair / reconnect
Flipper**, then enter the six-digit PIN displayed by Flipper. Disconnect the
phone app before starting. Normal phone pairing uses a separate key store.

The native app exposes a dedicated GATT write characteristic and receipt
characteristic. Each 516-byte detail envelope contains a stable node ID, the
selection request sequence and the original 444-byte status payload. It uses
bounded ordered chunks, authenticated write-with-response and a validated
complete-frame receipt. Compact catalog packets stage the full registry before
committing it atomically. A separate authenticated notification characteristic
carries the latest absolute node selection. Rapid button presses coalesce into
the latest target, and late details cannot change the Flipper's local focus.
Selection runs on Glimdock's display task through its normal node picker.
Left/Right, OK and Back work locally without waiting for network data.
Private conversation pages and credentials are not part of the payload.

`firmware/src/flipper_protocol.h` defines the status payload;
`firmware/src/flipper_cache_protocol.h` defines the catalog, detail envelope and
absolute selection messages; `firmware/src/flipper_ble_protocol.h` defines
chunk/receipt framing and UUIDs.
The custom Flipper profile has its own Bluetooth identity and bond-key file.
Back stops the service, disconnects, removes its callbacks and restores the
normal Bluetooth profile before the app exits.

References: [Momentum mntm-012 source](https://github.com/Next-Flip/Momentum-Firmware/tree/mntm-012),
[uFBT](https://github.com/flipperdevices/flipperzero-ufbt).

To capture the actual 128 × 64 status display over USB after pairing:

```sh
output/flipper-status/sdk/toolchain/current/bin/python3 tools/flipper-capture-screen.py \
  --port /dev/cu.usbmodemflip_... --output output/flipper-status/screen.png
```

This streams the real GUI and closes its own USB RPC session afterward.
