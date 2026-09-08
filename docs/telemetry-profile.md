# Telemetry profile v1

This is the normative key registry for Unisphere's Claude source-record profile.
Adapters assign semantics; `unisphere-output-otlp::OtlpJsonlWriter` preserves the
supplied records and owns only OTLP encoding. It neither validates an adapter's
profile nor reads sources, opens destinations, observes a clock, or sends data to
an endpoint.

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
instance. Source path and byte offset preserve physical provenance; they are not
a durable global identity across file replacement or replay. Native UUIDs and
parent UUIDs are retained without inventing trace or span IDs.

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

## Claude attribute registry

These are log attributes, not resource attributes or new OTLP message fields.
All `unisphere.*` names below are **explicit Unisphere extensions**. This table is
the single registry; adapter documentation describes extraction and links here.
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

The usage namespace is closed to those five keys in v1. Missing usage remains
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
