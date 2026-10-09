# Native telemetry and node capabilities

The central collector and device agents run as compiled Rust executables.
The collector is independent of the devices it monitors: Linux/macOS host
collection, registered feeds and read-only printer APIs share a registry.
Proxmox is an optional node capability, not a prerequisite or preferred default.

## Runtime and registry

`glimdock-collector run --state-dir DIR` runs collection, node configuration and
the built-in web console in one Linux/macOS process. It generates private
configuration and distinct display/setup token files when missing, watches
validated inventory changes and retains existing state. Basic host collection
can run under the current account. The optional Linux systemd deployment
separates privileged probes/configuration from the unprivileged HTTP reader.

Fresh configuration enables the collector's native host with
`local_type: "server"`. Set `enable_local: false` for a remote-only hub, including
an initially empty registry. Server mode skips Proxmox, QGA and NAS guest probes.
Linux retains available native counters, mounted filesystem capacity and optional
hardware tools. macOS reuses the portable device agent's CPU, memory, filesystems,
battery, interfaces and available sensors. Unsupported or unreadable data is null.

Select `local_type: "proxmox"` explicitly on a Linux Proxmox host. Legacy
`enable_proxmox` configurations retain their mode and original identity when
`local_type` is absent; it remains an alias for `enable_local`, and both cannot
appear together. `local_node_id` preserves the host identity across name and
capability changes. A disabled/removed host retains restoration metadata.

A registered feed receives a stable ID when omitted. Later name changes keep it.
Registration is explicit: the collector does not scan the network or obtain
credentials automatically. Enabled nodes appear automatically on connected
displays. Up to sixteen feeds can remain configured, with four active nodes
including the optional host. Paused feeds keep settings and credentials and
are not polled. The default node follows inventory order, with no preferred OS.

Windows is supported as a monitored device through `glimdock-agent`.
The central configuration service uses Unix sockets; a Windows hub is not
currently implemented. [SETUP.md](../../SETUP.md) covers the one-command server,
local access, LAN credentials and optional split-service deployment.

## Portable devices and adapters

`glimdock-agent` supplies native Linux/macOS/Windows snapshots, SNMP router/device
snapshots and mapped JSON API snapshots through the same authenticated read-only
HTTP contract. Each process selects one mode. SNMP uses installed Net-SNMP
`snmpget`; vendor JSON mappings use the Rust HTTP client. No Python collector
is in either runtime path.

`remote_collectors` accepts `type: "server"` or `"proxmox"`. Server feeds require
a live schema-1 snapshot with a server descriptor. `platform` can be Linux,
macOS, Windows, Router, Other or omitted for the upstream descriptor. Platform
data and source inventories are retained within bounds. Proxmox feeds keep their
descriptor-free legacy compatibility. The upstream registry is discarded so a
registered feed represents that selected upstream node rather than duplicating
a whole second fleet.

Configured endpoints are fixed HTTP(S) origins or the fixed snapshot path.
Redirects are refused, HTTPS is verified, credentials are scoped to their origin,
responses are bounded, and frozen sequences and expired timestamps remain stale.
[DEVICES.md](../../public-docs/DEVICES.md) gives actual flags, examples and platform
limits. SNMP OIDs and JSON mappings belong to the adapter, while the central
registry stores the normalized feed URL, token and sampling policy.

## Built-in console and API roles

The Rust server embeds the public `collector-web/index.html`, `assets/`,
`emulator/` and `firmware/` files at build time. Overview shows the fleet,
Nodes manages the registry, and Display & firmware runs production LVGL through
WebAssembly and provides the supported public USB update. Private state, build
sources, development dependencies and keys are excluded from served assets.

The display token authorizes `GET /api/v1/snapshot`, `/api/v1/nodes` and
`/api/v1/snapshots`. Selected snapshots use schema 1; the browser aggregate uses
schema 2. Descriptors carry finite bounded summaries whose source age and TTL
are preserved. Offline/expired summaries mask current numeric values.
An empty registry returns a default schema-1 snapshot with null readings and
`nodes: []`; an explicit unknown selection returns 404.

The separate setup token authorizes versioned `GET` and `POST /api/v1/config`.
Public configuration uses plain `server`, `proxmox` and `klipper` types with
`origin: "host"` or `"feed"`. Compatibility mutation types remain accepted.
Configuration reveals only `has_secret`; keys and local secret paths are private.
Empty submitted keys preserve an existing association for the same origin;
`clear_secret: true` removes it. A stale version returns 409.
Neither API role authorizes guest or printer control.

The combined server's no-key convenience is restricted to actual loopback peers,
a loopback Host and same-origin browser writes. Remote clients always need keys.
For a LAN HTTP hub, the Rust loopback companion serves the same console on the
USB computer and forwards only fixed APIs with the appropriate server-side key:

```sh
glimdock-collector serve --bind 127.0.0.1 --port 8766 \
  --upstream http://COLLECTOR_ADDRESS:8765 \
  --token-file /PRIVATE/PATH/display.token \
  --setup-token-file /PRIVATE/PATH/setup.token
```

Web Serial requires desktop Chrome/Edge on HTTPS or localhost. The updater checks
board, manifest and image hashes, writes four segments outside NVS and restarts
the board. Public images contain blank pairing defaults; saved NVS provides the
existing connection. The firmware build supplies matching source/relink materials.
The browser UI emulates the display renderer and interactions, not the ESP32 CPU
or peripherals. [BUILDING.md](../../public-docs/BUILDING.md) covers all asset builds.

## Optional Linux and Proxmox sources

Probe tools are fixed read-only commands. Missing permissions, unsupported
hardware or absent tools produce unavailable sources and null readings rather
than invented zeroes. Direct hwmon remains available when `sensors` is absent.
Some extra probes require root or explicit permissions; basic generic collection
does not make elevated privileges a requirement.

| Source | Read path | Freshness ceiling |
| --- | --- | --- |
| Host CPU, RAM, swap, ARC, network and block I/O | `/proc` and `/sys` | A new sample every publication |
| Ordinary Linux host filesystem capacity | Fixed local `df -P -B1 -l` read | At least 90 seconds, or three polling intervals |
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

The existing `integrations/truenas/probe.py` compatibility helper remains the NAS-side
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
