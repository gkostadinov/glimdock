# Glimdock documentation

Start with [SETUP.md](../SETUP.md) if you want to run Glimdock. Use a Linux binary
for the collector and build the supported ESP32 firmware from source.

| Guide | Covers |
| --- | --- |
| [Setup](../SETUP.md) | Linux install, separate tokens, Wi-Fi, firmware flashing, node settings |
| [Supported hardware and telemetry](COMPATIBILITY.md) | Board revision, host requirements, source limitations |
| [Architecture](ARCHITECTURE.md) | Processes, privileges, tokens, network/data flow |
| [Troubleshooting](TROUBLESHOOTING.md) | Connection, unavailable readings, touch and power issues |
| [Build and test](../BUILDING.md) | Rust, musl binaries, PlatformIO, native LVGL previews, enclosure CAD |
| [Telemetry implementation](../collector-rs/docs/TELEMETRY.md) | Probe scheduling and measurement semantics |
| [Release verification](RELEASES.md) | Checksums, architecture validation and source provenance |
| [Contributing](../CONTRIBUTING.md) | Behavioral checks, synthetic fixtures, integration rules |
| [Security](../SECURITY.md) | Private reporting and deployment boundaries |
| [License scope](../LICENSE_POLICY.md) | MIT software, CERN-OHL-P-2.0 enclosure, third-party notices |
| [V1 driver provenance](DRIVER_PROVENANCE.md) | Authored driver and manufacturer-reference boundaries |

Examples use documentation-only addresses. Replace them with your own LAN
addresses, and keep passwords, keys and tokens out of issues and screenshots.
