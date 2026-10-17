# Validation scope

The version 0.3 centralized Glimdock runtime is tested as a Linux/macOS Rust
server with native push agents and an embedded web console. Current CI checks
workspace tests, native agents on Linux/macOS/Windows, public firmware and
WebAssembly behavior, pointer handling and public source packaging.

The combined `run` integration checks initialization without replacing saved
state, distinct credentials and loopback/remote roles, registered feed editing,
configuration conflicts, optional-host removal and an empty registry. Concurrent
runs are rejected. Startup failures and SIGTERM release listeners, private
sockets, locks and bounded probe workers.

Push coverage includes one-time enrollment, exact lost-response recovery after
grant expiry, scoped publisher credentials, durable private state, duplicate
and retired-session rejection, bounded latest-sample delivery, network retry,
sample expiry and node pause/revoke/re-pair. Polling-to-push migration retains
the node identity. OS, SNMP and mapped JSON collectors use the same push
transport; device-specific readings remain labeled by their source.

The shared firmware renderer has passed a full 320 × 240 native/WebAssembly
pixel comparison. The All systems view and selected-node navigation are exercised
through real LVGL input. The website runs this renderer with explicitly authored
example Mac, Windows, router and printer readings.

The configuration parser retains up to sixteen saved remote nodes and limits
active nodes to four top-level nodes and eight total nodes. Parent grouping
preserves child identity and independent freshness. Pointer and WebAssembly checks cover paused nodes, agent edits,
capacity, duplicate identities and active selection. Publisher credentials are
held by the agent and are never copied into display settings.

The public firmware image was programmed to a physical V1 display through the
guarded command-line uploader, and it reconnected using saved NVS pairing.
Chrome/Edge Web Serial image planning and hash-verification behavior are tested;
a browser-controlled hardware transaction is not yet recorded. The current native USB opening and complete case assembly require separate
physical fit checks.

Workspace operational logs and older device observations remain local evidence.
A source or binary package does not contain private addresses, credentials or
live telemetry. Synthetic snapshots in `examples/` are labeled as examples.

The agent-operations transport is implemented in the Rust collector and native
ESP32/WASM firmware. Compiled UI checks cover logo mode switching, four-card
geometry, detail/back navigation, swipe cancellation, unknown progress, scoped
references, heartbeat expiry and source failure. Physical verification is
recorded separately from software checks. Public examples and the website demo
are synthetic; no live hub telemetry or personal coordination history is distributed.

## Flipper Zero cache — 17 October 2026

The optional [Flipper companion](../integrations/flipper/README.md) was physically
tested with a Waveshare ESP32-S3-Touch-LCD-2.8 V1, Flipper Momentum mntm-012 and
six active nodes. USB screen capture recorded 14 local node changes at
19.71–24.59 ms. All 16 offline node/page navigation checks passed. Source ages
continued advancing while disconnected; cached pages stayed available, and
uncached pages identified the selected node while awaiting details.

Saved-pair reconnect required no new PIN, preserved the latest selected target
and delivered fresh detail pages in the background. Wi-Fi remained connected,
no firmware errors or reboots were observed, and saved NVS was verified
unchanged through the guarded firmware update.

Portable C99/C++17 and AddressSanitizer/UndefinedBehaviorSanitizer checks cover
atomic catalogs for up to eight stable node IDs, selection wrapping, rapid
selection changes, reordered and removed nodes, malformed packets, sequence
replay, clock wraparound and offline source ages. New source-age snapshots can
refresh unchanged healthy readings; retransmitted packets cannot revive old
samples. Loading responses preserve complete cached detail pages. The cache
lives only in RAM and clears when the Flipper app exits.

Both Momentum mntm-012 and official Flipper firmware 1.5.1 app builds passed.
Physical validation above applies to V1 with Momentum mntm-012; the official
1.5.1 build has not been tested on hardware.
