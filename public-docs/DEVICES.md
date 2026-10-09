# Servers, routers and other devices

The Rust central collector manages servers, routers, printers and other devices
in one registry. Proxmox is an optional node capability. The hub can collect its
own native host or aggregate remote feeds with no local node enabled.
Glimdock and the browser firmware renderer discover the configured registry
through the central feed. The default All nodes view presents summary cards;
selecting a card opens that node's dashboard.
The dashboard shows Overview, Storage, Sensors and Health. Available platform
details include OS/kernel, architecture, battery and network interfaces. A device
may provide only sensors; unavailable CPU, memory, power or disk readings stay
`--`. Existing Proxmox guest views and Klipper job views remain available.

| Device/source | Collector | Available readings |
| --- | --- | --- |
| Linux hub host | Optional native Rust collection with `local_type: "server"` | CPU/RAM/load/uptime/I/O, filesystems, hwmon sensors, optional SMART/ZFS/GPU/power/fault tools |
| macOS hub host | Shared portable Rust host collector | Native CPU/RAM, storage, battery, interfaces and available SMC/OS sensors |
| macOS, Windows or Linux device | Native Rust `glimdock-agent` | OS CPU/RAM/swap/uptime/I/O, filesystems, interfaces, CPU frequency, battery and sensors exposed by the OS |
| Router, switch, UPS or appliance with SNMP | Rust agent in SNMP mode | Uptime, configured CPU OIDs, IF-MIB interfaces, HOST-RESOURCES storage and mapped vendor/ENTITY sensor OIDs |
| Device with a JSON HTTP API | Rust agent in JSON mode | Explicitly mapped system metrics, component power and arbitrary numeric sensor readings |
| Proxmox host/feed | Explicit `proxmox` node capability | Host metrics plus guest/container, storage and available assigned GPU telemetry |
| Klipper printer | Moonraker read-only API | Job state, progress, heaters and available temperatures |
| Custom collector | Compatible authenticated snapshot feed | The same bounded schema-1 device telemetry contract |

Support depends on what each OS, driver or device exposes. The portable agent
does not grant extra permissions or install drivers. macOS reads available native SMC/OS temperatures and battery information.
Windows temperatures and fans depend on the drivers and interfaces exposed to
the OS; vendor APIs or an additional exporter may be needed. Unsupported
temperature, SMART, GPU and power measurements remain unknown. SNMP indexes and
vendor OIDs must be configured for the actual device. This does not imply every
router has every metric or that every vendor protocol is built in.

## Optional native hub host

Install or run the Rust hub as described in [SETUP.md](../SETUP.md). A fresh
configuration uses native server telemetry; for example:

```json
{
  "host_ip": "192.0.2.10",
  "display_name": "Linux server",
  "enable_local": true,
  "local_type": "server"
}
```

`enable_local: false` makes the hub a remote-only aggregator. Server mode skips
Proxmox, QEMU guest and NAS guest probes. Linux reads filesystem capacity and
available hwmon sensors directly when `sensors` is absent; installed optional
hardware tools provide their additional readings. macOS reuses the portable
native host collector. The host's ID remains stable when changing its capability.

Choose Proxmox explicitly with `local_type: "proxmox"` on a Linux Proxmox host.
The legacy `enable_proxmox` flag remains an alias for existing configurations;
do not specify it together with `enable_local`. The hub's combined `run` command serves collection, node management and the web
interface on Linux/macOS; the optional systemd installer targets Linux.
Windows devices use the native agent below, rather than the Unix hub executable.

## Native host agent

Use the `glimdock-agent` executable built for the monitored machine's OS and
architecture. Build from source with Rust 1.88 or newer:

```sh
cargo build --locked --release -p glimdock-agent
cargo test --locked -p glimdock-agent
```

The binary is `target/release/glimdock-agent` on macOS/Linux and
`target\release\glimdock-agent.exe` on Windows. Copy it to your
chosen executable directory. Rust is needed for building only; no Python
interpreter or Python packages are needed to run the agent. The same binary
includes host, SNMP and JSON modes; choose one configuration flag per process.

Create a private display token file on macOS/Linux:

```sh
umask 077
/ABSOLUTE/PATH/glimdock-agent --generate-token > /ABSOLUTE/PRIVATE/display.token
chmod 600 /ABSOLUTE/PRIVATE/display.token
```

On Windows PowerShell, use a private directory whose ACL grants access only to
the account running the agent, then save the token without a UTF-8 BOM:

```powershell
$displayToken = & C:\Tools\glimdock-agent.exe --generate-token
[System.IO.File]::WriteAllText("C:\Private\display.token", $displayToken, [System.Text.UTF8Encoding]::new($false))
Remove-Variable displayToken
```

`--generate-token` prints only the new token. Keep the file out of source control.
This token belongs to this agent, separately from hub setup credentials.
Copy [examples/agents/host.json](../examples/agents/host.json)
to a configuration file, set its `address` to the monitored host's LAN address,
and adjust the optional interface/mount selections. The platform is detected
automatically by default.

```sh
/ABSOLUTE/PATH/glimdock-agent --config /ABSOLUTE/PATH/host.json \
  --token-file /ABSOLUTE/PRIVATE/display.token --bind 192.0.2.20
```

```powershell
& C:\Tools\glimdock-agent.exe --config C:\Private\host.json `
  --token-file C:\Private\display.token --bind 192.0.2.20
```

The default listener is `127.0.0.1:8765`; LAN listening requires an explicit
IPv4 `--bind` address. `--port` changes the listener port.
`--cert` and `--key` enable HTTPS. `/healthz` reveals only health, while
`/api/v1/snapshot` and `/api/v1/nodes` require `Authorization: Bearer <token>`.
The agent is read-only and has no setup or control API. `--once` prints one
initialized snapshot for local diagnosis. Run the command under your normal
service manager, launchd or Windows Task Scheduler for persistent monitoring,
using absolute executable, configuration and token paths. The Linux hub installer
does not install this device agent or configure macOS/Windows services. For a
local sample, run `glimdock-agent --config host.json --once`; this initializes
counter baselines before printing and does not require a display token.

## Routers and SNMP appliances

Run the Rust adapter on a machine that can reach the router. Install the Net-SNMP
`snmpget` command on that adapter host and make it available on its PATH. Enable read-only SNMP on
the device using its normal administration interface. Use SNMPv3 with
authentication/encryption where supported, or a restricted read-only SNMPv2c
community on a trusted network.

Copy [examples/agents/snmp.json](../examples/agents/snmp.json), set the
device address and its actual interface/storage indexes and sensor OIDs. Keep
the SNMP credential JSON in the configured `credential_file`, privately readable
by the adapter account. The agent uses a temporary private Net-SNMP configuration
directory so secrets are not passed as process arguments or published in the
snapshot. Its HTTP display token is separate from the router's SNMP credentials.

```sh
/ABSOLUTE/PATH/glimdock-agent --snmp-config /ABSOLUTE/PATH/router.json \
  --token-file /ABSOLUTE/PRIVATE/router-feed.token --bind 192.0.2.30
```

Use a hub feed TTL of at least twice the adapter's sampling interval; the default
SNMP interval is 10 seconds, so 30 seconds is a useful starting TTL.

Each sensor mapping specifies numeric `oid`, `kind`, `unit` and optional `scale`,
`offset`, `high` and `crit`. For example, a temperature OID returning 425 in
tenths of a degree needs `scale: 0.1`. For ENTITY-SENSOR-MIB, configure the actual
instance OID and conversion from its reported scale/precision. No sensor-table
discovery or arbitrary SNMP writes are performed. Vendor CPU and memory OIDs
can supplement standard HOST-RESOURCES data. Missing OIDs remain unavailable,
and counter resets invalidate throughput until a new baseline exists.

## Vendor JSON APIs

Copy [examples/agents/json.json](../examples/agents/json.json) and replace
the example URL and mappings with the device's documented telemetry endpoint.
The example's field names illustrate the mapping format; they are not a
particular router manufacturer's API. Each path is an array of literal JSON keys
and array indices, such as `["sensors", 0, "temperature"]`. Numeric values can
be converted with `scale` and `offset`. Map system readings through `host`,
component readings through `power`, and custom values through `sensors`. APIs
that encode numbers as JSON strings can opt into `"parse": "numeric-string"`
per mapping; strings containing units or expressions are rejected.

```sh
/ABSOLUTE/PATH/glimdock-agent --json-config /ABSOLUTE/PATH/device.json \
  --token-file /ABSOLUTE/PRIVATE/device-feed.token --bind 192.0.2.30
```

The adapter requests only GET from its configured URL, rejects redirects,
validates HTTPS and bounds responses to 48 KiB. Optional `token_file` supplies
the upstream API's Bearer token. It is separate from the agent's display token.
Optional `timestamp_path` identifies a Unix timestamp in seconds and
`sequence_path` a changing nonnegative integer. Configure these when the API
provides them so expired samples and frozen exporters are detected. Without
them, freshness describes the successful API poll. Missing, boolean, string,
nonfinite or invalid numeric values are unknown. Strings require the explicit
numeric-string parser described above. API failures clear measurements
and publish source health without exposing response bodies or credentials.

## Register a device with the central collector

Open the collector web interface at `http://COLLECTOR_ADDRESS:8765/`, choose
**Nodes** → **Add node**, and select the server/device feed type. Enter the
adapter's `http://192.0.2.20:8765/api/v1/snapshot` URL and its display token, then
choose Auto, Linux, macOS, Windows, Router or Other. Auto uses the upstream
platform. A blank new ID is assigned by the collector; later name changes
preserve that ID. Registration is a one-time collector configuration step; it does not scan the
network or discover device credentials. The web console runs alongside collection
as part of the same Rust server.

The physical display's **Nodes & monitoring** → **Device feed** form can make
the same configuration change using its setup token. Glimdock continues to
connect to the central collector, which polls the registered adapter. Newly
registered nodes appear automatically on the display and browser's All nodes
cards; the firmware does not need a separate connection to each device.

The four-active-node limit includes the optional hub host, printers and remote
feeds. Use `enable_local: false` for a remote-only hub or `enabled: false` to pause
a configured feed. The web interface keeps up to sixteen feeds configured and
supports editing, pausing, resuming and deleting them.

The equivalent hub configuration is:

```json
{
  "remote_collectors": [{
    "id": "office-mac",
    "name": "Office Mac",
    "type": "server",
    "platform": "macos",
    "url": "http://192.0.2.20:8765",
    "token_file": "/etc/homelab-monitor/office-mac.token",
    "poll_interval_s": 5,
    "timeout_s": 2.5,
    "ttl_s": 15
  }]
}
```

Set `type: "proxmox"` explicitly for Proxmox collectors. Device feeds require
an upstream `node.type: "server"` descriptor with platform metadata. All feeds
retain upstream source ages, reject demo/printer payloads, and invalidate expired
or frozen upstream snapshots. Node editing stores only the feed token on the hub;
SNMP OIDs and vendor mappings belong to the adapter's configuration file.

Custom collectors can use the same endpoint contract: schema 1, advancing
sequence, Unix `generated_at`, a host object, object-valued source statuses,
bounded inventory arrays, and a server node descriptor. Use JSON null for
unsupported measurements. [device-agent-rs/src/protocol.rs](../device-agent-rs/src/protocol.rs)
defines the native agent's bounded telemetry contract.
