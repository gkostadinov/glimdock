# Supported hardware and telemetry

## Display hardware

v0.1.0 supports **Waveshare ESP32-S3-Touch-LCD-2.8 V1**, with ST7789 display,
CST328 touch controller, 16 MiB flash and 8 MiB PSRAM. It uses landscape rotation
and the V1 GPIO7 power latch. Confirm the revision before flashing.

The visually similar **V2 / CST3530 board is not supported**. V1 is discontinued;
V2 support needs a driver port and physical touch/power/enclosure validation.
The r7 enclosure is an authored prototype with physical dry fit still pending.
The public sources do not include manufacturer CAD, slicer profiles or G-code.

Use a stable USB-C supply appropriate for the board. Glimdock connects to 2.4 GHz
Wi-Fi. First-boot setup uses a password-protected access point whose randomly
generated password is shown on the display. Normal operation uses your LAN.

## Collector hosts

| Platform | Release status |
| --- | --- |
| Linux x86_64, systemd | Static musl binary; live-tested on Proxmox |
| Linux aarch64, systemd | Static musl binary; compiled/ELF verified, not executed on ARM hardware |
| Other Linux hosts | Basic procfs/sysfs metrics; available host tools determine extra telemetry |
| Windows/macOS collector host | No supported collector package |
| Windows/Linux VM on Proxmox | Guest OS memory requires installed/enabled QEMU Guest Agent and configured VM ID |
| Klipper printer | Moonraker read access, normally on port 7125, via a Linux collector hub |
| TrueNAS | Optional restricted SSH helper; NAS permissions determine disk/pool visibility |

The display supports **four total nodes**, including the local Proxmox/Linux host
when enabled. For a printer-only hub, disable the local host node.

## Measurement limits

- VM allocated/resident memory is different from guest OS memory. QGA supplies
  supported guest readings; unavailable guest telemetry is labeled as such.
- TrueNAS memory identifies ARC cache and estimated process use separately.
- Passthrough disks are visible to their owning guest/NAS, not necessarily to
  Proxmox SMART probes. Configure the restricted NAS helper for those disks.
- Current/voltage readings require exposed physical sensors and kernel drivers.
  `turbostat`/RAPL power depends on CPU/kernel support. Overlapping CPU/graphics
  power domains are not independent values to sum.
- GPU support depends on sysfs/vendor telemetry. NVIDIA detailed readings use
  `nvidia-smi`; guest GPUs require an appropriate guest telemetry source.
- Moonraker heater power is duty percentage. The display does not treat it as
  measured watts, and printer time remaining is an estimate.
- Offline and expired sources retain their status/age. A zero reading and an
  unavailable reading are different states.

See [SETUP.md](../SETUP.md) for optional probe packages and [TELEMETRY.md](../collector-rs/docs/TELEMETRY.md)
for implementation details.
