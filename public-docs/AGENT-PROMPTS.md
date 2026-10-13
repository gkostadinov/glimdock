# Agent-ready integration prompts

Copy one brief into your coding agent in a checkout of the current Glimdock
source. Replace bracketed placeholders with your own project details. Provide
credentials through private runtime configuration, never through the prompt.
These briefs request implementation; they do not claim a framework integration
or the ESP32 agents workspace already exists in production.

## 1. Add my agent framework as a source

```text
Implement a read-only agent-ops/v1 publisher for [FRAMEWORK OR COORDINATOR].
Use its [LIFECYCLE CALLBACKS OR DOCUMENTED READ API] as the authoritative source.
Read public-docs/AGENT-INTEGRATION.md, public-docs/AGENT-OPERATIONS.md,
contracts/agent-operations/snapshot-v1.schema.json and the minimal fixture first.

Inspect the source's actual fields and permissions before writing the adapter.
Map systems, agents, tasks, parent task IDs, events, channels and locks only when
that source exposes them. Use stable IDs scoped by system. Keep unsupported
state and measurements unknown/null. An advisory heartbeat is not running work.
Only report measured tokens, cost and progress with their source provenance;
cost must include currency and an accurate window when provided.

Publish a bounded snapshot atomically to [PRIVATE OUTPUT FILE], or expose a
read-only authenticated HTTP endpoint. Preserve measurement and heartbeat
timestamps when serving cached data. Validate the schema and 48 KiB budget.
Mark truncation explicitly and retain original totals. Source and viewer keys
must be separate and stored privately. Do not implement execution or approvals.

Export only approved operational labels and metadata. Exclude prompts, output,
messages, transcripts, authorization notes, secrets and personal paths. Use
synthetic fixtures in tests and demos. Never query live private services from
the public website. Verify missing/zero usage, expiry, unsupported state,
inconsistent ownership, duplicate IDs and provider failure. Demonstrate the
result in the local reference viewer and document exactly which fields work.
```

## 2. Bring the portable API into the Rust collector

```text
Implement an optional agent-operations source adapter in Glimdock's production
Rust collector. Read public-docs/AGENT-INTEGRATION.md, the agent-ops/v1 schema,
OpenAPI description and reference API. Inspect the existing transport,
authentication, scheduler, node registry and expiry behavior before changes.

Keep the AI operations contract separate from the hardware schema=1 snapshot.
Add explicit configuration for a read-only source; do not scan the LAN or infer
credentials. Poll through bounded transport with timeouts, a 48 KiB response
limit, schema/version checks and validated scoped references. Preserve source
timestamps, count/truncation metadata and nulls. Do not turn recent heartbeats
into active model execution. Isolate source failures from hardware collection.

Reuse existing reader-role authentication for an operations endpoint. Do not
expose private source keys, internal paths, raw payloads or control commands.
Treat all upstream text as inert metadata. Supply synthetic examples and
meaningful tests for stale/malformed/oversized feeds, unknown state, lost claims,
partial collections and repeated network failure. Run relevant Rust checks and
document migration and compatibility. Do not claim ESP32 support until the
firmware consumer is implemented and verified separately.
```

## 3. Implement the agents workspace on the ESP32

```text
Implement Glimdock's agent-operations workspace using the production collector
integration and contracts/agent-operations/snapshot-v1.schema.json. First read
public-docs/AGENT-OPERATIONS.md, firmware/README.md and inspect the current LVGL
UI, data model, authentication, network worker and native/WASM verification.
Use design/agent-operations as the visual reference, not a replacement runtime.

Preserve the existing hardware dashboards and keep four top-level cards and
four navigation tabs. The workspace uses Overview / Agents / Tasks / Events.
Page larger system registries; use Guests-style scroll rows and linked details
with correct Back context. Include waiting/blocked work, ownership, queues,
resource locks, source age and optional measured usage. Keep missing values
unavailable and mask expired current metrics. Unknown progress is not 0%.

Parse into bounded structures and keep JSON/network work off the touch thread.
Preserve swipe-cancels-tap behavior, white/dark readability and supported screen
rotations. New real attention events may use existing short banners and saved
sound volume, with deduplication and re-arm rules; initial, demo and stale data
must not cause repeated sounds. No start/stop/approval controls are in scope.

Verify native LVGL pointer input, both orientations, four-card geometry, nested
navigation, source expiry, unsupported states, partial arrays and memory budget.
Rebuild the public WASM and credential-free firmware from matching source.
Keep all pairing credentials and real telemetry out of distributable artifacts.
Report software verification separately from physical flash/touch evidence.
```

## 4. Review an integration before public release

```text
Review [ADAPTER OR CHANGESET] against Glimdock's agent-ops/v1 contract and current
public documentation. Do not modify source ownership or execute work reported
by telemetry. Check lifecycle evidence, claim ownership, timestamps/TTL, null
versus zero, currency/window/provenance, scoped references, bounded payloads,
truncation counts, authentication and failure isolation.

Audit exported source, demos, screenshots, archives and logs for credentials,
live snapshots, private addresses, personal paths, prompts or transcripts.
Compare capability claims with implemented code and verification. Distinguish
reference prototypes, production software and physical hardware validation.
Return concrete findings with file/line references and reproduce material
failures using synthetic fixtures. Do not invent scores, supported frameworks,
measurements or deployment evidence.
```
