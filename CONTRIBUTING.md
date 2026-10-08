# Contributing

Small, reviewable changes are welcome. Describe the problem, supported board/OS
and validation. Include synthetic fixtures and a failing behavioral test when
fixing a parser, authentication, configuration or touch regression. Do not submit
real lab snapshots, credentials, manufacturer reference files or copied code
without its license. Contributions retain the applicable license in LICENSE_POLICY.md.

Run the checks in BUILDING.md. UI changes need native LVGL White/Dark captures and
the gesture suite; physical touch/rotation/power changes also need the supported
V1 board. Collector changes must preserve schema 1 selected snapshots, schema 2
aggregates, bounded sizes, expiry semantics and separate read/setup credentials.
Slow-source tests should prove that other nodes continue publishing.

Integrations are read-only. New commands must be fixed argument vectors, have
timeouts/output bounds, and avoid arbitrary shells. Root configuration accepts
only its node schema and managed secret paths. Keep tokens out of URLs/logs and
never carry a secret automatically to an unrelated URL origin.

For V2 support, open a hardware-specific proposal with board revision, controller,
pin mapping, dimensions and measured touch/power results. Do not silently change
the V1 driver for a visually similar product.
