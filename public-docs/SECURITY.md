# Security

Glimdock is a local-network monitoring project. Use private LAN access or verified
HTTPS. Report vulnerabilities privately through the repository's
**Security → Report a vulnerability** feature; do not open a public issue with
credentials or a real snapshot.

Include the version, platform, reproduction steps and a synthetic example.
Remove Wi-Fi details, Bearer/API tokens, SSH keys, personal hostnames and addresses.
See [SETUP.md](../SETUP.md) for the runtime and credential model.

## Collector access

The display key authorizes telemetry; the separate setup/management key
authorizes bounded node configuration. Neither role provides VM, printer or
arbitrary command control. Direct remote browser access keeps entered keys only
in session memory. The combined `run` command's convenience access is limited to
actual loopback peers, a loopback Host and same-origin browser writes; LAN
requests need credentials. The localhost companion forwards only fixed upstream
API routes and keeps the corresponding keys server-side.

Keep the state directory private. `run` stores its configuration and tokens in
mode-0600 files; the Linux installer uses the root-only
`/etc/homelab-monitor/server.env`. Rotate exposed role tokens and update paired
clients. Feed credentials are separate private files, never returned in public
configuration. A changed service origin requires a new credential association.
Delete obsolete managed node secrets.

Use a verified CA for HTTPS. Pin SSH host keys and restrict NAS keys to the fixed
read-only helper. Browser firmware updates verify supported board and image
hashes and retain NVS; a deliberate full erase is required when clearing device
pairing. This development firmware does not encrypt saved credentials at rest.

## Device adapters and source exports

Native Rust agents have their own display token and no setup/control API.
Keep SNMP and vendor API credentials private on the adapter host, separate from
the normalized feed token stored on the central collector. SNMP credentials use
a temporary private Net-SNMP configuration. JSON adapters use GET, reject redirects
and verify HTTPS. Device credentials never belong in URLs or telemetry.

Public builds exclude private firmware defaults, state, keys and saved live
snapshots. Source releases use an explicit allowlist and synthetic examples.
Do not upload private source/relink modifications containing personalized
credentials. [DEVICES.md](DEVICES.md) describes adapter configuration and
[BUILDING.md](BUILDING.md) describes public assets and verification.
