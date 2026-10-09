# Devices send readings to one collector

Native Rust agents push readings from Linux, macOS and Windows to the central
collector. A paired SNMP or JSON adapter can collect from a router, switch, UPS or
vendor API and push the same normalized readings. The collector owns the node
registry; Glimdock and its browser renderer read that registry and automatically
show active nodes. The devices need outbound access to the collector and no
incoming agent port.

| Source | Collection | Delivery |
| --- | --- | --- |
| Linux/macOS/Windows | Native Rust CPU/RAM, swap, uptime, I/O, filesystems, interfaces, frequency, battery and exposed sensors | Paired agent push |
| Router/switch/UPS/appliance | SNMP OIDs and read-only vendor API mappings on a reachable adapter machine | Paired adapter push |
| Collector host | Optional native server telemetry | Local collection |
| Proxmox | Explicit capability for host, guests, containers, storage and available assigned GPUs | Local collection or compatible polling feed |
| Klipper | Moonraker job state, progress, heaters and temperatures | Collector polling |
| Existing/custom exporter | Bounded compatible schema-1 snapshot API | Optional authenticated polling feed |

Support depends on the OS, hardware, driver and device. The agent does not install
drivers or grant itself extra permissions. Available macOS SMC/OS temperatures
and battery data are supported; Windows temperatures/fans depend on exposed
interfaces or additional vendor exporters. Unsupported SMART, GPU, temperature
and power values stay unknown. Configure actual SNMP indexes/OIDs and vendor JSON
fields rather than expecting every router to provide every measurement.

## Build or install an agent

Use the `glimdock-agent` executable matching the monitored machine's OS and
architecture. The push protocol is part of v0.3.0; use a matching collector and
agent. The repository source can be built with Rust 1.88 or newer:

```sh
cargo build --locked --release -p glimdock-agent
cargo test --locked -p glimdock-agent
```

The binary is `target/release/glimdock-agent` on macOS/Linux and
`target\release\glimdock-agent.exe` on Windows. Copy it to your executable
directory. Rust is needed to build, not to run the native executable; no Python
interpreter or packages are required. The same executable has native host,
SNMP and JSON modes. Choose one configuration flag per process.

## Pair a device

Open the central console, choose **Nodes** → **Add node** → **Pair a device**, and
enter a device name. The collector address must be reachable from that machine;
`localhost` refers to the machine running the agent. Platform is detected during
enrollment unless selected explicitly. Choose **Create pairing key**, then
**Save key file**. The one-time key expires after ten minutes and is never
retrievable from saved configuration.

Move the file into a private directory on the device as `enrollment.key`.
Keep it out of source control and readable only by the agent account. Copy
[examples/agents/host.json](../examples/agents/host.json) to that directory and
adjust optional mount/interface selections. Its address describes the monitored
host; it is not an incoming listener requirement.

On macOS/Linux:

```sh
mkdir -p /ABSOLUTE/PRIVATE/glimdock-agent
chmod 700 /ABSOLUTE/PRIVATE/glimdock-agent
chmod 600 /ABSOLUTE/PRIVATE/enrollment.key
/ABSOLUTE/PATH/glimdock-agent --collector-url https://COLLECTOR_ADDRESS:8765 \
  --state-dir /ABSOLUTE/PRIVATE/glimdock-agent \
  --enrollment-key-file /ABSOLUTE/PRIVATE/enrollment.key \
  --config /ABSOLUTE/PRIVATE/host.json
```

On Windows, use a private directory with an ACL allowing only the account running
the agent (and required system administrators):

```powershell
& C:\Tools\glimdock-agent.exe --collector-url https://COLLECTOR_ADDRESS:8765 `
  --state-dir C:\Private\glimdock-agent `
  --enrollment-key-file C:\Private\enrollment.key `
  --config C:\Private\host.json
```

HTTPS is verified; supply `--collector-ca-cert /ABSOLUTE/PRIVATE/ca.pem` for a
private CA. To use trusted LAN HTTP, change the URL to `http://...` and explicitly
add `--allow-insecure-http`. HTTP sends credentials and telemetry without
encryption. The collector URL is a base HTTP(S) origin, with no path, query,
embedded credentials or redirects.

The agent creates its durable device identity and private publisher credential
before enrollment. The collector assigns a stable node ID and binds that agent
and platform. After successful pairing, delete the enrollment key file. Future
starts use the same command with `--enrollment-key-file` omitted; keep the same
state directory and source configuration. The publisher credential is separate
from the collector display/management keys and cannot read telemetry, edit nodes
or publish on behalf of other nodes. Do not copy one paired state directory to
multiple machines.

Run this command with absolute paths under systemd, launchd or Windows Task
Scheduler to start it persistently. The Linux hub installer does not install
agents on other computers. Ensure the service account can read the configuration,
private state and any optional source credentials, and that its PATH includes
required adapter tools.

For a local diagnostic sample with no enrollment or listener:

```sh
glimdock-agent --config /ABSOLUTE/PRIVATE/host.json --once
```

`--once` initializes counter baselines before printing one bounded sample.
Unsupported readings stay null; known zero remains zero.

## Freshness and node controls

The collector receives periodic authenticated samples. It records receipt time
and the device's measurement time separately. The web console labels the source
**Agent push** and shows the last receipt; stale measurements cannot become fresh
merely because a new request arrives. Out-of-order or replayed sequences are
rejected. A new authorized agent session can restart its sequence.

Use **Edit** to rename a node and set sending interval/expiry. Allow at least
three sending intervals for expiry. **Pause** retains enrollment but removes the
node from active monitoring; **Resume** restores it. **Revoke access** rejects
further publication immediately, clears current readings and leaves a disabled
node. **Create new pairing key** revokes the old publisher and requires enrollment
again. **Remove node** deletes its registry entry and revokes access. These
collector actions do not remotely terminate the agent process.

An interrupted agent retries with bounded backoff, keeping its identity. The
collector marks silent agents offline after their TTL and clears current numeric
readings. Old samples are not accumulated in an unbounded offline queue. Check
source health as well as the receipt age: an adapter can successfully reach the
collector while its router/API is unavailable.

Four active nodes include the optional collector host; up to sixteen remote
nodes can stay configured. The desk display starts on All nodes and opens a
node's Overview, Storage, Sensors and Health on selection. Proxmox guest views
and Klipper job/temperature views remain available. Enrollment and revocation
are managed from the web console; the firmware can rename, pause, resume and
remove paired nodes with its management key.

## Switch an existing polling device to push

In **Nodes**, edit the existing Server/device node and choose **Switch to push**.
The collector retains its ID (including legacy IDs such as `remote:office-mac`),
replaces the polling entry, and issues a one-time pairing key. Start the new agent
with that key. Polling stops at the saved switch; readings resume after enrollment.
Renaming the node retains its identity and the display's selection.

Keep existing collector/display keys and saved display pairing. Ordinary updates
retain them. No per-device URL or credential is added to Glimdock firmware.
Re-pairing a push node intentionally invalidates its old publisher, while ordinary
rename/pause/resume changes retain it. An expired or consumed enrollment key
requires creating a new key. If the agent's private state is lost, re-pair the
existing node instead of silently creating duplicate identities.

## Recover or renew an existing pairing

When the publisher was revoked or must be renewed, stop the running agent.
In the console, edit the existing node and create a new pairing key. This
retains the central node ID and immediately revokes its previous publisher.
Save the fresh key privately, then run one explicit recovery command:

```sh
glimdock-agent --collector-url https://COLLECTOR_ADDRESS:8765 \
  --state-dir /ABSOLUTE/PRIVATE/glimdock-agent \
  --enrollment-key-file /ABSOLUTE/PRIVATE/enrollment.key \
  --re-enroll \
  --config /ABSOLUTE/PRIVATE/host.json
```

```powershell
& C:\Tools\glimdock-agent.exe --collector-url https://COLLECTOR_ADDRESS:8765 `
  --state-dir C:\Private\glimdock-agent `
  --enrollment-key-file C:\Private\enrollment.key `
  --re-enroll `
  --config C:\Private\host.json
```

Use the same absolute state directory and source configuration as before.
`--re-enroll` requires a new enrollment key file, retains the durable agent ID,
rotates its private publisher credential and enrolls against the existing node.
It is a one-time recovery action. After success, delete the key file and omit
both `--re-enroll` and `--enrollment-key-file` when restarting the normal service;
do not leave these options in permanent service arguments. Retrying a pending
enrollment uses the same rotated credential instead of creating new identities.

If the state was actually lost, create a fresh private state directory and pair
using a new key targeted at the existing central node. Its assigned central node
ID is retained although the agent identity is new. A lost HTTP response can be
retried with existing pending state; deleting state is not the normal recovery.
Do not paste display/management keys in place of an enrollment key.

## Routers and SNMP appliances

Run the adapter on a machine that can reach the device. Install Net-SNMP's
`snmpget` and make it available on that process's PATH. Enable read-only SNMP
through the device's administration interface. Prefer authenticated/encrypted
SNMPv3 where supported, or restrict a read-only SNMPv2c community to a trusted
network.

Copy [examples/agents/snmp.json](../examples/agents/snmp.json), set the address,
actual interface/storage indexes and sensor OIDs. Save SNMP credentials as the
configured private `credential_file` on the adapter host. The adapter creates a
private temporary Net-SNMP configuration rather than putting secrets in process
arguments or snapshots. Router credentials remain separate from the agent's
publisher credential.

Pair this node using the same console flow, then run:

```sh
glimdock-agent --collector-url https://COLLECTOR_ADDRESS:8765 \
  --state-dir /ABSOLUTE/PRIVATE/router-agent \
  --enrollment-key-file /ABSOLUTE/PRIVATE/enrollment.key \
  --snmp-config /ABSOLUTE/PRIVATE/router.json
```

The adapter polls the router locally and pushes to the collector. The default
SNMP sampling interval is ten seconds; use a sending interval appropriate to that
source and an expiry of at least three intervals. OID mappings specify numeric
`oid`, `kind`, `unit`, optional `scale`, `offset`, `high` and `crit`. For example,
425 in tenths of a degree needs `scale: 0.1`. Configure ENTITY-SENSOR-MIB instance
OIDs and scale/precision explicitly. No arbitrary SNMP writes or sensor-table
autodiscovery are performed. Missing OIDs stay unavailable; counter resets clear
throughput until the next baseline.

## Vendor JSON APIs

Copy [examples/agents/json.json](../examples/agents/json.json), replacing the
example URL and mappings with the device's documented telemetry endpoint. The
example is a mapping format, not a built-in manufacturer's protocol. Each path
is an array of literal JSON keys and indexes, such as
`["sensors", 0, "temperature"]`. Use `scale`/`offset` for conversion. Map system
readings under `host`, component readings under `power`, custom values under
`sensors`. Numeric strings require explicit `"parse": "numeric-string"`; unit
strings and expressions are rejected.

```sh
glimdock-agent --collector-url https://COLLECTOR_ADDRESS:8765 \
  --state-dir /ABSOLUTE/PRIVATE/api-agent \
  --enrollment-key-file /ABSOLUTE/PRIVATE/enrollment.key \
  --json-config /ABSOLUTE/PRIVATE/device.json
```

The adapter performs GET only, refuses redirects, verifies HTTPS and caps
responses at 48 KiB. An optional source `token_file` supplies its upstream API
Bearer token, separately from enrollment/publisher credentials. Configure
`timestamp_path` (Unix seconds) and `sequence_path` (changing nonnegative integer)
when provided by the source so expired samples and frozen APIs stay stale.
Without these source fields, measurement freshness describes successful API
collection. API failures clear measurements and report source health without
publishing response bodies or credentials.

## Optional native collector host and polling feeds

A fresh hub monitors its own host as `local_type: "server"`; set
`enable_local: false` for a remote-only collector. Proxmox's additional probes
require explicit `local_type: "proxmox"` on a Linux Proxmox host. Legacy
`enable_proxmox` remains an alias when `local_type` is absent; do not combine it
with `enable_local`. Basic host monitoring does not require Proxmox.

Choose **Add a polling feed** for Moonraker printers and existing compatible
exporters. Existing `remote_collectors`, feed tokens, stable IDs and printer API
keys remain supported. Polling URLs may be a base origin or the fixed snapshot
endpoint; no embedded credentials, redirects or arbitrary API paths are allowed.
A blank saved key retains it for the same origin; changing origin needs a new
credential association. Custom feeds use schema 1, advancing sequences, Unix
`generated_at`, object-valued source statuses, bounded arrays and a server node
descriptor. [protocol.rs](../device-agent-rs/src/protocol.rs) defines this contract.

For compatibility, the native agent can still expose an explicit HTTP listener:

```sh
glimdock-agent --serve-http --config /ABSOLUTE/PRIVATE/host.json \
  --token-file /ABSOLUTE/PRIVATE/feed.token --bind 192.0.2.20
```

Generate a distinct private feed token with `glimdock-agent --generate-token`
when using this mode. The default bind is loopback and `--port` defaults to 8765;
`--cert`/`--key` enable verified HTTPS. `/healthz` exposes health only; snapshot
and inventory endpoints require the feed Bearer token. It has no setup/control
API. Pair-and-push is the usual setup for new native agents.
