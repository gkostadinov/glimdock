# Configuration and example readings

`collector.json` is a generic optional-host collector configuration.
`nodes/proxmox.json` explicitly enables additional Proxmox probes.
`agents/` contains native OS, SNMP and mapped JSON API adapters.
`snapshots/host.json` and `all-nodes.json` are authored synthetic readings.
`firmware-inventory.json` is a separate hypervisor inventory fixture for UI tests.

Examples use documentation addresses, no credentials and no live telemetry.
Copy adapter files into your private state directory before adding actual keys.
See the root SETUP guide and `public-docs/DEVICES.md`.
