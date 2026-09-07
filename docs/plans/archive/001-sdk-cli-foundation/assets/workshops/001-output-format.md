# Workshop: Session/Event Output Format

**Type**: Storage Design / Data Model
**Plan**: 001-sdk-cli-foundation
**Spec / intent source**: [Requirements spine](../../requirements-spine.md), especially RQ-002–003, RQ-013–017 and Q-002, Q-004–006
**Created**: 2026-09-07
**Status**: Review — operator selected a standard-first direction; detailed profile remains open
**Target Proof Level**: Preferred Direction
**Current Proof Level**: Preferred Direction

**Value Thesis**: Separate native-session semantics from telemetry transport and first-consumer policy so the operator can choose a durable direction without accidentally approving a schema or expanding the foundation plan.

**Selected Value Axes**:
- **Safety to Change**: Make native-format evolution, Development-stage conventions and independent projection versions visible before they become public compatibility promises.
- **Cross-Domain Coordination**: Keep reader observations, session semantics, telemetry export and Flowspace indexing at explicit boundaries.
- **Proof Quality**: Replace a vague “standard” or “lossless” claim with identifiable facts, unresolved fidelity requirements and concrete comparison scenarios.

**Related Documents**:
- [OpenTelemetry common-format research](../research/otel-common-format.md) — specification/source review and candidate formats; no chosen format.
- [Flowspace3 first-consumer interview](../requirements/flowspace3-interview.md) — questions and consumer-local boundaries; the reviewed artifact is awaiting answers, not an accepted consumer contract.

## Purpose and fresh entrant outcome

Clarify what should be canonical, what should be a projection, and what evidence an eventual format choice requires. A fresh reader should be able to compare the three candidates, distinguish official facts from this recommendation, and identify the operator decisions still needed.

**Direction selected: give standard OTLP JSONL and GenAI fields the first chance.** This is not a validated export, a frozen extension schema or permission to ship readers; add features when concrete telemetry demonstrates the need rather than designing them all upfront.

## Recommendation versus scope and decision

**Operator decision, 2026-09-07: start with option A, OTLP LogsData JSONL and existing GenAI fields.** Keep the Rust API typed and ergonomic without inventing a separate persisted encoding. Add the smallest documented extension only when a real source field or required behavior is not represented adequately; preserve the already-requested pij metadata boundary.

This supersedes the earlier provisional recommendation for B. B remains a fallback only if concrete fidelity, compatibility or consumer evidence justifies its additional model/mapping cost; C waits for an actual multi-signal requirement. No speculative extension framework or alternate encoder is required before useful work starts.

| Boundary | Meaning here | Not implied |
|---|---|---|
| Plan001 foundation | SDK/CLI, configuration, diagnostics, architecture, packaging and tests | Shipping native readers, a frozen telemetry model or its persistence implementation |
| Plan001 CLI response envelope | How foundation commands report their operation result and diagnostics | A session/event record format; eventual exported JSONL need not be wrapped in a command-response object |
| Scratch native readers | Independently authorized experiments under `scratch/native-readers`, preserving raw JSON observations with optional native IDs/role/timestamp and source byte offset | A chosen normalized schema, Claude message grouping, approved fidelity guarantees or code imported by shipping crates |
| Eventual common model | Session semantics shared by consumers | A central server, database, daemon, Postgres schema or required physical layout |
| OTLP projection | A documented mapping to a standard telemetry encoding | Native-store reconstruction or lossless export by default |
| Flowspace3 projection | First-consumer indexing and presentation | Universal truncation, model dropping, thinking/tool-content dropping, PostgreSQL storage or turn-partition semantics |

The operator selected the standard-first direction; Main records subsequent decisions. Scratch observations and source-driven gaps inform profile refinements but do not silently change the canonical contract or expand Plan001 scope.

## Key questions

1. Is the authoritative artifact a source-derived session journal, a telemetry stream, or separately declared surfaces with different fidelity guarantees?
2. Which fields are already standardized, and can an OTLP log semantic profile cover the genuine native-reader gaps without a separate canonical encoding?
3. Which facts must survive native ingestion, canonical representation and export independently, and what counts as “preserved” at each boundary?
4. Which versions and extension rules protect consumers from native-format and GenAI/file-spec evolution?
5. How can Flowspace prove replay/change-detection usability without imposing its search policies on other consumers?

## Decision space

“Canonical” means the authoritative common representation, not necessarily a particular storage engine. **Standard-aligned model, typed Rust API and on-disk encoding are three distinct choices.** A typed API over standard fields can serve A, B or C; it is not evidence in favor of bespoke persistence.

| Option | Shape | Strengths | Costs and unresolved risks | Position |
|---|---|---|---|---|
| **A — Canonical OTLP LogsData JSONL + semantic profile** | A log-signal journal using OTLP bodies/attributes and standard GenAI fields, with minimal documented extensions when needed | One canonical/export representation; standard file framing; existing telemetry tooling can parse the envelope | Native identity, updates, completeness and pij context still need evidence-backed mappings; generic tooling may not understand custom semantics | **SELECTED AS STARTING DIRECTION.** Prove against real telemetry and add only demonstrated gaps |
| **B — Standard-aligned typed session/event model + OTLP JSONL projection** | A distinct canonical encoding plus an OTLP mapping, retaining standard GenAI meanings | Can isolate native reconstruction needs from transport if concrete evidence requires it | Another representation, mapping and compatibility surface to maintain | **NOT SELECTED.** Reconsider only when A demonstrably waters down required facts or behavior |
| **C — Standard signal files + session manifest/index** | Separate OTLP logs/traces/metrics with cross-file session metadata | Useful for an actual multi-signal delivery requirement | Additional publication, recovery and association contracts | **DEFERRED BY NEED.** Do not build a bundle/manifest without a concrete requirement |

B can later export a C-style bundle; that does not require making the bundle canonical now. A must keep traces/metrics out of its log-signal file. None of these options inherently defines message grouping, a turn, a cursor, exactly-once processing or native branch history.

## Evidence ledger

E1–E9 carry forward the existing [research findings OF-001–008](../research/otel-common-format.md#verified-findings), which records Main's primary-source review. To answer the operator's field-level question, the current GenAI event, input/output message schemas and OTLP protobuf definitions were also read directly for E10–E11 and the crosswalk below. Links identify actual specifications, not a generated answer. No runtime or round-trip evidence is claimed; `main` links describe the reviewed material, not pinned release dependencies.

| ID | Evidence and exact source | What it supports | Status / limit |
|---|---|---|---|
| E1 | [OTLP specification](https://opentelemetry.io/docs/specs/otlp/) | OTLP network traces, logs and metrics are **Stable** | Ready — specification evidence; does not confer stability on file export or GenAI conventions |
| E2 | [OTLP File Exporter specification](https://github.com/open-telemetry/opentelemetry-specification/blob/main/specification/protocol/file-exporter.md), [telemetry data requirements](https://github.com/open-telemetry/opentelemetry-specification/blob/main/specification/protocol/file-exporter.md#telemetry-data-requirements) | File spec is **Development**; **UTF-8 JSONL**, preferred `.jsonl`; top-level `LogsData`, `TracesData` or `MetricsData`; **exactly one signal per file**; **no ordering guarantee** for data/timestamps | Ready — file framing is defined, native replay is not. No ordering guarantee does not mean ordering is forbidden |
| E3 | [OTLP JSON Protobuf Encoding](https://github.com/open-telemetry/opentelemetry-proto/blob/main/docs/specification.md#json-protobuf-encoding) | lowerCamelCase fields, integer enums, decimal strings for 64-bit integers, case-insensitive hex trace/span IDs; receivers must ignore unknown message fields | Ready — generic added top-level OTLP fields may be ignored; deriving ordinary JSON from Rust values does not establish OTLP compliance |
| E4 | [GenAI events](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-events.md), [agent spans](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-agent-spans.md) | GenAI conventions are **Development**; current opt-in `gen_ai.client.inference.operation.details` can carry input/output independently of traces; `gen_ai.conversation.id` supplies conversation correlation | Ready — useful vocabulary, not proof that every native record is an inference operation |
| E5 | [GenAI input message schema](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/model/gen-ai/gen-ai-input-messages.json), [GenAI events](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-events.md) | Message parts include text, reasoning, tool requests/responses, blobs/files/URIs, **compaction** and generic parts; `gen_ai.conversation.compacted` is a boolean attribute, set `true` when reliably known and otherwise left unset | Ready — standard compaction representation exists; it is not a generic rewind event |
| E6 | [GenAI content requirements](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-events.md), [OTLP known limitations](https://github.com/open-telemetry/opentelemetry-proto/blob/main/docs/specification.md#known-limitations) | Content is opt-in, sensitive and may be filtered/truncated; OTLP network acknowledgements do not provide end-to-end exactly-once guarantees | Ready — transport success is not full-content fidelity or deduplicated replay |
| E7 | [Collector file exporter](https://github.com/open-telemetry/opentelemetry-collector-contrib/tree/main/exporter/fileexporter), [OTLP JSON file receiver](https://github.com/open-telemetry/opentelemetry-collector-contrib/tree/main/receiver/otlpjsonfilereceiver) | Concrete tooling exists; reviewed components mark signals Alpha; exporter warns about field-name stability and defaults `append` to false | Ready — pin and exercise selected versions; no Collector compatibility result here |
| E8 | [Requirements spine](../../requirements-spine.md#product-research-context-to-preserve), [consumer interview boundary](../requirements/flowspace3-interview.md#context-and-boundary) | Native records/messages/requests/turns differ; Flowspace drops/truncates information for search; first-consumer policy must stay local | Ready — captured requirements/source-research context, not answered interview or complete native-format coverage |
| E9 | Three-candidate fixture and Collector comparison below | Whether the recommendation survives actual data, privacy and interoperability constraints | Missing — proposed evidence only; no comparison executed |
| E10 | [Current inference-operation event and attributes](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-events.md#event-gen_aiclientinferenceoperationdetails), [input schema](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/model/gen-ai/gen-ai-input-messages.json), [output schema](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/model/gen-ai/gen-ai-output-messages.json) | Exact standard keys, role/parts shapes, tool/compaction fields and token subset semantics in the crosswalk | Ready — direct primary-source read for this revision; Development, not a selected version |
| E11 | [OTLP logs protobuf](https://github.com/open-telemetry/opentelemetry-proto/blob/main/opentelemetry/proto/logs/v1/logs.proto), [common AnyValue protobuf](https://github.com/open-telemetry/opentelemetry-proto/blob/main/opentelemetry/proto/common/v1/common.proto) | Envelope structure, event category, timestamps, attributes and nested structured values; JSON spelling follows E3 | Ready — direct primary-source read; illustrative encoding is not a Collector conformance result |
| E12 | [GenAI agent spans: create/invoke agent identity](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-agent-spans.md#create-agent-span); operator instruction “need pij names and roles etc in there too” | `gen_ai.agent.id`/`gen_ai.agent.name` describe the GenAI agent; for hosted agents the ID should be the provider-assigned stable resource ID. The requested pij peer metadata needs a separate semantic comparison | Ready — direct primary-source read and explicit requested scope; no registry/session/transport data read, no pij schema approved |

A file line may contain a batch of telemetry records: one JSONL line is not necessarily one log record, conversation event or message. The reviewed specifications do not supply the complete native discover/read/checkpoint/revision/rewind/deduplication lifecycle. This is not a claim that OTLP cannot represent those facts through a profile.

## Which fields are standard? Exact starting crosswalk

**Yes: the envelope and much of the useful GenAI content already have standard definitions.** The following is the starting vocabulary for every candidate, not a newly approved required-field list. Use a field only when native evidence matches its documented meaning. Sources are E10–E11; GenAI fields remain Development.

| Native/common concept | Exact standard field or structure | Meaning and mapping boundary |
|---|---|---|
| File/log envelope | `resourceLogs[].scopeLogs[].logRecords[]`; optional `resource`, `scope` and their `schemaUrl` fields | Standard OTLP structure, independent of the Rust API; schema URLs are not native-reader cursor versions |
| Event category and time | LogRecord `eventName`, `timeUnixNano`, `observedTimeUnixNano`; optional `traceId`, `spanId` | Event category is not event-instance identity. Source occurrence time differs from collection time; do not derive trace IDs from native message IDs |
| Inference-operation details | `eventName = "gen_ai.client.inference.operation.details"`; `gen_ai.operation.name` and `gen_ai.provider.name` | Required operation/provider attributes for this opt-in event. Do not label an arbitrary native record as a completed inference operation; provider reflects the instrumented GenAI provider, not automatically Claude Code/OMP/Copilot as the harness |
| Conversation/session association | `gen_ai.conversation.id` | Use an available conversation identifier. The convention says not to invent a UUID, trace ID or content-hash fallback; collision-resistant native-store namespacing still needs a profile |
| Requested versus actual model | `gen_ai.request.model`, `gen_ai.response.model` | Preserve both when known; requested model and response model need not match. Do not drop model metadata because an index does not use it |
| Response identity and prior context | `gen_ai.response.id`, `gen_ai.request.previous_response.id` | Completion identity and provider-linked previous response, not universal native message/record identity. No generic `gen_ai.request.id` is defined in the reviewed inference-event attribute list; do not invent it and call it standard |
| Input/output messages | `gen_ai.input.messages`, `gen_ai.output.messages`; each message has `role` and `parts`, with optional `name` | Input is the history actually sent to the model, ordered as sent. Each output message is one model choice/candidate, not a physical line or a consumer turn. Both are opt-in and **must be structured on events**, not JSON-in-a-string |
| Roles and ordinary text | `role`: `system`, `user`, `assistant`, `tool` or another string; `parts[]`: `{"type":"text","content":"…"}` | These are existing schema fields; do not introduce competing role/content names merely to make Rust values typed |
| Tool request/result | Request part: `type = "tool_call"`, `name`, optional `id`, `arguments`; response part: `type = "tool_call_response"`, optional `id`, `response` | Reuse the tool-call identity to correlate when available; arguments/response can be structured. These IDs are not source-record IDs. The schemas also define `server_tool_call` and `server_tool_call_response` |
| Reasoning and compaction | Part `type = "reasoning"` with `content`; part `type = "compaction"` with optional `id`, `content`; attribute `gen_ai.conversation.compacted` | Reuse standard parts. The compaction attribute is true-or-unset, not false-for-no-evidence; neither it nor a compaction part specifies native rewind/revision history |
| Attachments/other parts | Part types `blob`, `file`, `uri`, with `content`, `file_id` or `uri` respectively, plus applicable `modality`, `mime_type`; generic part has `type` and additional properties | Existing extensibility, not an instruction to fetch an attachment or retain its sensitive content. Preserve native unknowns only under the selected privacy/fidelity policy |
| Usage totals | `gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens` | Inference usage, not automatically cumulative session usage. Input includes cached tokens; use provider totals/count semantics rather than adding overlapping counts |
| Cache/reasoning breakdown | `gen_ai.usage.cache_read.input_tokens`, `gen_ai.usage.cache_write.input_tokens`, `gen_ai.usage.reasoning.output_tokens` | Cache read/write counts are included in input totals; reasoning output is included in output totals. They are subsets, not extra totals to add again |
| Completion outcome | `gen_ai.response.finish_reasons`; conditionally `error.type` | Finish reasons correspond to returned generations, even if output messages were filtered. Output-message `finish_reason` is deprecated in the reviewed schema; prefer the event attribute |
| Instructions and tool definitions | `gen_ai.system_instructions`, `gen_ai.tool.definitions` | Existing opt-in structured fields. Separate system instructions apply when the provider receives them separately; instructions in chat history belong in input messages |

Structured attributes are encoded through OTLP `AnyValue` (`arrayValue`, `kvlistValue`, primitive wrappers); dotted `gen_ai.*` keys remain literal attribute keys. This is distinct from adding properties directly to `LogsData`. File-level lack of ordering guarantees does **not** relax the ordered-input-message requirement inside a GenAI event.

### Genuine gaps — profile work, not reasons to reinvent messages

| Gap | What standard fields do not settle | OPEN profile question |
|---|---|---|
| Native harness identity | `gen_ai.provider.name` identifies the GenAI provider as known to instrumentation; `scope` identifies instrumentation. Neither alone identifies a native storage dialect/version | Where to record harness/source-adapter identity and version without mislabelling the model provider? |
| Source provenance | Conversation/response/tool-call IDs do not identify a source file generation, byte range or raw record | Which source references/raw evidence survive, and under what privacy and byte-fidelity policy? |
| Checkpoints and resumption | OTLP timestamps, file framing and schema URLs do not define native parser state or restart after file replacement | Which cursor state is returned, who persists it, and what invalidates it? |
| Record/message identities and revisions | Standard messages describe content; the reviewed schemas do not define a universal native record ID, revision chain or idempotent update protocol | Which identities are native versus derived, and how do updates/snapshots deduplicate without conflating messages? |
| Rewind/branch/lineage | Previous-response linkage and compaction represent useful facts but do not define harness branch selection or historical visibility after rewind | How are active view, retained history and subagent lineage represented without consumer turn policies? |
| Completeness and loss | `droppedAttributesCount` counts dropped log attributes, not missing source records, truncated messages, unread sidecars or redacted content | How does each boundary distinguish incomplete source, unsupported data, deliberate policy removal and complete capture? |

These are gaps in the reviewed contracts, not claims that no OpenTelemetry convention could ever carry them. Check existing applicable conventions before naming any extension; no custom extension fields are approved here.

## Pij names, roles and lineage — standard versus extension

**Required topic coverage:** pij peer identity/name, orchestration role and parent/root lineage belong in this format discussion. Their inclusion does not select A/B/C, require a live registry during native parsing, or authorize reading private registry/transport data. Native observations remain independently useful when pij metadata is unavailable.

The `unisphere.pij.*` keys below are **proposed namespaced extension examples, NOT OpenTelemetry standards and NOT frozen final names or types**. They can be carried in supported OTLP attributes under any selected semantic profile; standard message fields remain the starting point.

| Concept | Existing standard meaning — do not conflate | Proposed extension / decision boundary |
|---|---|---|
| GenAI agent identity/name | `gen_ai.agent.id`, `gen_ai.agent.name` identify the GenAI agent in the applicable agent operation; hosted IDs should be provider-assigned stable agent-resource IDs (E12) | These are not automatically pij seat ID/name. Reuse only when evidence establishes the same entity and the convention applies; retain a separate pij identity when it is a different entity |
| Pij peer/seat ID | Neither native harness session ID, `gen_ai.conversation.id`, model/provider identity nor a hosted agent ID means “pij peer” | Candidate `unisphere.pij.peer.id`; preserve the actual available peer identity, not a generated name-based ID. Registry namespace/lifetime and seat-reuse behavior remain OPEN |
| Pij peer name | Standard message `name` names a message participant; GenAI agent name names that agent. Neither automatically means the current orchestration display name | Candidate `unisphere.pij.peer.name`; name is a label, not a deduplication key. Preserve renames as time-scoped metadata, not retrospective edits to all history |
| Orchestration role | Message `role = "assistant"` is conversational role; it does not mean prime, PM, coder or reviewer. Provider name and model name also say nothing about the orchestration assignment | Candidate `unisphere.pij.role`; values come from observed assignment evidence, not a frozen enum here. A peer can change roles; historical records require historical assignment evidence |
| Parent peer | A trace parent, tool-call ID, previous-response ID or native subagent parent may refer to a different graph | Candidate `unisphere.pij.parent.peer.id`; record the observed orchestration parent relation separately; do not infer it merely from process nesting or message adjacency |
| Root lineage | `gen_ai.conversation.id` and trace ID are not root-peer IDs; missing parent does not prove a root | Candidate `unisphere.pij.root.peer.id`; use explicit root evidence or a demonstrably complete, time-valid parent chain, with derivation provenance. Unknown ancestry stays unknown |
| Metadata source/evidence | LogRecord source/collection times do not say which registry fact supplied a role/name | Candidates `unisphere.pij.metadata.source`, `unisphere.pij.metadata.source_ref`, `unisphere.pij.metadata.join_basis`; distinguish native evidence, historical registry snapshot and derived lineage. References must be privacy-safe and identify the join evidence, not expose private registry paths |
| Metadata observation, join and validity time | `timeUnixNano` is event occurrence; `observedTimeUnixNano` is log collection. Neither proves when a role assignment was valid or when registry enrichment was applied | Candidates `unisphere.pij.metadata.observed_at`, `unisphere.pij.metadata.joined_at`, `unisphere.pij.metadata.effective_from`, `unisphere.pij.metadata.effective_until`; distinguish when source metadata was seen, when a join ran and the interval for which the fact is supported. Preserve source precision; unknown bounds remain unknown |

**Joining rules to carry into the future contract:**
- Join only on evidenced identity relationships, such as an explicitly recorded native-session-to-peer binding with the relevant source namespace/generation. Similar names, model/provider matches and current process ancestry are not sufficient.
- A current registry snapshot proves current metadata at its observation time, not yesterday's role or name. **Never stamp today's role on historical records without evidence.** If a historical binding or effective interval is unavailable, leave historical attribution unknown; any current enrichment must be visibly labelled as current rather than event-time truth.
- Missing metadata stays unknown, not `assistant`, “coder,” a fabricated peer, or a guessed root. Conflicting or ambiguous joins must remain distinguishable from absence; precedence/error representation is OPEN.
- Retain provenance and time per metadata fact when peer name, role and lineage have different evidence. The simple example below assumes one historical snapshot supports all included facts; it is not a universal flat-schema decision.
- Peer names, assignment metadata and topology can be sensitive. Apply the selected export/privacy policy to extensions as well as message content. Sanitization must not silently turn unrelated peers into the same identity.

## Identity and reconstruction: requirements to resolve before a schema

These distinctions are design constraints from the requirements spine; their encoding and precise guarantees remain **OPEN**. A reader should preserve evidence, not silently resolve ambiguity through a consumer's grouping policy.

| Distinction | Required question / failure to avoid |
|---|---|
| Physical record vs logical message | Several native records may contribute to one message, or one record may contain several parts. Do not make line number or log-batch index the universal message identity |
| Native identity vs canonical identity | Preserve native IDs and their source/session namespace where available; define missing-ID derivation, collisions and stability across rereads before promising stable canonical IDs |
| Event identity vs message revision | A correction to message `m7` is not necessarily a new message or a duplicate. Decide event identity, revision ordering, replacement/patch semantics and how an earlier revision remains addressable |
| Rewind/branch vs deletion | Distinguish source history from the currently active view. Decide how branch parentage, rewind targets, superseded records and late updates are represented; do not silently erase history or treat compaction as rewind |
| Cursor vs time/order | Source byte offset is useful provenance but not a globally durable identity. File replacement/truncation/rotation and parser state may affect resumption; timestamp alone is insufficient, including for OTLP files |
| Session vs request/turn/subagent | Define namespace and lineage separately from consumer turns. Preserve native request/tool-call links and parentage where available rather than infer a universal turn partition |
| Missing vs zero/empty/redacted | Preserve unknown roles, absent usage/model/time and timestamp precision distinctly from actual empty/zero values or policy removal; define usage scope so snapshots are not double-counted |

Replay equivalence must specify whether it means the same observations, the same logical message state, the same active branch, or byte-identical native reconstruction. None is promised by merely naming an event ID.

## Minimal illustration — non-binding, synthetic, standard-first

One fictional sanitized source record is shown at three boundaries. For this example only, the source explicitly identifies a model response for a `chat` operation, provider `openai`, and known conversation `s1`. A **hypothetical historical registry snapshot**, labelled `fixture-registry-1`, explicitly links this exact session/record to peer `peer-17`, name `example-reader`, role `coder`, parent/root `peer-1`; its assignment interval begins at `2026-09-07T09:00:00Z` and covers this record. Metadata was observed at `2026-09-07T10:00:00Z`. These are stipulated example facts: no such fixture file was created or read, and no join was executed. Native field names and extension names are illustrative; no Rust types, new message schema or profile version are defined.

**1. Native observation:** keep raw JSON and source location, including fields the extraction does not yet interpret.

```json
{"sourceByteOffset":120,"raw":{"kind":"model_response","operation":"chat","provider":"openai","sessionId":"s1","messageId":"m7","role":"assistant","text":"[sanitized]","vendorExtra":true}}
```

**2. Candidate canonical event, semantic view:** standard GenAI fields come first; optional, explicitly non-standard pij enrichment follows. This plain attribute map illustrates what a typed SDK might expose, not a proposed on-disk encoding. `assistant` remains the message role while `coder` is a separate historical orchestration role. The historical fixture's binding is the evidence for these pij facts; without it, omit them as unknown.

```json
{
  "eventName": "gen_ai.client.inference.operation.details",
  "attributes": {
    "gen_ai.operation.name": "chat",
    "gen_ai.provider.name": "openai",
    "gen_ai.conversation.id": "s1",
    "gen_ai.output.messages": [
      {"role": "assistant", "parts": [{"type": "text", "content": "[sanitized]"}]}
    ],
    "unisphere.pij.peer.id": "peer-17",
    "unisphere.pij.peer.name": "example-reader",
    "unisphere.pij.role": "coder",
    "unisphere.pij.parent.peer.id": "peer-1",
    "unisphere.pij.root.peer.id": "peer-1",
    "unisphere.pij.metadata.source": "historical_registry_fixture",
    "unisphere.pij.metadata.source_ref": "fixture-registry-1",
    "unisphere.pij.metadata.join_basis": "explicit_session_record_binding",
    "unisphere.pij.metadata.observed_at": "2026-09-07T10:00:00Z",
    "unisphere.pij.metadata.effective_from": "2026-09-07T09:00:00Z"
  }
}
```

**3. Illustrative OTLP LogsData JSONL envelope:** the same standard attributes plus proposed namespaced pij attributes, using supported OTLP `AnyValue` encoding. Standard envelope acceptance does **not** make the `unisphere.pij.*` semantics standard. The extension timestamp strings are illustrative only; their final encoding/precision policy is OPEN.

```json
{"resourceLogs":[{"scopeLogs":[{"logRecords":[{"eventName":"gen_ai.client.inference.operation.details","attributes":[{"key":"gen_ai.operation.name","value":{"stringValue":"chat"}},{"key":"gen_ai.provider.name","value":{"stringValue":"openai"}},{"key":"gen_ai.conversation.id","value":{"stringValue":"s1"}},{"key":"gen_ai.output.messages","value":{"arrayValue":{"values":[{"kvlistValue":{"values":[{"key":"role","value":{"stringValue":"assistant"}},{"key":"parts","value":{"arrayValue":{"values":[{"kvlistValue":{"values":[{"key":"type","value":{"stringValue":"text"}},{"key":"content","value":{"stringValue":"[sanitized]"}}]}}]}}}]}}]}}},{"key":"unisphere.pij.peer.id","value":{"stringValue":"peer-17"}},{"key":"unisphere.pij.peer.name","value":{"stringValue":"example-reader"}},{"key":"unisphere.pij.role","value":{"stringValue":"coder"}},{"key":"unisphere.pij.parent.peer.id","value":{"stringValue":"peer-1"}},{"key":"unisphere.pij.root.peer.id","value":{"stringValue":"peer-1"}},{"key":"unisphere.pij.metadata.source","value":{"stringValue":"historical_registry_fixture"}},{"key":"unisphere.pij.metadata.source_ref","value":{"stringValue":"fixture-registry-1"}},{"key":"unisphere.pij.metadata.join_basis","value":{"stringValue":"explicit_session_record_binding"}},{"key":"unisphere.pij.metadata.observed_at","value":{"stringValue":"2026-09-07T10:00:00Z"}},{"key":"unisphere.pij.metadata.effective_from","value":{"stringValue":"2026-09-07T09:00:00Z"}}]}]}]}]}
```

Neither event illustration preserves native `messageId`, source offset, `vendorExtra` or raw JSON. Those are visible gaps to resolve, not approved losses. Event timestamps/resource/scope metadata are omitted for illustration; a real collector must supply observation-time semantics rather than fabricate source occurrence time. The pij metadata times are not substitutes for event times; `joined_at` is not shown because no join was executed. This is not a full compliant-export or round-trip proof. Every eventual mapping must distinguish preserved, transformed, redacted, referenced, unsupported and omitted facts; an arbitrary native message with no inference-operation evidence must not be forced into this event.

## Extensions, versioning and privacy implications

| Concern | Preferred direction / constraint | Decision still OPEN |
|---|---|---|
| Native unknown fields and new record kinds | Retain raw observations in the scratch experiment; do not erase evidence just because extraction recognizes only some fields | Product retention: exact source bytes vs parsed JSON fidelity; opaque extension payloads vs raw sidecar/reference; supported limits and unknown-kind behavior |
| Canonical model evolution | Version the semantic model explicitly; distinguish it from native adapter version, OTLP mapping/profile version and selected GenAI/file-spec versions | Compatibility policy, version granularity, migrations, unsupported-version behavior and whether unknown fields must survive read/write round trips |
| OTLP extensions | Put profile data in supported body/attribute structures with documented names and types; arbitrary generic top-level fields can be ignored by OTLP receivers | Which profile facts a generic pipeline must preserve, what receivers actually retain, and whether a separate raw-evidence artifact is needed |
| GenAI reuse | Start with the exact standard fields/message parts above; expose their semantics through typed Rust values without defaulting to an independently invented message schema | Which genuine native gaps need a profile; how any distinct canonical encoding earns its mapping cost; handling of convention changes |
| Content/privacy | Treat raw JSON, paths, prompts, reasoning, tool data and attachments as potentially sensitive. Content entering memory, local retention and outward export are separate boundaries | Consent and default profiles; allowlist/redaction rules; identifiers/path treatment; retention/deletion, attachment access and diagnostics leakage |
| Fidelity reporting | Describe guarantees per boundary and expose intentional/unsupported losses rather than call the whole pipeline “lossless” | How consumers distinguish source absence, parser non-support, truncation, redaction and export omission without leaking sensitive content |

Gitignored scratch is not a privacy control. Experiments use sanitized fixtures/source references, not real user session stores. Counts/cursor inspection must not print raw content. Preserving observations for an authorized experiment does not authorize their eventual retention or export by the product.

### Incremental fidelity and adapter discipline

- Start with standard fields. For each observed reduction, record the native fixture/field, what the current mapping loses or weakens, why a consumer needs it, and the smallest proposed mapping/extension; keep this in the existing evidence ledger rather than inventing a second schema programme.
- Watch for erased tool linkage, merged message identities, lost unknown fields, zero substituted for unknown usage, invented timing precision and current pij roles stamped onto historical events. Unsupported meaning must be visible, not silently watered down.
- The shipping contract is one single-responsibility adapter per harness/storage dialect, returning the same common semantic types. Shared framing/cursor helpers, serialization/export and optional registry enrichment stay outside harness-specific implementations.
- New ordinary adapters should need only their isolated implementation, registration metadata and fixtures, with no changes to core or existing adapters. A genuinely new semantic requirement may need an explicit, reviewed profile change; easy registration is not a promise that every vendor format is simple.
- Every adapter must pass a shared common-output conformance suite plus its own real-fixture/boundary tests. The current scratch readers are raw-observation experiments, not proof that this final normalization contract is implemented.

## Future comparison tests — proposals, not executed evidence

Exercise A first with sanitized native fixtures and explicit expected facts. Use the scenarios below as relevant inputs expose risks, not an upfront implementation checklist. Build or compare B/C only if an observed limitation justifies them, using the same fixtures so a format change cannot hide lost meaning.

| Scenario | Concrete exercise | Evidence that changes the choice |
|---|---|---|
| Observation vs message/revision | Two records sharing a native message ID, a later correction, a record with multiple parts, and two distinct records with identical content; read once and resume in batches | Ledger of native records, logical messages and revisions; no accidental collapse or duplicate summarization identity; compare SDK consumer work required for each candidate |
| Rewind, branch and compaction | Branch from an earlier message, rewind the active path, then append a late update and a compaction part/indicator | Both source history and the declared active view remain explainable; compaction is not equated with deletion/rewind; unresolved native semantics are reported |
| Cursor/replay/order | Stop at an incomplete JSONL tail, append the remainder, replay a batch, replace/truncate the file, and supply out-of-order timestamps and multi-record OTLP lines | Cursor/parser-state and source-generation rules recover the declared scope without timestamp-only deduplication; no assumption that one file line is one event |
| Unknown data and future versions | Unknown native field/record kind, new message part, unknown canonical field, unsupported model/profile version; include duplicate JSON keys if exact-byte fidelity is under consideration | Explicit preservation/rejection/opaque-carry behavior; distinguish parsed-JSON fidelity from original-byte reconstruction; measure what each mapping loses |
| Standard interoperability | Pin a Collector file receiver/exporter and spec/convention versions; import/re-export log fixtures with custom attributes, structured content, large 64-bit values and optional correlation IDs | Field-by-field mapping report after round trip; required facts retained or losses named; generic envelope acceptance alone is insufficient |
| Standard-field semantics | Map known conversation/provider/model/response fields, structured input/output messages and tool IDs; supply missing conversation ID, a requested/actual-model mismatch, 100 total input tokens including 40 cache-read tokens, and 80 output tokens including 20 reasoning tokens | No fabricated conversation ID or request-ID field, no harness/provider conflation, no JSON-string event messages, no double-counted token subsets; identical standard semantics through A, B and C |
| Pij historical metadata and lineage | Use sanitized historical bindings and a current snapshot with a renamed peer/new role; include absent metadata, identical names for different peer IDs, conflicting bindings and a missing parent link | Preserve historical name/role with evidence time and source; do not stamp current metadata onto old events, guess a root, conflate IDs or require a live registry; compare extension retention through every candidate |
| Privacy boundaries | Seed sanitized marker strings in text, reasoning, paths, native extensions and tool payloads; exercise each proposed ingest/retain/export profile and inspect diagnostics | No marker crosses a boundary forbidden by that profile; count-only output alone does not prove content never entered the process |
| Multi-signal value and recovery | Export correlated logs/traces/metrics to separate files; interrupt publication before the manifest/checkpoint is complete and attempt consumption | Demonstrated consumer need for C, cross-file association, detectable partial bundles and a documented recovery rule; no mixed-signal file is presented as conforming |
| Independent consumers and mapping cost | Consume the same canonical facts in a Flowspace-like search projection and a second full-fidelity inspection use case; keep truncation, model selection and turns inside the search projection | General model survives both without policy leakage; compare adapter/projection complexity and observed export size before accepting B's extra representation |

No tests, validation commands, Collector runs or reader runs were performed for this workshop. It reaches **Preferred Direction** through an explicit recommendation, fair alternatives and source-backed constraints; it is neither Contract Ready, Implementation Ready nor Validated.

## Open decision register

| ID | Decision / question | Status and owner |
|---|---|---|
| WF-001 | Select the starting format direction | **RESOLVED — operator chose standard-first A; try OTLP JSONL/GenAI fields and add small features as real telemetry demonstrates need, not BDUF** |
| WF-002 | Define fidelity per ingestion/canonical/persistence/export boundary, including raw-byte vs parsed-JSON preservation, revisions, branches, rewinds, attachments and incomplete source data | **OPEN — operator intent needed; fixture comparison informs the contract** |
| WF-003 | Define model/profile/native-adapter/spec versioning and unknown-field/unsupported-version behavior | **OPEN — no version or migration policy selected** |
| WF-004 | Define consent, content-ingestion, raw retention, export/redaction and metadata-only boundaries | **OPEN — no privacy profile selected** |
| WF-005 | Define durable identities and cursor ownership/resume semantics separately from Flowspace transaction, scheduling, deduplication and turn policies | **OPEN — SDK/consumer responsibilities not frozen** |
| WF-006 | Determine whether first-class traces/metrics require a multi-file bundle or remain later derived projections | **OPEN — no manifest or storage layout selected** |
| WF-007 | Define the pij profile: peer ID/name, orchestration role, parent/root lineage, namespace/lifetime, historical joins, per-fact provenance and validity time, unknown/conflict behavior and privacy | **OPEN — inclusion requested; extension names, values, representation and join guarantees are not finalized** |

**Operator decision:** “yeah give standerd the first chance then as long as its cheap to add features then we can add htem as we need them rather than bduf. keep an eye out as we work aroudn what watering down we might see and want to include as we see output from various telemetry.”

The direction is recorded; detailed profile semantics remain Review/Preferred Direction until exercised. Later requests also require single-responsibility harness adapters, common output and low-friction adapter addition. No runtime fidelity, final schema or interoperability proof is implied by those requirements.
