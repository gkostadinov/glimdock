# Integrate an agent system with Glimdock

The portable `agent-ops/v1` contract is a **read-only monitoring interface**. It
describes agent systems, sessions, tasks, events, channels, resource ownership and
reported usage. It is independent of the Rust **device agent** that collects host
telemetry. An AI framework does not need to enroll as a hardware device to use it.

The current implementation is a browser design and Python reference API. The
installed ESP32 firmware and production Rust collector do not yet consume this
contract. [AGENT-OPERATIONS.md](AGENT-OPERATIONS.md) documents the working prototype;
[AGENT-PROMPTS.md](AGENT-PROMPTS.md) provides implementation briefs.

## Choose an integration boundary

| Source | Adapter responsibility |
| --- | --- |
| Framework lifecycle callbacks | Translate reported task/agent transitions into a snapshot |
| Existing orchestration API | Poll its authenticated read API; preserve source timestamps |
| File-based coordination | Read metadata without mutating claims, locks or messages |
| A2A task updates | Map task states; retain unknown states and scoped IDs |
| OpenTelemetry GenAI attributes | Map actually reported token counts and their provenance |

These are integration approaches, not bundled native LangGraph, CrewAI, AutoGen,
A2A-client or OTLP-receiver implementations. The small vocabulary helpers in
`tools/agent-operations/mappings.py` do not connect to those services themselves.

## Start with a minimal publisher

Copy [minimal.json](../examples/agent-operations/minimal.json) into your adapter.
It includes one system and one unknown-state agent, with optional measurements
set to `null`. Use IDs that remain stable across renames, scoped by system. Names
should be approved operational labels, not prompts or private project paths.

The [Python publishing helper](../examples/agent-operations/publisher.py) validates
and atomically publishes a snapshot. Call it after your adapter records a real
source update:

```python
from publisher import publish_snapshot

# Build this object from your framework's actual lifecycle and telemetry.
publish_snapshot(snapshot, "/PRIVATE/STATE/agent-operations.json")
```

The helper does not infer task state, increment token counts, probe hosts or
change source timestamps. Your adapter supplies `sampled_at`, `heartbeat_at`,
`generated_at` and `sequence`. A successful HTTP fetch does not refresh an old
source measurement. For a deliberately simulated demonstration, set `demo: true`.

Serve the file locally with the reference API:

```sh
python3 -m venv .venv-agent-ops
.venv-agent-ops/bin/pip install -r tools/agent-operations/requirements.txt
.venv-agent-ops/bin/python tools/agent-operations/api.py \
  --snapshot /PRIVATE/STATE/agent-operations.json --port 8784
```

Open `http://127.0.0.1:8784/?mode=file`. A reader consumes `GET /v1/snapshot`;
`GET /v1/capabilities` reports configured source kinds and the 48 KiB maximum.
The [schema](../contracts/agent-operations/snapshot-v1.schema.json) is authoritative;
the [OpenAPI description](../contracts/agent-operations/openapi.json) describes
the reference endpoint. Other languages can serve the same validated JSON.

## Map lifecycle honestly

| Contract state | Evidence needed |
| --- | --- |
| `queued` | Source explicitly queued the task |
| `running` | Source reports active work; a file hub also needs a matching claim |
| `waiting` | Source explicitly requests input or authentication |
| `blocked` | Source reports a blocking dependency |
| `done`, `failed`, `canceled` | Explicit terminal result |
| `unknown` | Missing, inconsistent or unsupported state |

An advisory session heartbeat is recency evidence. It does not prove that a model
is currently executing, a process is healthy or an approval is pending. Do not
map every recent session to `running`. Parent task IDs describe delegated work;
they are not execution instructions.

## Measurements and freshness

- `null` is unavailable; measured zero stays zero.
- Usage must come from the source. Cost needs currency and provenance; do not
  estimate it from wall time, files or guessed model pricing.
- Source TTL, entity sample time and agent heartbeat are distinct. Cached feeds
  retain their original timestamps, so clients can expire them.
- Bound every collection and payload. Preserve original collection totals in
  `limits.counts` and mark omitted records in `limits.truncated`.
- Track counters within a stated window. Do not label a lifetime counter as a
  daily value. Do not sum costs across incompatible currencies or windows.
- Events are concise metadata. Omit prompt bodies, model output, message text,
  transcripts, authorization notes, credentials and personal filesystem paths.

## Secure the reader

Keep the reference server on loopback unless remote access is needed. A LAN bind
requires `--read-token-file` with a random token of at least 24 characters. Send
it only as `Authorization: Bearer …`; never in a URL or snapshot. Use HTTPS or a
trusted authenticated reverse proxy for remote readers. Keep source credentials
separate from the viewer's read key. No POST/control endpoint is provided.

The public website uses synthetic fixtures only. Do not connect a public demo to
an internal coordinator, NAS, host probe or production telemetry endpoint.

## Acceptance checks for an adapter

Validate representative snapshots against the schema and the reference validator.
Exercise a missing metric, measured zero, an expired source, an unknown lifecycle
state, duplicate IDs, a lost owner claim, an oversized collection and a provider
failure. Check that the adapter leaves its source unchanged and exports only
approved metadata. Then test navigation with your snapshot in the local viewer.

For a production Glimdock integration, also verify bounded Rust parsing, read-key
roles, scheduler isolation and the four-card/four-tab layout. Those changes are a
separate implementation step, not capabilities of this prototype.
