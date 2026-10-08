# Architecture and security boundaries

Glimdock has one Rust executable with separate collection, HTTP and configuration
subcommands. Its installer uses three systemd services so privileged host probes
do not run in the network-facing HTTP process.

| Process | Role | Boundary |
| --- | --- | --- |
| `homelab-monitor-collector` | Samples local/guest/NAS/printer sources, publishes bounded snapshots | Root is needed for privileged host probes; commands use bounded fixed arguments |
| `homelab-monitor-http` | Serves snapshots and forwards bounded configuration requests | Unprivileged `homelab-monitor` user; cannot execute host probes |
| `homelab-monitor-config` | Validates node edits and manages node secrets | Root-only files; group-restricted Unix socket; can restart only the collector |
| ESP32 firmware | Native LVGL display and settings | Holds display/setup tokens, not host/Proxmox administrator credentials |

The installed paths and `homelab-monitor-*` names retain compatibility with the
earlier Python deployment. Python source remains for compatibility checks and the
optional NAS helper; a Python collector is not required for the Rust Linux install.

## Data flow

The collector combines local Linux/PVE probes, optional guest-agent readings,
restricted TrueNAS SSH output, Moonraker printer status and other collector feeds.
Sources have independent intervals and expiry. Published snapshots have bounded
sizes, and the firmware shows unavailable/stale status instead of inventing data.

Selected snapshots use schema 1; aggregate node snapshots use schema 2. The
display's usual endpoint is `/api/v1/snapshot`. Health checks use `/healthz`.
See the checked-in source and tests for the complete endpoint/schema contract.

## Read and setup tokens

The installer creates distinct cryptographically random `HOMELAB_DISPLAY_TOKEN`
and `HOMELAB_SETUP_TOKEN` values in root-only `server.env`.

The display token reads telemetry. The setup token changes monitor node settings.
A display-only token cannot edit nodes; a setup-only token cannot read snapshots.
Leave the setup token out of a display intended only for reading.

Node secrets live in managed mode-0600 files on the hub, separate from the public
configuration projection. A URL change to another origin needs a new secret.
Deleting a monitor node does not delete a Proxmox VM, host or printer.

## Network deployment

Fresh installs bind to loopback until the owner deliberately chooses a LAN bind.
Keep plain HTTP on a trusted LAN. Use verified HTTPS with an issuing CA configured
on the ESP32 when the network requires it. Insecure TLS fallback is disabled.
This reader is not designed for direct exposure to the public internet.

TrueNAS SSH uses a dedicated key, a pinned host key and a forced read-only helper
command restricted to the collector IP. Printer integration reads Moonraker
status; it does not offer motion, heater, start, pause or cancel controls.

Report security problems through [private vulnerability reporting](../SECURITY.md).
