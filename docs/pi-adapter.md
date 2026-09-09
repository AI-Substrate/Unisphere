# Pi JSONL adapter

`unisphere_adapter_pi::PiAdapter` implements the core `SessionAdapter` port.
`DESCRIPTOR.id`, `PiAdapter::name()` and emitted adapter provenance are `pi`.
The mapper accepts supplied `NativeRecord` bytes and a validated explicit
`SessionRef`; it performs no filesystem, environment, network, clock, discovery,
sidecar or output access. The application composition root owns registration.

## Native basis and scope

Implemented against the installed Pi coding agent 0.83.0 public declarations:
`@earendil-works/pi-coding-agent/dist/core/session-manager.d.ts`,
`dist/core/messages.d.ts`, and its `@earendil-works/pi-ai/dist/types.d.ts`.
The repository's plan 010 structural research identifies ordinary header-first
v3 JSONL trees under `.pi/agent/sessions/<project>/*.jsonl`. These are symbolic
location hints, not installation observations or implicit discovery rules.
No private session payloads were used; the regression fixture is synthetic.
Pi is not the OMP dialect and has no OMP 256-byte mutable title prefix.

Every supplied valid JSON record produces one `unisphere.session.record`, in
physical order. Required provenance includes numeric `unisphere.profile.version`
`1`, adapter `pi`, the caller's source path, actual supplied byte offset and
native record kind. Native `id` and non-null `parentId` retain tree identity;
`unisphere.pi.root_entry=true` distinguishes an explicit null parent. Repeated
IDs are not deduplicated. Header `id` supplies `gen_ai.conversation.id` on that
header only. No session ID, model, active branch or context is propagated across
records or batches. A resumed batch needs no header replay.

| Native record / role | Projection |
|---|---|
| `session` | Version and session identity; opt-in `cwd` / `parentSession` references |
| `model_change`, `thinking_level_change` | Explicit provider/model or thinking-level change, not inferred later message state |
| `message:user` | User role; opt-in text and image parts |
| `message:assistant` | Selected model/provider/API, explicit response model/ID, stop reason, native usage; opt-in text, reasoning and tool calls |
| `message:toolResult` | Tool role, call ID/name, error flag, added tool names; opt-in structured response parts and separately scoped tool usage |
| `compaction`, `branch_summary` | Tree references, hook flag, pre-compaction token count, separately scoped summary usage; opt-in summary body, never a user/assistant turn |
| `custom` | Extension type and provenance only; arbitrary extension state is not conversation content or usage |
| `custom_message`, `message:custom` | Extension type/display flag and opt-in text/image parts under `kind:custom_message`, not promoted to an ordinary user turn |
| `label`, `session_info` | Label target identity and opt-in label/name; absent label represents the native label-removal operation |
| `message:bashExecution` | Exit/cancel/truncate/context-exclusion facts; opt-in command, output and full-output path reference; not an inferred tool call |
| `message:branchSummary`, `message:compactionSummary` | Explicit summary kind and source references/counts, not reconstructed ordinary turns |

`message.model` is the selected `gen_ai.request.model`; only an explicit
`responseModel` supplies `gen_ai.response.model`. `responseId` is not a trace ID.
The physical entry's RFC3339 timestamp is converted exactly to unsigned Unix
nanoseconds. The message's separate integral epoch-millisecond timestamp is
retained as `unisphere.pi.message.timestamp_unix_nano`. It never repairs or
replaces the physical entry timestamp. Missing times remain absent; wrong units,
negative, fractional, overflowing or unrepresentable values produce
`InvalidTimestamp`, not a clock-derived value or saturation.

## Content and omissions

Default `MappingOptions` produces `body=None` for every record. Prompts, output,
reasoning, arguments, images, errors, summaries, labels, names, working-directory
and sidecar-reference values do not escape into metadata. Structural identifiers,
model/provider names, tool names/call IDs, extension types and the explicitly
supplied source path are metadata, not an anonymization guarantee.
`unisphere.pi.tool.calls` retains assistant call IDs/names without arguments.

With `include_content=true`, supported bodies keep structured parts: `text`,
`reasoning`, `image`, `tool_call`, and `tool_call_response`. Tool results contain
structured text/image responses, not flattened string dumps. Images retain only
the supplied native data and MIME type; no decoding, URL access or attachment
read occurs. Bash full-output paths and session-parent paths are references only.
Control/summary bodies use a distinct `kind`, not an ordinary message `role`.

Opaque text/thinking/tool signatures, redacted reasoning payloads, provider
`diagnostics`, tool/summary/custom `details` and custom `data` are omitted even
with content enabled, with `ContentOmitted` and `unisphere.content.omitted`.
Redacted thinking becomes a payload-free `unisphere.redacted_reasoning` marker.
Unknown parts produce `UnsupportedPart` and an opt-in `unisphere.unknown` marker
containing only the native type. Supported sibling parts survive malformed parts.
Unknown fields are not copied wholesale. Content opt-in is a documented
projection, not raw archival retention.

## Usage fidelity

All counters are measured native components, accepted independently only as
nonnegative signed-64-bit integers. Missing or invalid components are not zero.

| Native usage field | Attribute |
|---|---|
| `input`, `output` | `unisphere.usage.input_tokens`, `unisphere.usage.output_tokens` |
| `cacheRead`, `cacheWrite` | `unisphere.usage.cache_read_input_tokens`, `unisphere.usage.cache_creation_input_tokens` |
| `cacheWrite1h` | `unisphere.pi.usage.cache_write_1h_tokens` (subset of cache write) |
| `reasoning` | `unisphere.pi.usage.reasoning_tokens` (subset of output) |
| `totalTokens` | `unisphere.pi.usage.total_tokens` (reported total only) |
| `cost.{input,output,cacheRead,cacheWrite,total}` | `unisphere.pi.usage.cost.<native field>` (finite, nonnegative native values) |

`unisphere.usage.scope` is `assistant_message`, `tool_execution`, `compaction`,
or `branch_summary`, according to the actual usage-bearing record. Native costs
carry that same scope without currency conversion or inferred currency. Summary
usage can represent the native aggregate of multiple summarization calls. Tool
usage is not part of main-model accounting. `tokensBefore` is a context-size
observation at `unisphere.pi.compaction.tokens_before`, not consumed usage.

No totals are calculated, no cache or reasoning subset is added twice, no
`gen_ai.usage.*` totals are guessed, and no cross-record accounting or operation
pairing is claimed. Repeated physical usage observations remain repeated.

## Errors and ceilings

Malformed JSON/UTF-8 fails the entire supplied batch with fixed `UNI-DATA` and
the offending physical byte offset; parser payloads never enter public errors.
Wrong field types yield `InvalidField`; unsupported records/roles yield
`UnsupportedRecord` with provenance retained and no payload body. Diagnostics
contain only typed codes and offsets. A bad explicit source fails before parsing.

Version 3 is the documented dialect. Non-v3 or missing header versions produce
`UnsupportedRecord`; compatible individual fields can still be projected.
The mapper does not run Pi's v1/v2 migration, fabricate legacy tree IDs, accept
legacy `hookMessage` as modern `custom`, or infer a batch-wide version from a
prior header. Unknown extension-defined message roles and future record/part
variants remain explicit unsupported projections. No schema completeness beyond
the named declarations is claimed.

All branches are physical observations, not an active-leaf conversation rebuild.
Compaction, branch selection, custom context transformation, label reduction and
parent-session traversal are not replayed. Migration/repair can rewrite JSONL;
caller-owned byte/inode resume assumes append-only source generations and cannot
prove absence of same-inode rewrites or truncate-and-regrow. There is no persisted
CLI resume, delayed-update reconciliation, lossless archive or final-session claim.
The mapper is platform-neutral; the existing registered loader/export pipeline
is Unix-only. Storage limits and output limits remain loader/writer concerns.

## PM validation handoff

Worker-authored coverage is in `crates/adapter-pi/tests/mapping.rs`, using the
18-record synthetic `tests/fixtures/v3-tree.jsonl` and shared testkit conformance.
It covers privacy, structured opt-in, tree identity, split/replay equivalence,
usage scopes/subsets, timestamps, unsupported shapes, safe failures and explicit
source validation. The worker did not run formatting, lint, builds, tests or boot;
these are PM-owned coordinated proof, not claimed successes.

After the PM admits the crate and updates the workspace lockfile, run:

```sh
cargo test --locked -p unisphere-adapter-pi
```

The PM also runs the coordinated formatter, architecture checks, workspace lint
and product collection/provenance smoke against the composed registered artifact.
Independent source review is required for purity; unit fixtures alone are not
proof of a filesystem/network-denied runtime or installed CLI integration.
