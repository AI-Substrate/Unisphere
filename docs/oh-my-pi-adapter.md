# Oh My Pi JSONL adapter

`unisphere-adapter-omp` exports the stateless `OmpAdapter: SessionAdapter` and
`DESCRIPTOR` with selection/provenance ID `oh-my-pi`. This is **not** the Pi
adapter: Oh My Pi has a fixed mutable title prefix and additional native entry,
message, usage and reference shapes.

## Input and physical identity

The caller supplies `SessionRef` and LF-framed `NativeRecord` bytes. The mapper
opens nothing: no files, environment, network, clock, output or sidecars. Location
hints describe `.omp/agent/sessions/<project>/*.jsonl`; they do not discover it.
Registration and application/CLI composition are owned separately by the PM.

Every supplied valid JSON value yields one `unisphere.session.record`, in input
order, with numeric `unisphere.profile.version = 1`, adapter ID, explicit source
path, actual byte offset and native record kind. Entry `id` and `parentId` remain
source record/parent identifiers, not invented spans or traces. A header's `id`
becomes `gen_ai.conversation.id` on that header only. The mapper neither reads a
header from disk nor carries header/model/branch state between calls. Splitting a
batch at any record boundary does not change its projection.

Malformed JSON or UTF-8 returns one fixed `InvalidData` error at the offending
physical offset, without a partially successful mapped batch or parser/source
text. Valid but unsupported shapes remain observable through provenance and
`UnsupportedRecord`, `UnsupportedPart`, `InvalidField` or `InvalidTimestamp`
diagnostics. Diagnostics contain only a typed code and offset.

## Native records

| Native kind | Projection |
| --- | --- |
| `title` | v1 title slot, `updatedAt` event time, source, mutable marker; title in opt-in source-event body. Padding is not content. Validate offset zero and 255 bytes excluding LF: the slot occupies exactly 256 UTF-8 bytes. |
| `session` | v3 header/version/session identity; opt-in title, cwd and parent-session reference. Legacy/missing and future header versions receive `UnsupportedRecord`, with no migration or inferred schema. Title-less v3 files need no artificial title slot. |
| `message` | Native message role, distinct native message timestamp and supported role-specific projection below. |
| `model_change` | Verbatim `provider/model` selection and optional model role; not a response model and not carried into later messages. |
| `thinking_level_change` | Native level/configured selector; explicit null remains a clearing operation. |
| `service_tier_change` | Explicit openai/anthropic/google tier selections, null clear, or uninterpreted legacy string; no provider-family inference. |
| `compaction`, `branch_summary` | Source events with first-kept/from identifiers, tokens-before and extension marker; opt-in summaries/warning. Tokens-before are not model usage. |
| `custom`, `mode_change` | Extension discriminator or mode only; arbitrary data is unsupported, never an ordinary turn. |
| `custom_message` | Extension discriminator/display plus opt-in structured content in a source-event body, not a fabricated user/assistant turn. |
| `label`, `title_change` | Target/source metadata and opt-in label or current/previous title/trigger in source-event bodies. |
| `session_init`, `ttsr_injection` | Opt-in system/task/tool-name/spawn context or injected-rule names in source-event bodies; schema enforcement flags remain native metadata. |
| Unknown | Provenance-only record plus `UnsupportedRecord`; no raw payload copied. |

The title slot can be rewritten **before a retained append cursor** without
changing subsequent byte offsets. Append-only collection therefore misses such
retitles. A supplied `title_change` audit entry is independently exported, but
is not a guarantee that every slot rewrite has an audit entry. Re-reading the
file from the start can observe the current slot, but this mapper performs no
reconciliation, deletion handling or title refresh itself.

## Messages and opt-in content

Metadata-only always has `body = None`. Message text, thinking, tool arguments,
results, titles, prompts, file contents and reference locators are only emitted
with `MappingOptions { include_content: true }`. Omission is marked with
`ContentOmitted` when supported content-bearing records are processed. Structural
validation still runs when content is disabled. Metadata such as source path,
record IDs, model, role, custom discriminator and tool name is not anonymized.

- `user`, `developer`, `assistant`: ordered text, reasoning, tool-call and image
  parts. Assistant model/provider/API/upstream provider/response ID/stop reason,
  duration/TTFT and numeric error metadata remain independent of content opt-in.
  Native error text is opt-in only.
- `toolResult`: common body role `tool`, a `tool_call_response` containing ordered
  response parts, native call ID/name and error flag. Metadata keeps the native
  `toolResult` role, call identity, error/pruning/useless flags. No tool-call
  matching or result-content recovery is attempted.
- `bashExecution`, `pythonExecution`: command/code and output source events with
  exit/cancellation/truncation/context-exclusion metadata; not model tool calls.
- `custom`, legacy `hookMessage`, `compactionSummary`, `branchSummary`,
  `fileMention`: explicit source-event bodies, not ordinary model turns. File
  mentions preserve supplied paths/content/counts/skipped reasons/inline images
  only on opt-in; referenced files are never opened.
- `text` and `thinking` become common `text` and `reasoning` parts. `toolCall`
  requires ID, name and object arguments and retains opt-in native intent,
  raw-block and custom-wire-name fields. Fallback boundaries retain from/to
  models as `unisphere.model_fallback`, not assistant text.
- Inline `image` data remains supplied data with MIME type/detail; there is no
  decode or re-encode. `blob:sha256:…` becomes an unresolved `unisphere.reference`
  rather than masquerading as image bytes. Tool `details.meta`/execution `meta`
  can expose supplied artifact IDs and path/URL/internal source references on
  opt-in. They are never fetched, hash-verified, expanded or claimed resolved.

Opaque signatures, `redactedThinking`, provider transport payloads, retry recovery,
context snapshots, stop details, arbitrary tool/custom/compaction details,
preserved extension state and output schemas are not reconstructed. They receive
`UnsupportedPart` where recognized; unknown content types retain only their type
marker on opt-in. Arbitrary new fields outside the supported projection are not
copied. Compaction-summary runtime image/archive blocks are unsupported. This is
a documented semantic projection, not a lossless provider-history archive.

## Time and measured usage

The event timestamp is the outer entry/header RFC3339 `timestamp`, or title-slot
`updatedAt`. Native message `timestamp` is separately retained as
`unisphere.message.timestamp_unix_nano`, converted from integer Unix milliseconds
with checked multiplication. No header, file mtime, wall clock, neighboring
message or guessed fallback supplies missing time. Invalid, negative or
out-of-range timestamps are omitted with `InvalidTimestamp`.

Assistant `usage` is retained under `unisphere.usage.*` with
`scope = native_record_snapshot`:

| Native component | Attribute suffix |
| --- | --- |
| `input`, `output` | `input_tokens`, `output_tokens` |
| `cacheRead`, `cacheWrite` | `cache_read_input_tokens`, `cache_creation_input_tokens` |
| `totalTokens`, `reasoningTokens` | `total_tokens`, `reasoning_tokens` |
| `premiumRequests` | `premium_requests` |
| `orchestration.{input,output,cacheRead}` | `orchestration.{input,output,cacheRead}` |
| `cttl.{ephemeral5m,ephemeral1h}` | `cache_write_ttl.{ephemeral5m,ephemeral1h}` |
| `server.{webSearch,webFetch}` | `server_requests.{webSearch,webFetch}` |
| `cost.{input,output,cacheRead,cacheWrite,total}` | `cost.{input,output,cacheRead,cacheWrite,total}` |

Token/request counts must be nonnegative integers representable as i64. Native
premium-request and cost components may be finite nonnegative fractional numbers.
Zero is retained; absent components remain absent; invalid components are
individually diagnosed, never string-coerced. No currency or billing-grade
accuracy is inferred from native cost fields.

OMP's input bucket is non-cached input; reasoning is a subset of output; native
totalTokens can include orchestration. Consequently the mapper neither sums the
buckets nor exports guessed `gen_ai.usage.*` totals. Reported totals are retained
verbatim even if inconsistent with components. Repeated snapshots/branches are
not deduplicated and must not blindly be summed as independent billable calls.

## Proof and remaining boundaries

`crates/adapter-omp/tests/fixtures/native.jsonl` is synthetic, authored from public
source structure; no private session payloads were read or copied. The regression
suite covers shared conformance, metadata/content separation, split-batch
invariance, title UTF-8 framing, parent identity, structured tools/references,
measured usage, invalid counts/time, custom/control separation and safe failures.

PM-run proof after coordinated workspace registration/lockfile resolution:

```sh
cargo test --locked -p unisphere-adapter-omp --test mapping
cargo clippy --locked -p unisphere-adapter-omp --all-targets -- -D warnings
cargo fmt --all -- --check
```

These are requested proof commands, **not a claim that the worker ran them**.
The PM also owns source-purity/architecture checks and real composed SDK/CLI
export proof. No persisted CLI resume, active-leaf reconstruction, history
migration, late-update reconciliation, sidecar traversal, source finality or
complete-session capture is claimed. Native persistence itself may truncate
unsigned strings or remove raw subprocess events before this mapper receives
bytes; mapping cannot recover that missing data.

Schema basis: public Oh My Pi `packages/coding-agent/src/session/session-entries.ts`,
`session-title-slot.ts`, `session-persistence.ts`, `blob-store.ts`,
`session/messages.ts`, `tools/output-meta.ts`, `packages/ai/src/types.ts`,
`packages/catalog/src/types.ts` and `packages/agent/src/compaction/messages.ts`.
