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

Native Rust agents pair with a ten-minute single-use enrollment key, then publish
with a per-agent credential bound to one node and platform. Display/management
keys cannot authorize ingest; publisher credentials cannot read telemetry,
edit configuration or publish for another node. The collector stores credential
hashes, never returns saved keys, and rejects replayed or stale samples. Revoke or
remove a node to disable its publisher immediately. Re-pairing also revokes the
prior publisher and requires a new enrollment. Stop the agent and use one
`--re-enroll` invocation with a fresh pairing key and its existing private state:
this preserves device identity while rotating the publisher credential. Omit
recovery/enrollment options from normal service arguments after success.

The agent state directory is private: mode 0700 with atomically written mode-0600
state on Unix, a protected account-specific DACL on Windows. Keep its durable
identity and publisher credential local; do not clone paired state across devices.
Move downloaded enrollment keys to a private file and delete them after pairing.
Commands use key files, not secrets in command arguments or shell history.
Push agents open no incoming HTTP listener; the read-only polling listener
remains an explicit compatibility mode with a separate feed token.

Agent HTTPS verifies certificates and accepts a configured private CA. Plain LAN
HTTP requires explicit `--allow-insecure-http` and provides no transport
encryption. Use it only on a trusted network. Redirects and credential-bearing
URLs are rejected.

Keep SNMP and vendor API credentials private on the adapter host, separate from
the per-agent publisher credential. SNMP credentials use
a temporary private Net-SNMP configuration. JSON adapters use GET, reject redirects
and verify HTTPS. Device credentials never belong in URLs or telemetry.

Public builds exclude private firmware defaults, state, keys and saved live
snapshots. Source releases use an explicit allowlist and synthetic examples.
Do not upload private source/relink modifications containing personalized
credentials. [DEVICES.md](DEVICES.md) describes adapter configuration and
[BUILDING.md](BUILDING.md) describes public assets and verification.
