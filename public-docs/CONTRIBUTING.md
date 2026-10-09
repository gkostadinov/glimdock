# Contributing

Describe the user problem, affected platform/board and validation in a small,
reviewable change. Keep the central collector host agnostic: new integrations
add a node capability or an adapter, rather than turning that platform into a
hub requirement.

The root Rust workspace is the primary runtime. Build and run the checks in
[BUILDING.md](BUILDING.md). Include synthetic fixtures and a meaningful behavioral
test for parser, authentication, configuration, freshness or touch regressions.
Do not submit real snapshots, keys, manufacturer references or copied code
without its license. Contributions retain the scope in
[LICENSE_POLICY.md](../LICENSE_POLICY.md).

Collector changes must preserve selected schema-1 snapshots, schema-2
aggregates, bounded sizes, source-age semantics, stable node identities and
separate telemetry/management roles. Cover empty and paused inventories.
Slow or failed adapters must not prevent other nodes from publishing.
Device support belongs in native host, read-only SNMP, mapped JSON or an
explicitly scoped integration. Missing fields remain unknown.

Firmware UI changes need native White/Dark captures, pointer checks and the
browser target rebuilt from the same code. Hardware driver, rotation, power or
USB changes also need the supported V1 board. Keep the published firmware
manifest, source/relink materials and dependency notices version matched.
Enclosure changes need their own source/export ledger and physical fit evidence;
a render or body-only print is not an assembled-fit result.

Integrations are read-only. External commands use fixed argument vectors,
timeouts and output bounds, without arbitrary shells. Management accepts only
the node schema and service-managed secret paths; it cannot control guests or
printers. Keep tokens out of URLs/logs and do not transfer them automatically to
a different origin.

For V2 support, provide a separate board proposal with revision, controller,
pin mapping, dimensions and measured touch/power behavior. Preserve the V1
integration while validating the new hardware.
