# Codex rollout adapter

`unisphere-adapter-codex` exports the stateless `CodexAdapter: SessionAdapter` and
`pub const DESCRIPTOR` with ID `codex`. It accepts supplied `NativeRecord` bytes;
it never reads files, indexes, attachments, environment variables, networks or a
clock. The composition root owns registration, discovery, loading and output.
The usual location hint is `.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`; the hint
is not discovery or evidence that Codex is installed. The shared JSONL loader's
Unix export and caller-owned append-only cursor limitations still apply.

## Physical records, not reconstructed turns

Every syntactically valid supplied JSON value produces one
`unisphere.session.record`, including unsupported records. Required provenance is
`unisphere.source.adapter = "codex"`, the supplied UTF-8 path, actual byte offset,
outer native `type` as `unisphere.source.kind`, and **numeric**
`unisphere.profile.version = 1`. Missing/invalid kinds use `unknown` with a typed
diagnostic. For `response_item` and `event_msg`, the inner discriminator is
`unisphere.codex.payload.type`. Repeated logical IDs retain separate physical
records. Offsets are not inferred from timestamps, filenames or line ordinals.

Only the outer RFC3339 `timestamp` becomes `timestamp_unix_nano`; offsets and
nanoseconds are preserved. Missing timestamps remain absent. Invalid, pre-epoch
or out-of-u64-nanosecond-range values produce `InvalidTimestamp`, never a clock
value or a timestamp borrowed from `session_meta.payload`.

Context is local to each record. A session header's native `id` is
`gen_ai.conversation.id`; separately supplied `session_id` is
`unisphere.source.session.id`. These are not silently equated: newer native
schemas distinguish thread and root session identities. Native `turn_id`,
`root_turn_id`, response item `id` and tool `call_id` retain separate attributes.
A response item's optional
`internal_chat_message_metadata_passthrough.turn_id` is retained. No session,
model, tool name or turn identity is carried into later records. Mapping a batch,
replaying it or splitting it at any physical boundary has the same result.

## Supported projection

| Native shape | Projection |
|---|---|
| `session_meta` | Native thread/root-session/fork/parent IDs, originator, CLI version and explicitly supplied model provider; no conversation body |
| `turn_context` | Native turn IDs, `model` as `gen_ai.request.model`, and reasoning effort; no invented response model |
| `response_item.message` | Explicit user/assistant/system/developer role, item ID and phase; opt-in ordered `input_text`/`output_text` parts |
| `response_item.reasoning` | Ordered readable summary/content as reasoning parts; `unisphere.codex.native_type` distinguishes `summary_text`, `reasoning_text` and `text` |
| `function_call`, `custom_tool_call` | Native item/call IDs, name/namespace/status; opt-in `tool_call` part. Function `arguments` stays a string, including its exact JSON encoding; custom `input` stays a freeform string |
| `function_call_output`, `custom_tool_call_output` | Native call ID/name only when present; opt-in `tool_call_response` with exact string output or ordered structured text/unsupported-part markers |
| `local_shell_call` | Native IDs/status; opt-in exec action command vector, working directory and user. `unisphere.codex.tool_kind` identifies this native tool kind without fabricating a tool name or treating legacy item ID as call ID |
| `web_search_call` | Native item ID/status and opt-in search query/queries, open-page URL or find-in-page URL/pattern; references are not followed |
| `event_msg.token_count` | Independently measured last and cumulative usage components; see below |
| `event_msg.user_message`, `agent_message`, `agent_reasoning`, `agent_reasoning_raw_content` | Opt-in explicitly named native summary parts, not ordinary message/reasoning turns |
| Task/turn started/completed, turn aborted, context compacted events | Physical event metadata; not completion/finality of collection |
| `exec_command_begin`, `exec_command_end` | Native call ID and valid signed exit code; no duplicate tool-call/output bodies |
| `compacted` | Optional opt-in `unisphere.compaction` text, never an ordinary turn; replacement history is not replayed |
| Response `compaction`, `compaction_summary`, `context_compaction` | Explicit compaction classification, no opaque encrypted body |

Event summaries carry `unisphere.codex.representation = native_event_summary`;
compaction carries `compaction`. Only native message response items receive an
ordinary message role. For example, an agent-message event can repeat a response
item's text; its opt-in part is `unisphere.codex.message_summary`, not `text` in a
second assistant turn. Do not count every exported physical record as a model
operation. No deduplication, tool pairing across records, reconstruction or
source finality is claimed.

## Usage: two scopes, no arithmetic

`event_msg.payload.info.last_token_usage` and `total_token_usage` are kept under
separate attribute prefixes:

- `unisphere.usage.last_token_usage.<component>` with `.scope = native_last_token_usage`.
- `unisphere.usage.total_token_usage.<component>` with `.scope = native_cumulative_token_usage`.

Supported independently measured components are `input_tokens`,
`cached_input_tokens`, `cache_write_input_tokens`, `output_tokens`,
`reasoning_output_tokens` and `total_tokens`. Only supplied nonnegative i64 values
are retained. Wrong types, negatives and values above i64 max produce
`InvalidField`; valid siblings survive. Missing components are absent, not zero.
A supplied nonnegative `model_context_window` is native context metadata, not
usage. Null/absent `info` means no usage observation.

No input/cache subtraction, reasoning/output addition, estimated total, delta
from previous record or cumulative aggregation is performed. A repeated
cumulative snapshot remains the same measured value at another physical offset;
it must not be summed as another operation. No standard `gen_ai.usage.*` totals
are synthesized from these snapshots.

## Content policy and remaining gaps

Metadata-only always has `body = None`; supported content is marked omitted with
`ContentOmitted`. Opt-in content uses the existing structured `parts` body
convention, never a raw payload dump. Malformed supported fields produce
`InvalidField`; unknown native records produce `UnsupportedRecord`; unsupported
parts produce `UnsupportedPart` and, within supported part arrays, content-free
markers. Unsupported parts cannot erase supported siblings. Diagnostic payloads
contain only the physical offset and typed code. Invalid JSON/UTF-8 fails the
entire supplied batch with fixed `UNI-DATA` at the bad offset, not parser text or
source contents.

This is a documented projection, **not a lossless archive**:

- Image/audio data, URLs in message/tool-output media parts, encrypted reasoning,
  encrypted function arguments and encrypted output are not copied or decoded.
  Supported part arrays retain only content-free unsupported markers. Web-search
  action URLs are explicitly opt-in strings, not attachment dereferences.
- Session instructions and recognized user/developer context instructions are
  unsupported non-message content. Configuration, cwd, git metadata, dynamic
  tools, rate limits and extension fields are not a general metadata dump.
- Shell environment and timeout fields are not projected. Execution-event
  command/stdout/stderr and task-complete `last_agent_message` are not duplicate
  tool/message bodies. Streaming delta events are unsupported records, not
  concatenated output. Native output strings are not parsed for inferred status,
  exit code, duration or embedded JSON.
- Compaction replacement history is not replayed. World-state, configuration,
  arbitrary custom state, newer inter-agent `agent_message` response items,
  tool-search/image-generation response variants and unknown records retain
  provenance plus unsupported diagnostics, not invented ordinary turns.
- No SQLite index, archived-session tree, inherited parent/fork history, sidecar,
  attachment or separate transcript is loaded. A later batch need not contain a
  header. No persisted resume, previous-record backfill, delayed revision
  reconciliation or complete-session coverage is claimed.

## Schema basis and proof boundary

The approved structural research establishes the timestamp/type/payload rollout
family across multiple CLI eras, readable messages/reasoning/function/custom
tools, optional native IDs and distinct last/cumulative usage. Field shapes were
also checked against the public OpenAI protocol definitions on 2026-09-08:

- [Protocol structs: session metadata, turn context, events and token usage](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs).
- [Response items, content items and native tool-output wire encoding](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/models.rs).

These evolving upstream sources are evidence for named shapes, not a claim that
all historical/future variants are supported. The fixture
`crates/adapter-codex/fixtures/rollout.jsonl` is authored synthetic data using
reserved `SENSITIVE-` content markers, not copied private session material.

Worker validation is intentionally not executed; the PM owns lockfile updates,
formatting, build/test/lint/boot and composition evidence. After coordinated
lockfile integration, the targeted command is:

```sh
cargo test --locked -p unisphere-adapter-codex
```

The authored regressions cover shared conformance and all batch split points,
metadata content exclusion, distinct native IDs, exact string tool arguments,
structured outputs, summaries/compaction versus turns, repeated usage scopes,
numeric boundaries, invalid timestamps, malformed bytes, unsupported sibling
parts and native shell/search actions. Passing this crate would establish only
these synthetic mapper contracts; production descriptor registration, SDK/CLI
export and OTLP provenance still require the PM's composed product proof.
