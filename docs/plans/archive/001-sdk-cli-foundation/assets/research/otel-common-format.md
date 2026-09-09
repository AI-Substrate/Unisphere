# Research — OpenTelemetry common file format for agent telemetry

**Status:** Research findings and candidates only; no format decision, approved plan or implementation.
**Date:** 2026-09-07.
**Question:** Is there a common file format for agent telemetry in OpenTelemetry that Unisphere should adopt rather than invent?

## Answer

**Yes: OpenTelemetry has an official OTLP File Exporter serialization specification for telemetry files, currently Development.** It specifies UTF-8 JSON Lines containing OTLP JSON `LogsData`, `TracesData` or `MetricsData`, with exactly one signal type per file. This is more than an exporter convention.

**That specification is not a complete native agent-session archive/reconstruction contract.** The reviewed file/OTLP/GenAI specifications provide rich telemetry data and message schemas, but do not specify the whole discover/read/checkpoint/revision/rewind/deduplication lifecycle needed to normalize native harness stores. This is a bounded research conclusion, not a claim that OpenTelemetry cannot carry those facts or that no other standard exists.

## Verified findings

| ID | Finding | Primary evidence | Consequence for requirements |
|---|---|---|---|
| OF-001 | OTLP itself is Stable for traces, metrics and logs; the published spec page identifies version 1.11.0. | [OTLP specification](https://opentelemetry.io/docs/specs/otlp/) | Do not conflate mature telemetry encoding with the maturity of file export or GenAI conventions. |
| OF-002 | The dedicated File Exporter spec is Development and explicitly defines UTF-8 JSON Lines, newline separators and preferred `.jsonl` extension. | [File Exporter: status and JSON File serialization](https://github.com/open-telemetry/opentelemetry-specification/blob/main/specification/protocol/file-exporter.md) | OTLP JSONL is a concrete standards-backed candidate for persistence/export. |
| OF-003 | Supported top-level file objects are `TracesData`, `MetricsData` and `LogsData`; a file must contain exactly one signal type, and data/timestamps need not be ordered. | [File Exporter: streaming appending and telemetry data requirements](https://github.com/open-telemetry/opentelemetry-specification/blob/main/specification/protocol/file-exporter.md#telemetry-data-requirements) | A single mixed logs/traces/metrics stream would not conform to this file specification; timestamps cannot be the sole ingestion cursor. |
| OF-004 | OTLP JSON requires lowerCamelCase field names, integer enums and decimal-string encoding for 64-bit integers; trace/span IDs are case-insensitive hex, not base64. Receivers must ignore unknown message fields. | [OTLP JSON Protobuf Encoding](https://github.com/open-telemetry/opentelemetry-proto/blob/main/docs/specification.md#json-protobuf-encoding) | Ordinary derived Serde JSON is not automatically OTLP JSON; arbitrary new top-level fields are not a safe extension/preservation mechanism. |
| OF-005 | Collector `fileexporter` implements plain JSON as one JSON object per line and supports a paired OTLP JSON file receiver; both components mark traces/metrics/logs Alpha. The exporter warns that exact field names are not guaranteed stable and defaults `append` to false. | [File exporter](https://github.com/open-telemetry/opentelemetry-collector-contrib/tree/main/exporter/fileexporter), [OTLP JSON file receiver](https://github.com/open-telemetry/opentelemetry-collector-contrib/tree/main/receiver/otlpjsonfilereceiver) | Pin and verify a selected exporter/receiver version rather than assuming persistence safety, append behavior or compatibility from the component name. |
| OF-006 | Current GenAI events/agent conventions are Development. `gen_ai.client.inference.operation.details` is an opt-in event that can record input/output independently of traces; correlation includes `gen_ai.conversation.id`. | [GenAI events](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-events.md), [agent spans](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-agent-spans.md) | Reuse existing vocabulary where semantics match; select/pin a convention version before treating it as a stable SDK wire contract. |
| OF-007 | The GenAI message schema already covers roles and parts including text, reasoning, tool requests/responses, blobs/files/URIs, compaction and generic parts; `gen_ai.conversation.compacted` also exists. | [Input message schema](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/model/gen-ai/gen-ai-input-messages.json), [GenAI events](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-events.md) | Do not invent equivalents without comparing the existing model, and do not claim OTel lacks messages or compaction. |
| OF-008 | Input/output content is opt-in, potentially sensitive and may be filtered/truncated; OTLP's network acknowledgement model is not an end-to-end exactly-once guarantee. | [GenAI event content requirements](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-events.md), [OTLP protocol limitations](https://github.com/open-telemetry/opentelemetry-proto/blob/main/docs/specification.md#known-limitations) | Telemetry availability does not establish complete transcript fidelity; retention, privacy, source coverage and replay must be explicit consumer-visible contracts. |

A file line may contain a batch of log records/spans/metrics; one line is not necessarily one conversation event. Typed Rust SDK values and the on-disk/export serialization can differ without forcing the consumer to parse OTLP envelopes itself.

## Candidates to compare — none selected

| Candidate | What it reuses | What still needs a contract | Main tradeoff |
|---|---|---|---|
| OTLP `LogsData` JSONL as the canonical event journal, with a documented Unisphere/GenAI event profile | Official file encoding, log envelope, attributes and applicable GenAI vocabulary | Stable native identities, source provenance, updates/rewinds, cursor/version semantics, meaningful events and privacy | Direct ecosystem compatibility, but telemetry wrappers and semantic extension rules may burden the canonical model. |
| Typed Unisphere session/event model with an OTLP JSONL projection | Standard GenAI terms/parts where appropriate and standard export encoding | The native model and mapping, including explicit losses/unsupported fields | Cleaner session API and replay semantics, but two representations must be maintained and tested. |
| Multiple standard signal files plus a separate session manifest/index | File spec for logs/traces/metrics individually | Cross-file session association, checkpoint atomicity and reconstructed views | Strong signal separation, more coordination for readers than a single journal. |

Do not claim a proposed mapping is lossless or interoperable until fixture and collector round-trip evidence proves the selected scope. Do not choose SQLite, a daemon or any persistence engine merely because one candidate mentions them.

## Questions carried to the requirement spine

1. Is the canonical artifact a faithful source-derived session journal, a telemetry export, or both through explicitly different surfaces?
2. Which native update, branch, rewind, sidecar and incomplete-data semantics must remain recoverable?
3. Can one log-signal event stream represent the required canonical facts while traces/metrics remain derived views, or do consumers require a multi-file signal bundle?
4. Must the SDK expose OTel message parts directly, map to its own types, or offer both without leaking wire-specific details into application services?
5. What versioning policy protects consumers from Development-stage GenAI/file-spec evolution?
6. Which fidelity/privacy/retention guarantees apply to ingestion, in-memory representation, persistence and export independently?
7. How will Flowspace3's first integration prove identity and change-detection guarantees without forcing its indexing/turn schema onto every consumer?

## Research method and corrections

A research agent attempted the Perplexity research MCP; the call timed out at the MCP boundary. A reduced-context Perplexity ask succeeded and identified the File Exporter specification and distinction from session replay; its returned answer and citations are preserved in [perplexity-fallback.md](perplexity-fallback.md). Main then ran the direct long-timeout `sonar-deep-research` route successfully: request `ec60d860-0ea7-4895-9899-3a71ffa9646c`, 45 reported search queries and 18 citations. Metadata/citations are preserved in [perplexity-deep-research.json](perplexity-deep-research.json).

The async tool capture truncated the single JSON answer-content line, so the saved deep-research evidence is explicitly metadata/citations, not a complete answer transcript. No full-answer capture or review is claimed. The findings table above rests on Main's independently retrieved primary specifications, not on unrecoverable generated prose; a future research runner should save raw responses before emitting a short tool receipt.

Primary-source adjudication corrected overbroad research claims: trace/span hex is case-insensitive, not lowercase-only; current GenAI events differ from legacy prompt/completion event names; compaction parts do exist; no ordering guarantee does not mean records must be unordered; and exporter behavior is not universally append-only. The claim retained is that the reviewed specifications do not provide a complete native-session reconstruction/replay contract, not that OTel explicitly forbids or cannot represent it.

**Proof boundary:** Specification and source review; no Collector runtime, Rust implementation, exporter round trip or production telemetry replay has been executed. The eventual format decision remains open.
