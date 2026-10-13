# Agent operations: portable API and Glimdock display design

This is a working **design prototype and read-only reference API**. It is separate
from the installed ESP32 firmware and the production Rust hardware collector.
The prototype can read actual shared-hub metadata or another system's normalized
snapshot, and demonstrate richer metrics using explicitly synthetic data.

## Four tabs, four main cards

The agents workspace uses **Overview / Agents / Tasks / Events**. The existing
four homelab cards remain a separate workspace; this design does not add a fifth
hardware card. A firmware integration can enter Agents from the fleet's redundant
Pick node tab while retaining the header node picker and Clock access.

- **Overview:** at most four system cards per page. Select a system for four
  metrics: tracked work, input needed, recent sessions, and operations/usage.
- **Agents:** rows in the style of Proxmox Guests. State, tracked task, host,
  heartbeat age and reported usage; tap for model, tokens, cost and source detail.
- **Tasks:** queued/running/waiting/blocked/completed work, owner, elapsed time and
  optional source-provided progress. A parent task ID supports delegated work.
- **Events:** failures, input requests, late heartbeats and other metadata;
  agent/task links retain Back context. Full prompts and transcripts are omitted.
- **Operations**, reached from Overview: queue/delivery counts, resource locks,
  measured channel latency if available, usage and source capabilities.

The display uses the Glimdock brand, white/dark palettes, landscape 320×240 or
portrait 240×320, fixed four-tab navigation and scrollable details. Larger
registries page through four system cards; they do not expand the main grid.
Pointer excursions cancel taps. Expired sources mask current numeric metrics.
Historical events retain their occurrence time; they are not presented as live
work. Unknown progress uses an unfilled patterned bar, not a fake percentage.

## Run the prototype

Use Python 3.10+ and install the small reference dependency in your own virtual
environment. This does not replace or change the Rust collector:

```sh
python3 -m venv .venv-agent-ops
.venv-agent-ops/bin/pip install -r tools/agent-operations/requirements.txt
.venv-agent-ops/bin/python tools/agent-operations/api.py --port 8784
```

Open `http://127.0.0.1:8784/`. **Settings** switches theme, orientation and data
source, and can demonstrate source expiry. The sample contains two heterogeneous
systems, 15 agent records, waiting/blocked work, events, resource locks and usage.
Sample usage is fictional; it is never a claim about your actual model spending.

For the existing authorized hub:

```sh
.venv-agent-ops/bin/python tools/agent-operations/api.py \
  --hub /ABSOLUTE/PATH/fast-share/AGENTS --port 8784
```

Select **Shared hub** in Settings, or open `/?mode=hub`. The default hides task
titles as opaque tracked-task labels. `--include-task-titles` opts into short,
path-redacted titles for a private personal dashboard. It never exports message
text, event text, task descriptions/notes, authorization, project paths, source
thread IDs, credentials or transcripts. An opt-in title can still contain
sensitive human-written metadata; keep the feed scoped to its intended readers.

The exporter only reads the hub. It does not heartbeat, acknowledge, claim,
release, mutate task state, execute commands or awaken agents. It reports corrupt
metadata while preserving valid rows and all source files. The adapter reads
registry, task/claim, resource and message-acknowledgment metadata from a
file-based coordination hub; it does not depend on a particular coding agent.

## Integrate another agent system

The API is language/framework neutral. Any system can produce the JSON shape in
[the schema](../contracts/agent-operations/snapshot-v1.schema.json). Use the
[minimal publisher fixture](../examples/agent-operations/minimal.json), fill only
measurements the source actually has, and atomically replace its own snapshot
file. Do not update its source timestamps while serving an unchanged cached file.

```sh
.venv-agent-ops/bin/python tools/agent-operations/api.py \
  --snapshot /ABSOLUTE/PUBLISHER/PATH/snapshot.json --port 8784
```

Open `/?mode=file`, or consume `GET /v1/snapshot`. The implementation preserves
publisher timestamps. `GET /v1/capabilities` identifies configured source kinds,
read-only operation and the 48 KiB limit. POST/control requests return 405.
The [OpenAPI description](../contracts/agent-operations/openapi.json) documents
these endpoints. Query `mode` is specific to the reference adapter; a production
endpoint can expose its own configured source without that query.

Examples in Python/TypeScript can serve the same JSON; a Rust collector adapter
can poll it with the existing bounded HTTPS transport. Native LangGraph, CrewAI,
AutoGen or other integrations are **not** bundled in this prototype. They need a
source adapter or callback that maps their reported lifecycle into this contract.
There is no mandatory provider account or Glimdock-specific SDK.

## State and measurement semantics

`schema_version` is `agent-ops/v1`. Stable IDs are scoped by system; source IDs
identify provenance and TTL. Systems, agents, tasks, events, resources and
channels have explicit bounded collections. `limits.counts` retain original
counts; truncation is explicit. A client does not infer totals from a partial
array. The reference API rejects duplicate identities, unknown sources, negative
measurements, unmarked truncation and unrecognized content fields.

- `null` means unavailable. Measured zero remains zero.
- `generated_at` is publication time, not proof that a source or agent is alive.
  Source `sampled_at`/`ttl_s` and agent `heartbeat_at`/`ttl_s` are separate.
- A hub heartbeat is **advisory recorded session recency**. Declared active status
  does not prove a model/process is executing. Tracked running work comes from
  an active task with a matching claim owner, not from heartbeat alone.
- Hub reads are best-effort: atomic files do not make a cross-file snapshot
  transactional, and SMB directory listings may lag. Inconsistent claims become
  unknown; the exporter never steals an apparently old lock.
- The hub currently has no model, token, cost, CPU/GPU, approval-count or model
  latency telemetry. Those fields remain unavailable, not zero.
- Usage requires its own source/provenance. Cost also requires currency. Do not
  infer prices, token counts or progress from time, files or event frequency.
- Acknowledgments are receipts. They do not prove a model read, executed or
  approved a message. The Windows-via-Ubuntu dependency is exposed as metadata;
  its current network status/latency are unknown without actual probes.

Two small [mapping helpers](../tools/agent-operations/mappings.py) map A2A task
states and OpenTelemetry token attributes. These are vocabulary adapters, not an
A2A client or OTLP receiver. OpenTelemetry's current GenAI conventions define
agent and token attributes in a dedicated [GenAI repository](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/README.md).
A2A distinguishes working, input-required and auth-required states in its
[task lifecycle](https://a2a-protocol.org/latest/topics/life-of-a-task/); those map
to running and waiting here. Unrecognized states remain unknown.

## Reader access and deployment boundary

The reference server binds to loopback by default. A non-loopback bind requires
`--read-token-file /PRIVATE/PATH/read-token` with a random token of at least 24
characters. Readers send it in an Authorization Bearer header; never put tokens
in URLs, snapshots, screenshots or exported examples. The viewer keeps a typed
read key in session memory, not local storage. Use a TLS reverse proxy or the
collector's existing authenticated transport for a production remote reader.

The API has no execution authority. A future control interface would need a
separate contract, identity and explicit permissions; monitoring data is never
an instruction to run a command. The design/API have not been flashed into the
ESP32. Firmware/Rust collector integration should consume the contract behind
its bounded source scheduler, preserve the four-card/four-tab constraint and
reuse the existing speaker/edge-notification policy for real new attention events.

## Verify

```sh
.venv-agent-ops/bin/python tools/agent-operations/test_api.py
.venv-agent-ops/bin/python tools/agent-operations/check_ui.py
.venv-agent-ops/bin/python tools/agent-operations/check_edges.py
```

API checks cover schema, absent metrics, invalid records, ownership races,
privacy, source immutability, mappings, payload size and denied writes. UI checks
use Playwright CLI snapshots and real pointer input: landscape/portrait, themes,
Back routes, unknown progress, live/sample separation, source expiry and four
cards/tabs. Start the reference server first. UI checks use the installed
Playwright CLI wrapper when available, otherwise the public npm Playwright CLI.
Node.js/npm and Playwright browser dependencies are needed for browser QA; no
Codex account is required. `AGENT_OPS_PLAYWRIGHT_CLI` can select another CLI command.
Live shared-hub checks
run when the server was started with `--hub`; generic sample and publisher checks
do not require that NAS channel.
