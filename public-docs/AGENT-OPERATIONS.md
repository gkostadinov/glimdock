# Agent operations: portable API and native Glimdock workspace

The production Rust collector and ESP32 firmware support a **read-only agent
operations workspace**. The public interactive demo runs this same LVGL firmware
with synthetic data. A Python reference API can read file-hub metadata or another
system's normalized snapshot. Native framework clients and OTLP ingestion are
not bundled; use the public contract to connect your coordinator.

## Four tabs, four main cards

The agents workspace uses **Overview / Agents / Tasks / Events**. The existing
four homelab cards remain a separate workspace; the workspace does not add a fifth
hardware card. Use the explicit **Agents / Nodes** control on the overview.
The original purple logo and approved letterforms remain unchanged, and tapping
the logo returns to the Nodes home screen. Both modes use **All systems / Clock / Pick system / Settings** in the same positions.
The picker selects a node or system and offers **Switch to Agents**. Selected
agent systems use **Overview / Agents / Tasks / Events** detail tabs.

- **Overview:** at most four system cards per page. Select a system for four
  metrics: recent/tracked agents, separate waiting/blocked counts, running/queued tasks, and operations/usage.
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
work. Unknown progress uses an unfilled dim bar, not a fake percentage.

## Run the reference source and design

Use Python 3.10+ and install the small reference dependency in your own virtual
environment. The optional Python service publishes the same contract as your own adapter:

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
AutoGen or other integrations are **not** bundled in the core. They need a
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
an instruction to run a command. The native workspace reads the contract through
the collector's bounded scheduler, preserving the four-card/four-tab constraint.
Hardware alerts retain their existing deduplicated notification policy; AI
execution and approval actions are outside this monitoring interface.

## Verify

```sh
.venv-agent-ops/bin/python tools/agent-operations/test_api.py
.venv-agent-ops/bin/python tools/agent-operations/check_ui.py
.venv-agent-ops/bin/python tools/agent-operations/check_edges.py
```

API checks cover schema, absent metrics, invalid records, ownership races,
privacy, source immutability, mappings, payload size and denied writes. UI checks
use the actual LVGL/WebAssembly renderer and continuous pointer input: landscape/portrait, themes,
Back routes, unknown progress, live/sample separation, source expiry and four
cards/tabs. Start the reference server first. UI checks use the installed
Playwright CLI wrapper when available, otherwise the public npm Playwright CLI.
Node.js/npm and Playwright browser dependencies are needed for browser QA; no
Codex account is required. `AGENT_OPS_PLAYWRIGHT_CLI` can select another CLI command.
Live shared-hub checks
run when the server was started with `--hub`; generic sample and publisher checks
do not require that NAS channel.

## Production transport and verification

See [AGENT-INTEGRATION.md](AGENT-INTEGRATION.md) for collector configuration, authentication and source failure behavior. `firmware/src/operations_ui.inc` implements the native workspace, `operations_parse.h` bounds the decoder, and `collector-rs/src/operations.rs` validates and polls the source. `node tools/web-emulator/test-operations.mjs` exercises the actual compiled UI, animated overview workspace switch, logo Home behavior, detail/back navigation, swipe guard, unknown progress, scoped IDs, heartbeat expiry and source failure. Public builds contain no Wi-Fi keys or private source data.

## Optional private conversation review

The local reference server accepts `--private-transcripts` only when bound to
loopback. It adds a **Private conversations** link to the local navigation.
This hands the browser to a separately installed, authenticated private chat
reader using a single-use, short-lived launch ticket. The reader supports
formatted user/assistant messages, code blocks and cursor-based history pages.
The archive collector and chat reader are separate components; they are not
bundled in the Glimdock Linux release.

The handoff is disabled by default and never appears on the public website.
It does not add transcript text, prompts, reader credentials or authenticated
URLs to the operations snapshot, ESP32, public demo or telemetry. A reader must
be installed locally with its restricted credential before enabling the link.
The browser requests the handoff only from the exact local origin; foreign
origins, alternate request targets and LAN binds are rejected.

Agents, tasks and events appear newest first (smallest age), using heartbeat,
last update and event timestamp respectively. Missing or future timestamps
appear last; sorting does not modify source records or their stable IDs.

Clock mode follows the selected workspace. Nodes shows hardware health, average
CPU/RAM and node shortcuts; Agents shows source freshness, recent agents, running
tasks and up to four agent-system shortcuts. Expired agent metrics stay unknown.

The physical ESP32 now has a separate optional **Settings → Private conversations**
reader. It pairs with a dedicated authenticated HTTPS relay over USB; the public
WASM demo accepts fictional pages only. See the [private-reader setup](../docs/private-conversations.md)
for the runtime interface and privacy/storage boundaries.
