# Copilot CLI adapters

## Current event JSONL

`unisphere-adapter-copilot-cli::CopilotCliAdapter` implements the pure
`SessionAdapter` port. `DESCRIPTOR.id` and `name()` are `copilot-cli`.
The caller supplies `NativeRecord` bytes and a `SessionRef`; this crate does not
open files, discover sessions, read environment variables, observe a clock,
load attachments, consult a database, or emit output.

Usual source: `~/.copilot/session-state/<session>/events.jsonl`. This is a symbolic
location hint, not a discovery operation or installation claim. The current
shared JSONL loader supports Unix; supplied-data mapping is platform independent.
`session-store.db` is a derived index, not an authoritative event source.

The implementation follows the local structural research's
`type/id/parentId/timestamp/data` envelope and the public
[Copilot SDK session event declarations](https://github.com/github/copilot-sdk/blob/main/nodejs/src/generated/session-events.ts)
inspected on 2026-09-08. Synthetic fixtures encode those shapes; they are not
copies of private sessions or evidence that every Copilot release has that schema.
The mutable upstream schema is supporting evidence, not a pinned compatibility guarantee.

### Physical identity and time

Each valid supplied JSON record produces one `unisphere.session.record`, in input
order, including unknown event kinds. Required attributes are numeric
`unisphere.profile.version=1`, `unisphere.source.adapter=copilot-cli`, the supplied
source path and physical byte offset, and the actual native `type` in
`unisphere.source.kind`. Missing/invalid type is `unknown` with `InvalidField`.
Native `id` and `parentId` become `unisphere.source.record.id` and
`unisphere.source.parent.id`. Null parent is absent, not an inferred root ID.

Only the event's RFC3339 `timestamp` supplies `timestamp_unix_nano`. Offset zones
and nanoseconds are preserved. Invalid, pre-epoch and unrepresentable timestamps
are absent with `InvalidTimestamp`. Missing time stays absent: session start time,
resume time and a prior event never substitute for the event clock.

Mapping is stateless and record-local. Repeated IDs, tool requests/executions,
deltas/final messages, compactions, resumes and repeated usage observations stay
separate. Splitting or replaying batches does not change records or diagnostics.
No deduplication, turn reconstruction, session finality, trace/span IDs, or persisted
CLI resume is claimed. The cursor is only safe under the loader's append-only
source assumption; rewinds, in-place history edits and deleted records are not
reconciled by this mapper.

### Supported event projection

| Native family | Projection |
| --- | --- |
| `session.start`, `session.resume` | Explicit session ID when supplied; version/producer, selected model and native start/resume strings; opt-in working-directory context. |
| `session.context_changed`, `session.model_change` | Context event, pending-git flag and selected/previous model. Not an assistant response or ordinary message. |
| `user.message`, `assistant.message`, `system.message` | Typed message fragments; text, transformed user text, readable reasoning, tool requests and supported attachment references under content opt-in. System role must explicitly be `system` or `developer`. |
| `assistant.reasoning`, `assistant.reasoning_delta`, `assistant.message_delta` | Independent reasoning/text fragments. Deltas retain `unisphere.copilot.fragment=true`, not assembled answers. |
| `assistant.turn_start`, `assistant.turn_end`, `assistant.message_start` | Native turn/message/model identities, no invented content or role. |
| `tool.execution_start`, `tool.execution_complete` | Tool-call and tool-response fragments with supplied IDs, structured arguments/results, success/error state and text result blocks. |
| `tool.execution_partial_result`, `tool.execution_progress`, `assistant.tool_call_delta` | Explicit native fragment body with partial output, progress or raw input delta; never parsed/accumulated tool arguments. |
| `assistant.usage`, `session.usage_checkpoint`, `session.shutdown`, `session.usage_info` | Independent measured usage/accounting and context-window observations, detailed below. |
| `session.compaction_start`, `session.compaction_complete` | Administrative record; native compaction counters, separate compaction-call usage and opt-in summary/instructions/checkpoint reference. No generated assistant turn. |
| `subagent.started/configured/completed/failed/selected/deselected` | Native agent/parent/tool-call/model identities and reported aggregate tokens when present; descriptions/display names/errors only in opt-in native bodies. |
| `session.error/info/warning/title_changed`, `assistant.intent`, `abort` | Opt-in allowlisted message/title/intent/reason text in native administrative bodies. |
| `session.idle`, `assistant.idle` | Provenance only. |
| Other kinds | Provenance and `UnsupportedRecord`; unknown native payload is never dumped, even with content enabled. |

`gen_ai.conversation.id` is populated only from explicit `data.sessionId`, not a
path, event ID or previous session header. `gen_ai.response.model` is populated
only for native assistant-message/API-usage observations. Selected, turn, tool and
subagent models remain explicit `unisphere.copilot.*` extensions. Record-level
`agentId` is `unisphere.copilot.agent.id`; subagent `data.parentId` is
`unisphere.copilot.agent.parent_id`, distinct from event-chain `parentId`.

Other retained IDs use `unisphere.message.id` and `unisphere.copilot.turn.id`,
`tool_call.id`, `parent_tool_call.id`, `interaction.id`, `api_call.id`,
`provider_call.id`, and `reasoning.id`. These are correlations, not OTel spans.
`ephemeral` records supplied by an SDK caller retain that boolean; their support
does not imply those normally transient records exist in on-disk JSONL.

### Usage is not additive across records

Native `inputTokens`, `outputTokens`, `cacheReadTokens` and `cacheWriteTokens`
map to the common `unisphere.usage.input_tokens`, `output_tokens`,
`cache_read_input_tokens` and `cache_creation_input_tokens`. Additional native
components are `unisphere.copilot.usage.reasoning_tokens`,
`accepted_prediction_tokens` and `rejected_prediction_tokens`.

`unisphere.usage.scope` distinguishes `assistant_message`, `api_call`,
`session_checkpoint`, `session_shutdown`, `compaction_api_call`, and
`subagent_reported_total`. Shutdown `modelMetrics` and `agentMetrics` remain
separate structured `unisphere.copilot.model_metrics` and `agent_metrics`
attributes: exact native model/agent keys, allowlisted usage components,
request count/cost and nano-AIU totals. Agent display names are excluded because
they can be delegated prompts. Model totals and per-agent totals overlap;
consumers must not sum them, or add shutdown/checkpoint totals to individual calls.

Native nano-AIU and premium-request counters, cost multipliers and API duration
are retained under `unisphere.copilot.usage.*`. Nano-AIU is not converted into
currency; fractional premium-request multipliers are not rounded. Integer token
counts must be nonnegative and fit `i64`; null, strings, negatives, fractional
tokens and out-of-range integers produce `InvalidField`, not zero. Durations,
cost and premium-request values accept finite nonnegative numbers.

Context-window counters are `unisphere.copilot.context.current_tokens`,
`conversation_tokens`, `system_tokens`, `tool_definition_tokens`, `token_limit`
and `message_count`, not inference usage. No `gen_ai.usage.*` totals are inferred.
Missing counters stay absent. A scope label identifies an observation even when
all optional counters are missing/invalid; it never proves a total is present.

### Content, omissions and errors

Default metadata-only mapping always has `body=None`. Text, transformed prompts,
reasoning, tool arguments/results, descriptions, context paths and attachment
references do not escape into attributes. `ContentOmitted` and
`unisphere.content.omitted=true` mark recognized omitted content. Structural
validation and unsupported-part diagnostics still apply without content opt-in.
Metadata-only is not anonymization: source paths, native IDs, configured agent
names and model names remain potentially sensitive metadata.

With opt-in, ordinary fragments use the common `role`/`parts` structure described
in [the telemetry profile](telemetry-profile.md). Parts include `text`,
`reasoning`, `tool_call`, `tool_call_response`, and explicit
`unisphere.transformed_text`/`unisphere.attachment_reference` extensions.
Tool argument and `result.structuredContent` JSON remain typed; no stringification
or speculative parsing. Failure text lives only in the opt-in response body.
Administrative records instead use `{type:<native kind>,data:{...}}`, not roles.

Supported file/directory/selection attachments retain supplied text/reference
fields, never dereference paths or asset IDs. Selection position ranges and file
line ranges are not projected. GitHub reference variants and non-text tool
content blocks currently produce `UnsupportedPart` and, with opt-in, an opaque
`unisphere.unknown` marker containing only the native type. Binary data, encrypted
reasoning, provider-native reasoning blocks, citations, server-tool internals,
UI resources, token-detail billing breakdowns and prompt-cache restoration state
are not projected. Known such fields are diagnosed; unknown optional fields are
not retained. This is not lossless archival or complete native-state replay.

Malformed JSON/UTF-8 fails the entire supplied batch with `InvalidData` and the
actual physical offset. Valid but unsupported/malformed structures retain their
provenance and typed `MappingDiagnosticCode` values. Diagnostics and errors contain
no parser text, source payloads, arbitrary error strings or new paths.

## Validation ownership

The worker authored synthetic behavioral regressions and shared-adapter
conformance coverage. No worker build, test, formatter, linter, boot or lockfile
update was executed; the PM owns coordinated validation and registration.
After the PM resolves workspace/lockfile integration:

```sh
cargo test --locked -p unisphere-adapter-copilot-cli
```

The production composition must separately register each descriptor with its
matching runner, then exercise actual SDK/CLI export. Unit tests alone do not
prove filesystem collection, supported installation formats, registry wiring,
output bounds, checkpoint delivery or snapshot replacement semantics.
