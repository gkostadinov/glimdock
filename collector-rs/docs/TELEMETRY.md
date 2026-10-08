# Native telemetry and compatibility helpers

The collector and HTTP/configuration services are compiled Rust. There is no
local Python interpreter or Python collector process in the runtime path.
Optional probes use fixed read-only tools already installed on a Linux/Proxmox
host. Missing tools, permissions or unsupported hardware produce an unavailable
source and JSON `null` values rather than invented zeroes.

| Source | Read path | Freshness ceiling |
| --- | --- | --- |
| Host CPU, RAM, swap, ARC, network and block I/O | `/proc` and `/sys` | A new sample every publication |
| Proxmox guests, storage and I/O wait | Fixed `pvesh get` endpoints | At least 30 seconds, or three polling intervals |
| Temperatures, fans, voltage and supported current/watts | `sensors -j` | At least 20 seconds, or three polling intervals |
| CPU package, cores and graphics-domain power | Fixed one-second `turbostat --Summary` sample | At least 20 seconds, or three polling intervals |
| Local drive health and temperature | `smartctl` JSON; standby checks for ATA/SCSI | At least 900 seconds, or three polling intervals |
| Local ZFS pools | Fixed `zpool list` columns | At least 90 seconds, or three polling intervals |
| PCI GPUs and passthrough ownership | PCI sysfs, Proxmox VM configuration, `lspci`, optional `nvidia-smi` | At least 90 seconds, or three polling intervals |
| Real VM OS RAM and passed-through GPU metrics | Existing QEMU guest agent | At least 45 seconds, or three polling intervals |
| TrueNAS pools, passed-through drives, cached temperature and real OS RAM | Restricted fixed SSH helper | At least 90 seconds, or three polling intervals |
| Segfaults, traps, crashes, hardware, OOM, lockup and filesystem/I/O faults | Bounded seven-day `journalctl` query and current-boot `dmesg` | At least 90 seconds, or three polling intervals |

Slow probes run independently with at most one pending probe per source and at
most ten executing local probes. CPU, network, disk and guest throughput are
counter deltas: the first reading and resets stay unknown until a baseline is
available. Linux diskstats sectors use 512 bytes even on disks with 4K sectors.
Windows NVIDIA memory is converted from MiB into bytes.

All output is bounded to the display contract: 48 guests, 64 sensors, 16 storage
items, 16 disks, 8 GPUs, 24 alerts and 16 fault events, within a 48 KiB node
snapshot. Omitted items remain visible through limits/counts. NaN, infinity,
invalid ranges and boolean values cannot become numeric telemetry.

## QEMU guest agent

Enable an existing QEMU guest agent in each selected VM, then put its numeric
VM ID in `qga_guest_ids`. Proxmox must be able to read the VM configuration and
use its installed QGA transport libraries. Proxmox IPC needs the privileged
collector's primary group to remain root. The collector does not install guest
packages, change ballooning, enable agents, restart guests or modify VM config.

`helpers/qga-bridge.pl` is a fixed transport shim using Proxmox's installed Perl
libraries. It accepts only four read-only telemetry actions, validates IDs,
sets a twelve-second monitor timeout, and bounds captured output. Windows reads
CIM OS memory and optional NVIDIA metrics using a fixed PowerShell payload.
Linux guests run an embedded fixed Python standard-library payload that reads
procfs/sysfs and optional NVIDIA metrics. **This Python code runs inside the
Linux guest, never as the host collector.** Linux guest telemetry therefore
requires an existing `python3` executable in that guest. The helper's payload is
compiled into the Rust binary, so the host does not need an installed helper
file to invoke it.

A QGA process ID persists when an execution-status transport call times out.
The next poll checks that same process instead of launching another one. Guest
stop/start, reboot and assigned-memory changes invalidate old samples; a
previous-boot process ID is discarded. When configured OS telemetry is
unavailable, primary guest RAM is unknown. Proxmox estimates and host QEMU
footprint remain separate fields rather than replacing real OS RAM.

For Intel Linux GPUs, the guest reports deduplicated DRM client engine
nanoseconds. Rust calculates the busiest engine's utilization between samples,
normalizing by engine capacity. The initial baseline, incomplete scans and
counter regressions yield unknown load. Changed client coverage is explicitly
marked partial. Host and guest PCI bus numbering can differ: passthrough
telemetry matches vendor/device identity together with the owning VM.

## TrueNAS

The existing `agent/truenas_probe.py` compatibility helper remains the NAS-side
helper. It uses the NAS's existing Python interpreter and fixed `midclt` reads.
Deploy it under a restricted SSH key whose forced command runs only that helper.
The native host collector uses a fixed `ssh ... snapshot` request with strict
host-key checking, a pinned known-hosts file, identity-only authentication, and
no password fallback. Configure `truenas_ssh_host`, `truenas_ssh_user`,
`truenas_ssh_key`, `truenas_known_hosts`, and the associated `truenas_guest_id`.
Do not also include that VM in `qga_guest_ids`: memory must have one source.

The NAS sends cached temperatures, ZFS errors and pool/scrub state. ZFS ONLINE
is **not** SMART passed. SMART health remains unknown when the restricted NAS
account cannot access it. A NAS boot-ID change resets its throughput baseline.

CPU RAPL power is a component/domain reading; GPU power is board power. Neither
is advertised as whole-system power. Current in amps appears only when hwmon
actually reports it; watts are never converted into guessed rail current.
Kernel crash counts are matching log records, not a claim of unique crashes.
Journal/current-boot kernel copies of the same event are deduplicated.
