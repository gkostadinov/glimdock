# Glimdock

Glimdock brings servers, routers, appliances and printers to one small touchscreen. A **host-agnostic Rust collector** owns the node registry, accepts device readings and serves the management web interface. The display pairs with that collector once; registered nodes then appear automatically in its **All nodes** bento overview.

Linux, macOS and Windows machines use native Rust agents that push telemetry to the collector. Agents pair once, receive a stable node identity and need no incoming network connection. Routers and other appliances can expose read-only SNMP or a JSON API through the same Rust device agent. Proxmox adds guest, storage and GPU features to a node; Klipper adds print job and temperature views. Neither integration is required to run the collector.

```text
Linux / macOS / Windows agents ──push──┐
SNMP / JSON adapters ────────────push──┤   Moonraker / legacy feeds
                                     │          ↑ polling
                   Rust collector: node registry + web console
                           /                         \
              Glimdock ESP32-S3                Desktop browser
              All nodes + details       Overview · Nodes · Display & firmware
```

## Start with the collector

Run the matching Linux or macOS executable with a private state directory:

```sh
glimdock-collector run --state-dir ./glimdock-state --bind 127.0.0.1 --port 8765
```

Open `http://127.0.0.1:8765/`. The same server collects telemetry, applies node edits and serves the interface; no separate web server is needed. Local access works directly. Fresh configurations monitor the collector's native host as an ordinary **Server / device** node. You can pause or remove that node and use the collector only to receive agent pushes and aggregate optional polling feeds.

Use [SETUP.md](SETUP.md) for credentials, LAN access, display pairing and persistent services. [DEVICES.md](public-docs/DEVICES.md) explains how to pair an agent or adapter with a one-time key. Enrollment assigns a stable node ID automatically; changing a name preserves it. Explicit `--re-enroll` renews a revoked publisher with a fresh key while retaining device and node identity. Polling remains available for printers and existing APIs. Devices enroll explicitly; network scanning and credential discovery are not required.

## One interface for the fleet and display

The collector's built-in web console has three views:

| View | What it does |
| --- | --- |
| **Overview** | Live summary cards for every active node, including available CPU, memory, temperature and printer readings |
| **Nodes** | Pair agents; rename, pause, revoke and remove nodes; add polling feeds and manage the optional collector host |
| **Display & firmware** | Run the actual LVGL firmware UI in the browser and update the supported display over USB |

The browser firmware is compiled from the production UI, fonts, parsers and touch behavior into WebAssembly. It shares the display's layout and interactions; browser transport replaces the board peripherals. The larger web overview is a separate interface for managing the fleet.

The physical display starts on **All nodes**, with up to four summary cards. Tap a card to open its dashboard. Ordinary devices have Overview, Storage, Sensors and Health; Proxmox adds guest views; Klipper has Overview, Job, Temps and Health. Unavailable or expired measurements remain unknown. Up to sixteen remote nodes can be configured, with four active nodes including the optional collector host.

## Browser firmware updates

Connect a **Waveshare ESP32-S3-Touch-LCD-2.8 V1** Glimdock by USB, open **Display & firmware** in desktop Chrome or Edge, then choose **Connect & update**. Web Serial needs HTTPS or localhost. For a collector served over LAN HTTP, the Rust loopback companion serves the same console on the USB computer; [SETUP.md](SETUP.md) has the command.

The public update verifies the board and image hashes, preserves saved NVS pairing and preferences, and restarts the display. A new board opens setup. The update download includes version-matched source, dependency notices and practical rebuild/relink materials. V2/CST3530 is not supported by the V1 image.

## Hardware and enclosure

The supported board has an ST7789 display, CST328 touch, 16 MiB flash and 8 MiB PSRAM. The current **[Pebble Landscape r19](enclosure/pebble-landscape/README.md)** concept uses a purple pillowed body, white USB base and an 18° backward tilt, with USB to the right. Its full factory glass stays exposed, with the wider black chin to the right and a constant 2 mm front border. Firmware orientation is rotation 3 at 320 × 240. Optional ears, antennas and eyes are removable accessories. The USB-powered concept is the current direction; a battery variant remains future work.

The case is a prototype. The body print has started, while assembly, clearance and the proposed internal USB-C extension still need physical fit verification. Older enclosure variants and frozen releases remain available as historical designs. See [firmware/README.md](firmware/README.md) for board setup and other supported orientations.

## Build and contribute

The Rust workspace contains `collector-rs/` and `device-agent-rs/`. Build both executables with:

```sh
cargo build --workspace --locked --release
cargo test --workspace --locked
```

The executables are `target/release/glimdock-collector` and `target/release/glimdock-agent` (`.exe` for a Windows agent). The collector embeds `collector-web/`, the firmware WebAssembly renderer and the public USB update bundle. Build browser assets before compiling a distributable collector; [BUILDING.md](public-docs/BUILDING.md) gives the complete order and verification commands.

| Path | Purpose |
| --- | --- |
| `collector-rs/` | Rust central collector, node configuration and authenticated HTTP APIs |
| `device-agent-rs/` | Native Rust push agents, host telemetry, SNMP and mapped JSON adapters |
| `collector-web/` | Built-in management console, browser firmware and USB updater |
| `firmware/` | Production LVGL touchscreen firmware |
| `examples/` | Generic collector, node, agent and synthetic snapshot examples |
| `deploy/` | Linux service installation and compatibility wrappers |
| `integrations/truenas/` | Optional restricted NAS compatibility helper |
| `tools/native-preview/`, `tools/web-emulator/` | Shared firmware rendering and behavioral verification |
| `enclosure/pebble-landscape/` | Current case concept and assembly documents |
| `commercial/site/` | Product website |
| `legacy/python/` | Archived Python collector and former design preview |

Deployed collectors, the web server and device agents use Rust. Python, Node.js, CMake and PlatformIO support builds, rendering and validation; the optional NAS/QGA compatibility probes run on their monitored systems. See [native telemetry](collector-rs/docs/TELEMETRY.md) for measurement semantics and optional tool requirements.

Original software, documentation and graphics are MIT licensed; original enclosure designs use CERN-OHL-P-2.0. Dependencies retain their own licenses. See [LICENSE_POLICY.md](LICENSE_POLICY.md), [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), [contributing](public-docs/CONTRIBUTING.md) and [security](public-docs/SECURITY.md). Historical validation and prototype observations remain in [docs/validation.md](docs/validation.md).
