# Telemetry profile v1

This is the common wire/provenance registry plus the Claude-specific source-record
profile. Other adapters own their documented native extension fields.
Adapters assign semantics; `unisphere-output-otlp::OtlpJsonlWriter` preserves
supplied records and owns only OTLP encoding. It neither validates an adapter's
profile nor reads sources, opens destinations, observes a clock, or sends data.

## Standards and interpretation

The wire schema is pinned to **opentelemetry-proto
`bb8796bff67cf6e1c7f218e21de6eaec0841871e`**:
[LogsData and LogRecord](https://github.com/open-telemetry/opentelemetry-proto/blob/bb8796bff67cf6e1c7f218e21de6eaec0841871e/opentelemetry/proto/logs/v1/logs.proto)
and [AnyValue](https://github.com/open-telemetry/opentelemetry-proto/blob/bb8796bff67cf6e1c7f218e21de6eaec0841871e/opentelemetry/proto/common/v1/common.proto).
`LogRecord.event_name` is field 12, present since proto v1.5.0. JSON uses the
[OTLP JSON rules](https://opentelemetry.io/docs/specs/otlp/#json-protobuf-encoding),
including lowerCamelCase field names and decimal strings for 64-bit integers.

Message parts reference **semantic-conventions-genai
`94f432d7126f5884d30a2cdde6f4e89908ebb6fd`**,
[gen-ai-input-messages.json](https://github.com/open-telemetry/semantic-conventions-genai/blob/94f432d7126f5884d30a2cdde6f4e89908ebb6fd/model/gen-ai/gen-ai-input-messages.json).
This is a pinned shape reference, not a claim of complete GenAI instrumentation.
Standard attributes are used only where their meanings fit:
`gen_ai.conversation.id` identifies a declared conversation and
`gen_ai.response.model` names an observed response model. See the
[GenAI attribute registry](https://opentelemetry.io/docs/specs/semconv/registry/attributes/gen-ai/).

Every mapped physical record has event name **`unisphere.session.record`**. This
is a Unisphere extension event representing a source-derived record fragment,
**not** `gen_ai.client.inference.operation.details`, a completed inference, a
span, a reconstructed conversation, or an output choice. Repeated message IDs
remain separate physical records. The event name identifies a category, not an
instance. JSONL source path and byte offset preserve physical provenance; native
snapshot keys and content revisions identify whole-source observations. Neither
is a durable global history or deduplication guarantee. Native IDs and parent
links are retained without inventing trace/span IDs.

## LogsData JSONL envelope

Each nonempty `write_batch` emits exactly one compact UTF-8 JSON object followed
by one LF. Embedded newlines/control characters in strings are JSON-escaped.
Record order is unchanged. Repeated calls append independent lines; an empty
batch neither writes nor flushes. The sole top-level key is `resourceLogs`:

```json
{"resourceLogs":[{"scopeLogs":[{"scope":{"name":"unisphere","version":"0.1.0"},"logRecords":[{"eventName":"unisphere.session.record","attributes":[]}]}]}]}
```

The example shows the envelope, not a complete Claude-profile record. Profile
attributes below belong inside each record's `attributes` array, as
`{"key":"...","value":<AnyValue>}`. The writer adds no nonstandard OTLP fields.

| Field | Encoding and meaning |
| --- | --- |
| `scope.name`, `scope.version` | `unisphere`, `0.1.0`: the encoding scope, not source/provider identity. |
| `eventName` | Supplied event category string. |
| `timeUnixNano` | Supplied `u64` nanoseconds since Unix epoch as a decimal string; omitted for `None`. Native RFC3339 timestamps are parsed upstream, never replaced with wall-clock time. |
| `attributes` | Unique keyed attributes represented as OTLP `KeyValue` entries; an empty map is `[]`. |
| `body` | Optional recursively structured `AnyValue`; `None` omits it, while explicit JSON null produces `{}`. |

`ResourceLogs.resource` is omitted because resource attribution is unknown.
Both `schemaUrl` fields are omitted; a protobuf revision is not an OpenTelemetry
semantic schema URL. `severityNumber`, `severityText`, `observedTimeUnixNano`,
`flags`, `traceId`, `spanId` and `droppedAttributesCount` are omitted rather than
fabricated. Under protobuf defaults, absent counts/flags are zero, severity is
unspecified and IDs empty; none establishes an observed fact or correlation.
Absent timestamp and explicit `"0"` both mean unknown/missing time to an OTLP
consumer; the encoder preserves the supplied distinction but cannot make epoch
zero unambiguous in OTLP.

This is source-data conversion to **LogsData**, not an OTLP collector protocol
request or a full OpenTelemetry logging SDK. The proto's observed-time requirement
applies when OpenTelemetry observes an event: this conversion intentionally does
not observe a clock. A downstream collecting system must establish its own
observation timestamp rather than treating an omitted value as measured time.

### AnyValue conversion

| Supplied JSON value | OTLP JSON |
| --- | --- |
| String | `{"stringValue":"..."}` |
| Boolean | `{"boolValue":true}` or `false` |
| Integer within `i64` | `{"intValue":"-9223372036854775808"}` through `"9223372036854775807"` |
| Floating-point number | `{"doubleValue":1.25}`; a number, not a decimal string |
| Array | `{"arrayValue":{"values":[<AnyValue>,...]}}` |
| Object | `{"kvlistValue":{"values":[{"key":"...","value":<AnyValue>},...]}}` |
| Null | `{}`: an empty AnyValue with no selected variant |

An object is **not** a JSON string inside `stringValue`. Nested tool arguments,
responses, arrays, booleans and numbers remain typed at every level. Empty arrays
and objects retain their respective variants. JSON provides no byte-string type,
so this writer does not invent `bytesValue`. An integer outside `i64` is rejected
with `InvalidData` (`UNI-DATA`), including in a nested body or attribute; it is
never rounded to double or relabeled as a string. The timestamp is separately
`u64` and can reach `"18446744073709551615"`.

## Native snapshot provenance and replacement manifests

All formats retain `unisphere.profile.version` as the **integer** `1`,
`unisphere.source.adapter`, `unisphere.source.path` and `unisphere.source.kind`.
Snapshot-derived records additionally require:

| Key | Value and meaning |
| --- | --- |
| `unisphere.source.key` | Native SQLite key or mapper's structural document/journal location. |
| `unisphere.source.revision` | Loader-owned snapshot SHA256 revision, or the Git object identity described below. |
| `unisphere.source.format` | `json_document`, `json_journal`, `sqlite_key_value` or `git_notes`. |
| `unisphere.source.session.id` | Optional verified native selected session identity. |

`unisphere.source.offset` is **absent** for snapshots, never a synthesized ordinal.
The composed proof selects the required provenance set by the registered source
representation; JSONL continues to require actual physical byte offsets.

The SDK appends one **`unisphere.session.snapshot`** event after a full snapshot
projection. Its source kind is `snapshot_manifest`, structural key `$snapshot`,
and body/timestamp are absent. It adds these explicit extensions:

| Key | Meaning |
| --- | --- |
| `unisphere.snapshot.semantics` | `replace_projection`; not an append-only stream transaction. |
| `unisphere.snapshot.records` | Number of preceding projected records in this batch. |
| `unisphere.snapshot.selection` | Structured explicit source path/format/table/session selector. |
| `unisphere.snapshot.requested_session.id` | Optional caller selection, not an invented native observation. |
| `unisphere.snapshot.include_content` | Applied content policy. |
| `unisphere.snapshot.finality` | `unknown`; no claim that a producer is finished. |

Empty snapshots still produce a closing manifest so consumers can remove a
previous projection. A caller must accept the entire output before replacing its
view. Encoding is one bounded writer batch; failed writes may leave a prefix and
return no checkpoint. No history store, destination binding or exactly-once
guarantee is implied by the manifest.

Native field/usage registries and their scope distinctions live in
[Codex](codex-adapter.md), [Oh My Pi](oh-my-pi-adapter.md), [Pi](pi-adapter.md),
[Copilot CLI](copilot-cli-adapter.md), [VS Code Copilot](vscode-copilot-adapter.md)
and [Cursor](cursor-adapter.md). Their `unisphere.*` fields are explicit native
extensions, not additional OpenTelemetry standard totals. In particular,
latest-call, whole-turn, checkpoint and cumulative-session counters are not
interchangeable or automatically summed.

## Git Notes attribution and selection manifests

The `git-ai` adapter independently projects supported `authorship/3.0.0` note
variants, including mixed current sessions, legacy 16/7-hex prompt keys and known
humans. Normal/optionally quoted paths, single lines and inclusive ranges retain
their source meanings. Unknown variants and duplicate JSON keys fail explicitly.
Valid unresolved keys remain attribution records with `identity_resolution`
`unresolved`; no identity is invented or loaded from another note/cache.

| Event | `unisphere.source.kind` | `unisphere.source.key` |
| --- | --- | --- |
| `unisphere.git_ai.note` | `note_metadata` | `$note` |
| `unisphere.git_ai.identity` | `declared_identity` | `metadata/<prompts\|sessions\|humans>/<native-key>` |
| `unisphere.git_ai.attribution` | `line_attribution` | `attestations/<file-index>/<entry-index>/<range-index>` |
| `unisphere.git_notes.snapshot` | `notes_manifest` | `$git-notes` |

All four retain integer profile version `1`, adapter `git-ai`, canonical selected
repository as `unisphere.source.path`, and source format `git_notes`. Structural
indices are not byte offsets. No event timestamp, OTel trace/span, tool duration
or token total is synthesized.

Every record has `unisphere.git.repository.id` (canonical Git common directory,
local identity only), `unisphere.git.notes.ref` and `unisphere.git.notes.tip`.
Note-derived records also have `unisphere.git.commit`, `unisphere.git.note.blob`
and `unisphere.source.revision` equal to that blob. Attribution adds
`unisphere.git_ai.file.path`, `attestation.key`, `identity.key`, `identity.kind`,
`identity_resolution`, positive inclusive `line.start`/`line.end`, and native
`session.id`/`checkpoint.id` when supplied by a session key. These suffixes are
all under `unisphere.git_ai`, not new standard OTel fields.

Declared agent fields are `unisphere.git_ai.agent.tool`, `.id` and `.model`.
Supplied prompt statistics retain their native names (`total_additions`,
`total_deletions`, `accepted_lines`, `overriden_lines`) and scope. Missing fields
stay absent; explicit null stays empty AnyValue. Human strings, custom attributes,
legacy messages and message URLs are included only with explicit content policy,
once on their identity record. URLs are never fetched.

The closing manifest has no body, timestamp, target commit or note blob.
Its revision and notes tip are the pinned ref commit, or explicit null for a
missing ref. Its `unisphere.git_notes.*` fields are:

| Suffix | Meaning |
| --- | --- |
| `semantics` | `replace_projection`, scoped to the selected repository/ref/selection only. |
| `records` | Count of preceding mapped records, excluding this manifest. |
| `notes` | Count of selected note objects. |
| `selection` | Structured canonical repository, requested notes ref, `mode: all\|commits`, and normalized requested `commits` when selected. |
| `include_content` | Applied content-policy boolean. |
| `finality` | `unknown`; no producer/session completeness claim. |
| `ref_state` | `present` or `missing`, distinguishing absence from a present empty selection. |

An empty selection still emits this manifest. Accept the complete output before
replacing its selected view; failed writes can leave a prefix and return no
accepted collection. Ref pinning identifies objects without retaining them
against Git garbage collection. Tracking refs are not automatically aggregated.


## Claude attribute registry

These are log attributes, not resource attributes or new OTLP message fields.
All `unisphere.*` names below are **explicit Unisphere extensions**. This table
owns Claude's keys; other native field registries are linked above.
Required means present on every mapped physical record, including unsupported
native kinds. Optional values are retained only when their native types are valid.

| Key | Value type | Presence and meaning |
| --- | --- | --- |
| `unisphere.profile.version` | Integer `1` | Required; version of this source-record profile. |
| `unisphere.source.adapter` | String | Required; `SessionAdapter::name()`, `claude-code` for this profile. |
| `unisphere.source.path` | String | Required; explicit supplied UTF-8 source path. |
| `unisphere.source.offset` | Nonnegative integer | Required; physical record's starting byte offset. Like all AnyValue integers, encoding must fit `i64`. |
| `unisphere.source.kind` | String | Required; raw native `type` when a string, otherwise literal `unknown` with `InvalidField`. An unrecognized string is retained with `UnsupportedRecord`. |
| `unisphere.source.record.id` | String | Optional native `uuid`. |
| `unisphere.source.parent.id` | String | Optional native `parentUuid`; no inferred parent session or span relationship. |
| `unisphere.source.is_sidechain` | Boolean | Optional native `isSidechain`; no inferred ownership. |
| `unisphere.message.id` | String | Optional native `message.id`; does not establish a complete response or deduplication key. |
| `unisphere.message.role` | String | Retained role for supported user/assistant message fragments. |
| `gen_ai.conversation.id` | String | Optional explicitly declared native `sessionId`; standard attribute, not inferred from a directory. |
| `gen_ai.response.model` | String | Optional observed `message.model`; standard attribute, not a provider or requested-model guess. |
| `unisphere.content.omitted` | Boolean `true` | Present when content exists but metadata-only mapping omits it; accompanies `ContentOmitted`. |
| `unisphere.usage.input_tokens` | Integer `0..i64::MAX` | Optional exact native `message.usage.input_tokens` snapshot. |
| `unisphere.usage.output_tokens` | Integer `0..i64::MAX` | Optional exact native `message.usage.output_tokens` snapshot. |
| `unisphere.usage.cache_read_input_tokens` | Integer `0..i64::MAX` | Optional exact native `message.usage.cache_read_input_tokens` snapshot. |
| `unisphere.usage.cache_creation_input_tokens` | Integer `0..i64::MAX` | Optional exact native `message.usage.cache_creation_input_tokens` snapshot. |
| `unisphere.usage.scope` | String `native_record_snapshot` | Present if any of the four usage components is retained. |

The Claude usage namespace is closed to those five keys in v1. Missing usage remains
absent, never zero. Negative, noninteger and out-of-range components are omitted
with `InvalidField`; unknown components are not guessed. Components are not
summed, deduplicated or aggregated across repeated physical records. No
`gen_ai.usage.*` totals are emitted: provider counter inclusion, overlap, and
snapshot-vs-final semantics are not established. In particular, adding cache
components to an unproven input total can double count. These extensions preserve
observations without falsely claiming standard usage semantics.

No provider name, request model, trace/span correlation, measured severity, or
whole-conversation input/output attribute is inferred. Unsupported native kinds
retain provenance and a diagnostic rather than silently disappearing or exposing
the entire native payload.

## Message fragments and content policy

Metadata-only is the default (`MappingOptions::default().include_content ==
false`). Bodies, thinking, tool arguments/results and unknown part payloads are
excluded; `unisphere.content.omitted=true` and `ContentOmitted` identify omitted
content when present. **Metadata-only is not anonymization**: source paths, IDs,
model names and other permitted metadata can still be sensitive. The writer
applies no redaction and must receive already policy-filtered records.

With explicit content opt-in, supported message bodies are logical objects
`{"role":"...","parts":[...]}`, encoded recursively as AnyValue. They are
GenAI-shaped **fragments**, not `gen_ai.input.messages` history or
`gen_ai.output.messages` complete choices.

| Native content | Logical part before AnyValue encoding |
| --- | --- |
| String or `text.text` | `{"type":"text","content":"..."}` |
| `thinking.thinking` | `{"type":"reasoning","content":"..."}` |
| `tool_use` | `{"type":"tool_call","name":"...","id":"...","arguments":<structured input>}`; ID is optional. |
| `tool_result` | `{"type":"tool_call_response","id":"...","response":<structured content>}`; ID comes from `tool_use_id` when present. |
| Unknown part | `UnsupportedPart`; an optional generic part with `type:"unisphere.unknown"` and `native_type` may retain selected native data only under content opt-in. |

`unisphere.is_error` is an extension boolean property **inside a tool-response
part** when native `is_error` is supplied, not an invented standard attribute.
`unisphere.unknown` is an extension part-type value; `native_type` describes that
extension's native discriminator. These parts use the referenced schema's
extensible object/generic-part shape, not a new standard part definition. Unknown
parts and recognized transformations do not promise full-byte fidelity. Files,
attachments, sidecars and URIs are never dereferenced by the mapper or writer.

## Bounds, delivery and proof

The writer stages the **actual OTLP representation** in a capped `std::io::Write`
buffer. `MAX_OUTPUT_BATCH_BYTES = 33_554_432` (32 MiB) includes the terminating LF
and all AnyValue wrappers, JSON escaping and UTF-8 bytes. Over-budget encoding
returns `OutputLimit` (`UNI-LIMIT-OUTPUT`) before any destination write or flush.
It does not first serialize an unbounded tree and measure afterward. Invalid
values likewise fail before destination access, even in the last record.

Once encoding succeeds, the writer calls `write_all` and then `flush`. Short
writes and interrupted writes follow the standard library's retry behavior;
zero writes, terminal write errors and flush errors return `Write` (`UNI-WRITE`).
Public diagnostics discard underlying I/O/serializer text and contain no source
payload or arbitrary path. A failed sink may already contain a prefix or a whole
line: there is no rollback, atomic-file, fsync, exactly-once or durable-delivery
promise. A caller must not accept its checkpoint on failure and must handle any
partial output before retrying.

Scoped behavioral proof: `cargo test -p unisphere-output-otlp` (guide `vd-0004`).
The tests cover exact legal OTLP envelopes, recursive typed values, integer and
timestamp boundaries, absent values, JSONL framing, late invalid data, short and
interrupted writes, safe partial/zero-write and flush errors, exact encoded-limit
acceptance including LF, and escape-expansion rejection before sink access.
The runnable `cargo run -p unisphere-output-otlp --example encode` writes a
synthetic structured record to supplied stdout; it is not native collection proof.
