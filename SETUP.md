# Set up Glimdock

## 1. Linux / Proxmox collector

Use a 64-bit Linux host with systemd. Choose the `x86_64` binary for ordinary Intel
or AMD Proxmox hosts, or `aarch64` for 64-bit ARM Linux. Download the matching
[Linux archive and SHA256SUMS](https://github.com/gkostadinov/glimdock/releases/tag/v0.1.0).
Verify the selected checksum as described in [release verification](docs/RELEASES.md),
then extract the archive. Each binary package contains its installer, example
configuration, setup guide and license notices. The source archive is only needed
to build firmware, modify the collector or generate enclosure CAD.
The collector is a single musl executable with Rustls TLS.

From the extracted Linux package directory, install it as root:

```sh
sudo ./deploy/install-rust.sh
sudo editor /etc/homelab-monitor/config.json
sudo editor /etc/homelab-monitor/server.env
```

If building from source, use `sudo ./deploy/install-rust.sh --binary /path/to/glimdock-collector`
instead. Review the script before running it; it changes local accounts, files
and systemd units, then leaves services stopped unless `--start` was supplied.

The installer retains existing configuration and tokens. New installations listen
on `127.0.0.1:8765` until you deliberately set `HOMELAB_BIND` in `server.env` to your
host's LAN IP. Set `host_ip` in `config.json` to that same IP for node metadata.
No packages or firewall rules are changed by the installer. It does not start or
restart services unless `--start` is supplied. The paths and `homelab-monitor-*`
service names are retained so an existing Python installation can be upgraded.

Typical local Proxmox configuration, with an example printer:

```json
{
  "host_ip": "192.0.2.10",
  "node": "",
  "display_name": "My homelab",
  "enable_proxmox": true,
  "interval_s": 3,
  "qga_guest_ids": [100, 101],
  "printers": [{
    "id": "printer", "name": "Workshop",
    "url": "http://192.0.2.20:7125",
    "poll_interval_s": 5, "timeout_s": 2.5, "ttl_s": 15,
    "api_key_file": ""
  }]
}
```

Replace documentation addresses with your own addresses. Omitted options take
their defaults. `node` may be blank to use the local hostname. Local collection
also works on a non-Proxmox Linux machine, though guest and PVE storage sources
will be unavailable there. For a printer-only hub, set `enable_proxmox` to false
and configure at least one printer or remote feed. The display supports four
nodes in total, counting the local host when enabled.

Validate, then start:

```sh
sudo /opt/homelab-monitor/glimdock-collector validate-config --config /etc/homelab-monitor/config.json
sudo systemctl enable --now homelab-monitor-config homelab-monitor-collector homelab-monitor-http
sudo systemctl restart homelab-monitor-config homelab-monitor-collector homelab-monitor-http
systemctl status homelab-monitor-config homelab-monitor-collector homelab-monitor-http
curl http://192.0.2.10:8765/healthz
```

After starting, give slow probes their first sampling interval. Keep the endpoint
on a trusted LAN or use authenticated HTTPS; do not expose this reader directly
to the internet. For TLS, the `serve` command accepts `--cert` and `--key`. Give the
HTTP service read access to only those certificate files, supply a unit override,
and paste the issuing CA PEM into the display's browser setup page. Insecure TLS
fallback is disabled.

### Tokens and privileges

`/etc/homelab-monitor/server.env` is root-only and contains two different tokens:
`HOMELAB_DISPLAY_TOKEN` reads snapshots; `HOMELAB_SETUP_TOKEN` edits node settings.
Copy both privately into the ESP32 setup form. A display-only token cannot edit
nodes and a setup-only token cannot read telemetry. Setup tokens are optional on
the display if you want its settings to remain read-only.

Only local collection and fixed-scope configuration run as root. The HTTP reader
runs as the nologin `homelab-monitor` user and cannot run host commands. Config
changes travel over a group-restricted Unix socket, accept a bounded node schema,
and can restart only the collector service. Direct edits to the configuration
file require a collector restart. URLs cannot contain inline passwords; use the
managed secret files or the node editor instead.

### Optional probes

| Information | Host requirement |
| --- | --- |
| CPU, RAM, load, network, disk I/O | Linux `/proc` and `/sys` |
| Guests and PVE storage | Proxmox `pvesh` and its Perl libraries |
| Temperatures, fans, voltage/current | `sensors -j` from `lm-sensors`; existing kernel drivers |
| Busy clocks, watts, C-states | `turbostat`; supported CPU/kernel and readable RAPL |
| Disk health and temperature | `smartctl` from `smartmontools` |
| ZFS pools/ARC | `zpool` and kernel ZFS statistics |
| Host GPUs | PCI/sysfs; `nvidia-smi` for NVIDIA telemetry |
| Guest OS memory / guest GPUs | QEMU Guest Agent in selected guests |
| NAS pools and passthrough disks | Restricted SSH TrueNAS helper described below |
| Klipper printer | Read access to Moonraker, usually port 7125 |

On Debian/Proxmox, install the optional tools you need through its package manager
(for example `lm-sensors smartmontools`). Package availability for `turbostat`
depends on the distribution/kernel. Missing or expired sources appear unavailable;
Glimdock does not invent measurements. Current readings require a physical current
sensor; heater duty is a percentage, not measured watts. Package/graphics power
domains must not be summed when they overlap.

### Real guest RAM

Enable the QEMU Guest Agent option in each Proxmox VM, install/start the agent in
the guest OS, and list its VM ID in `qga_guest_ids`. Linux normally uses the
`qemu-guest-agent` package; Windows uses the guest-agent MSI from its virtio driver
bundle. Verify `qm agent <VMID> ping` on Proxmox. The collector obtains OS memory
through a fixed read-only guest payload instead of showing the hypervisor's
allocated/resident memory as guest usage. Unsupported or failed guest telemetry
remains explicitly unavailable. TrueNAS memory distinguishes ARC cache and an
estimate from ordinary process use.

### Restricted TrueNAS access (optional)

Use an SSH account allowed to read disk/pool statistics on the NAS. Copy
`agent/truenas_probe.py` to its private directory, mode 0700. Generate a dedicated
ed25519 key on the collector, keep its private file mode 0600, and pin the NAS's
verified host key in a dedicated `known_hosts` file. Do not disable host verification.
Restrict its NAS `authorized_keys` entry to the collector IP and the fixed helper:

```text
restrict,from="192.0.2.10",command="/usr/bin/python3 /ABSOLUTE/NAS/HELPER/truenas_probe.py" ssh-ed25519 PUBLIC_KEY
```

Set `truenas_ssh_host`, `truenas_ssh_user`, `truenas_ssh_key`, `truenas_known_hosts`,
and optionally `truenas_guest_id` in the collector configuration. Leave that NAS VM out
of `qga_guest_ids` so its memory has one source. The helper runs
on the NAS; the Rust collector does not invoke a Python collector locally. Disk
visibility depends on NAS account permissions. Prefer supported NAS account/key
administration to ad-hoc appliance modifications. Never grant this key a general
shell or broad passwordless sudo for monitoring.

## 2. ESP32 firmware

This release supports only **Waveshare ESP32-S3-Touch-LCD-2.8 V1 / CST328**.
Check the hardware revision before flashing. V2 / CST3530 is different. Install
PlatformIO, then build from the source directory:

```sh
pio run --project-dir firmware --environment homelab_s3
pio device list
pio run --project-dir firmware --environment homelab_s3 --target upload --upload-port /YOUR/USB/PORT
```

The project pins its platform, LVGL and ArduinoJson versions. Public source uses
empty Wi-Fi and token defaults. Saved NVS settings take precedence and survive a
normal update. `HOMELAB_ROTATION` is 3 for a USB-right landscape enclosure; set it
to 1 for the opposite direction. GPIO7 is the V1 power latch. The firmware asserts
it early and does not initialize gyro, RTC, audio or sleep modules. If this board
loses power in ROM upload, hold BAT_PWR during the upload and release it after boot.
Do not erase NVS unless you intend to clear all saved connection settings.

Open Settings → Wi-Fi and enter the 2.4 GHz Wi-Fi name/password, snapshot URL
(e.g. `http://192.0.2.10:8765/api/v1/snapshot`), display token and optional setup
token. The on-screen editors mask secrets. For first boot or HTTPS CA setup, join
**Glimdock-Setup** using the generated password shown on the display, then open
`http://192.168.4.1/` and submit the form. The AP is closed after successful setup.

Choose a node in the header. Tap cards/rows for details; drag to scroll. Settings
includes appearance, brightness and node management. Stale/offline readings retain
their age and status instead of silently becoming current values.

## 3. Add other nodes

In Settings → Nodes, add a Klipper node with its Moonraker base URL and optional
API key. Add another Linux/Proxmox node as a **Proxmox feed**, using that collector's
`/api/v1/snapshot` URL and its read token. Remote feeds use the upstream collector's default local Proxmox snapshot; query URLs are not accepted. A printer's key and
remote collector's token remain in mode-0600 files on the hub, not the public
configuration projection. Changing a URL to another origin requires a new secret.

Node deletion requires a deliberate confirmation. Deleting the local Proxmox
node disables its dashboard; it does not remove anything from Proxmox. Wi-Fi is
stored on the ESP32, while node definitions are stored on the collector. Each
source has its own polling and expiry policy so a slow printer/NAS does not stop
other snapshots.

## Troubleshooting

Use `journalctl -u homelab-monitor-collector -u homelab-monitor-http -u homelab-monitor-config`.
HTTP 401 usually means the wrong token role; 404 on a selected node means the ID
was removed; 503 means no valid snapshot is published yet. Test a fresh snapshot
with a private Bearer header rather than putting the token into a URL.
If Wi-Fi cannot connect, check 2.4 GHz support, saved credentials and signal strength.
Missing disk data for a passed-through controller requires collecting from the
owning guest/NAS. For guest RAM, verify QGA service, VM option and configured IDs.
Do not report logs containing tokens, Wi-Fi passwords or private snapshots.
