# Claude Code adapter

`unisphere_adapter_claude::ClaudeCodeAdapter` is a stateless `Default` implementation
of the core `SessionAdapter` port. Its only inputs are the supplied `SessionRef`,
`&[NativeRecord]`, and `MappingOptions`. It does not list/read files, discover HOME,
open sidecars, consult a clock, dereference attachments, or write/export anything.
A caller can select it independently of an injected `SessionLoader` and writer.

The [telemetry profile](telemetry-profile.md) is the **normative key/type registry**.
This page describes native extraction, not a second registry or native-format
fidelity guarantee. The supported dialect is demonstrated by synthetic
`claude-basic.jsonl` and `claude-parts.jsonl` fixtures in `unisphere-testkit`.

## Query observations

`ClaudeCodeAdapter` also implements the pure core `QueryAdapter` port for supplied
JSONL records. Query inspection never performs discovery or I/O and returns the
caller-supplied source revision on every observation. The versioned
`claude-code/query-v1` rule creates a conversation partition only from an explicit
`sessionId`; an optional `agentId` remains a separate participant key so reused
session IDs do not collapse subagents. Records without a native session ID remain
in a source-only partition rather than receiving an invented conversation ID.
Explicit `cwd` values become source-qualified `NativeCwd` association observations;
scope matching remains the SDK's responsibility.

User text is an initiating request only when the native user message contains a
text part. A user record containing only `tool_result` parts is a tool response,
`isMeta: true` is injected context, and `isCompactSummary: true` is a summary.
These markers are evidence for later SDK reconstruction; the adapter does not
construct logical turns. `tool_use.id` and `tool_result.tool_use_id` are retained
as exact scoped call identities. Original tool names remain unchanged while known
names receive a separate family such as `shell`, `file-read`, or `file-write`.
Missing call IDs stay unavailable and are never derived from adjacency or text.

Native message usage is emitted as an invocation-scoped observation with only
independently valid counters. Native timestamps retain a native clock basis.
Message text, reasoning, tool inputs, and tool outputs are represented as
`SensitiveOmitted` unless the supplied `ContentAccess` authorizes that exact field
or explicit emission. Unsupported parts remain typed `NotSupported` markers even
with content consent; inspection never dumps opaque payloads.

## Physical records, not reconstructed operations

Every valid JSON physical record produces one source-derived
`unisphere.session.record` event in supplied order, including unknown record
kinds and non-object JSON. No records are deduplicated by native message ID and
no conversation history, response choices, inference operation, trace/span IDs,
provider identity, or complete-session claim is synthesized. Equal inputs/options
produce equal records and diagnostics; splitting a batch does not change mapping.

Source path and physical byte offset come exclusively from the supplied contract.
The source must be an absolute UTF-8 path; existence is neither required nor
checked. Native `type` is preserved as the source kind when it is a string;
missing/non-string kinds use `unknown` and an `InvalidField` diagnostic. Only
`user` and `assistant` kinds are interpreted as messages. Unknown kinds retain
provenance and `UnsupportedRecord`; their message/payload contents are not mapped,
even with content opt-in. A valid non-object JSON record has unknown provenance
kind and structural diagnostics, not a guessed message.

### Metadata extraction

- `uuid` is the physical native record ID; `parentUuid` is its native parent link.
  Neither is conflated with `message.id`, the logical message ID.
- The declared `sessionId` becomes the profile's conversation ID. `isSidechain`
  is retained only as a boolean source flag. It does not establish a different
  child/parent session or override the declared session ID.
- `message.id` and `message.model` are retained as message identity and observed
  response model, respectively. The adapter never infers a provider from a model.
- `message.role` must explicitly be `user` or `assistant`. It is retained as the
  message role, not guessed from the outer kind. Missing/invalid role produces
  `InvalidField`; a content body is not emitted without a valid role.
- Optional ID/model strings that are missing or null remain absent. Other types
  produce `InvalidField` and are omitted. Missing `isSidechain` stays absent;
  a supplied non-boolean, including null, is invalid. No default false is invented.
- A missing/non-object `message` in a supported kind is diagnosed, while the
  physical record and any valid provenance remain available.

### Timestamp and usage

A supplied `timestamp` must parse as RFC3339. Its timezone offset is respected and
its nanoseconds are converted to a nonnegative Unix value fitting `u64`. Missing
means no timestamp. Invalid syntax/type, null, pre-epoch values, and overflow are
omitted with `InvalidTimestamp`. There is no current-time fallback.

Only these exact `message.usage` components are retained, independently:
`input_tokens`, `output_tokens`, `cache_read_input_tokens`, and
`cache_creation_input_tokens`. Each must be a JSON integer in `0..=i64::MAX`.
A retained zero is an observed zero, never a substitute for an absent field.
Negative, fractional, string, null, boolean, or oversized components are omitted
with `InvalidField`; a supplied non-object usage value is also invalid.

The profile's usage scope is `native_record_snapshot` only when at least one
component survives. Unknown/nested usage components are ignored. These are not
standard GenAI usage totals: cache inclusion/overlap is not proven. The adapter
never adds cache components, sums repeated records, or emits `gen_ai.usage.*`.

## Content policy and parts

`MappingOptions::default()` is **metadata-only**. When a supported message has a
`content` field, the adapter emits `ContentOmitted` and the profile's content
omitted flag, with no body. This includes empty or structurally invalid supplied
content; missing content instead receives `InvalidField`. Part validation still
runs in metadata-only mode, so unsupported/invalid parts are observable without
exposing their data. This policy excludes bodies, tool arguments/results,
reasoning, and unknown payloads; it does **not** anonymize source paths, native
IDs, session IDs, model names, or native kind strings.

With `MappingOptions { include_content: true }`, a valid message emits a structured
`{role, parts}` body. Parts retain native order; malformed parts are omitted with
`InvalidField`, without deleting valid siblings. An explicitly empty content
array remains an empty parts array. Missing/invalid content does not become an
invented empty body.

| Native content | Opt-in part extraction |
|---|---|
| String `message.content` | One `text` part with the string as `content` |
| `text` part with string `text` | `text` part with that value as `content` |
| `thinking` with string `thinking` | `reasoning` part with that value as `content`; signatures are not promoted |
| `tool_use` with string `name` and present `input` | `tool_call` with `name` and structured `arguments`; string `id` retained when present |
| `tool_result` with present `content` | `tool_call_response` with structured `response`; string `tool_use_id` becomes optional `id` |
| Unknown part kind, including `image`, `document`, or `redacted_thinking` | Marker `{type: "unisphere.unknown", native_type: <native kind>}` and `UnsupportedPart`; **no raw unknown payload** |

Tool inputs and results preserve their supplied JSON structure, including arrays,
objects, scalars, and explicit null; absence is not replaced with `{}` or null.
Optional null tool IDs are absent, and other non-string IDs are diagnosed and
omitted. A supplied boolean `tool_result.is_error` is retained as the namespaced
part property `unisphere.is_error`; an invalid supplied value is diagnosed and
omitted, not coerced to a boolean. A malformed part object/type or missing required
text/name/input/result value is omitted with `InvalidField`.

**Omitted, redacted, unsupported are not claims of successful collection.** Policy
omission is signalled by `ContentOmitted` and the omitted flag. A native redaction
is identifiable in opt-in output by `native_type: "redacted_thinking"`, distinct
from another unsupported kind; its encoded/redacted bytes are never exported.
There is no redaction decoder. In metadata-only mode both native redactions and
other unsupported parts yield the shared `UnsupportedPart` diagnostic without a
body/type marker. Unknown kinds, attachments, and extra native fields are not
claimed lossless. Tool-result JSON is retained only as a value; attachment/spill
references inside it are never opened. Outer `toolUseResult` sidecar metadata is
not interpreted or exported.

## Failure and provenance boundary

Malformed JSON or invalid UTF-8 fails the entire supplied batch as `UNI-DATA`
(`PipelineErrorKind::InvalidData`) at the physical record offset. No partially
mapped batch is returned; the caller must not accept a checkpoint from failure.
Public errors use core-owned fixed code/message/fix text, never parser messages
or native bytes. Valid unsupported data produces the records/diagnostics described
above rather than panicking or silently disappearing.

Diagnostics contain only offset and a closed code. They are emitted in stable
field/part traversal order and may repeat for multiple issues at the same offset;
there is no fabricated field path or raw diagnostic payload. This adapter does
not enforce loader byte budgets or the writer's 32 MiB **encoded OTLP** batch cap.
It maps exactly the records supplied to it; the loader owns physical framing and
partial-tail/cursor handling, and the writer owns output representation/bounds.

## Proof surface

`cargo test -p unisphere-adapter-claude` runs the common adapter conformance helper
on both frozen native fixtures, plus explicit tests for multipart structure,
privacy, repeated IDs/snapshots, missing/malformed fields, unknown/redacted parts,
timestamp/usage boundaries, safe parse failures, source validation and replay.
All inputs are synthetic and in-memory. These tests prove mapping behavior, not
live private-store compatibility, filesystem containment, SDK orchestration,
installed CLI behavior, or actual OTLP encoding; those have separate proof lanes.
