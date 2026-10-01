# VS Code Copilot adapter

`unisphere-adapter-vscode-copilot` exports `VsCodeCopilotAdapter`, implementing
both `SnapshotAdapter` and `QueryAdapter`, plus `DESCRIPTOR` with ID
`vscode-copilot` and query policy `QUERY_POLICY_VERSION`. It consumes bounded
`NativeSnapshot`
values supplied by a loader; it does not discover files, read a database, expand
an environment variable, observe a clock, dereference a URI, or write output.
The application composition root owns SDK/CLI registration, loading, revision
manifests, checkpoints and the OTLP writer.

## Query inspection

`QueryAdapter::inspect` accepts only caller-supplied `NativeQueryInput::Snapshot`
values. It validates the supplied `SourceEvidence` against the snapshot path and
revision, applies `journal::reduce` before interpreting a journal, and returns the
shared core `InspectedSource`/`SourcePartition`/`Observation` types. It performs no
discovery or I/O. The registration must use
`vscode-request-containment-v1` (`QUERY_POLICY_VERSION`) for both document and
journal representations.

The policy creates one source-local partition. A native non-empty `sessionId`
establishes `Conversation` plus `NativeContainment`; without one the partition is
`SourceOnly` with unavailable membership, and no conversation ID is invented.
Partition identity uses the native session ID when present and the supplied source
revision otherwise. Snapshot source references always retain the exact revision and
logical key (`document#/requests/...` or `journal:reduced#/requests/...`). The
reduced journal exposes only the final supplied revision: overwritten, truncated and
deleted requests or parts do not become observations.

Each persisted request is a versioned containment boundary. Its user message,
assistant response and serialized tool parts carry the same native `requestId` turn
key; a request missing that ID receives only a revision-local `request-index:N`
containment label, never a fabricated native record or conversation ID.
`isSystemInitiated: true` is `RequestMarker::Injected`, not an initiating user turn.
Missing/null responses do not create an assistant observation. Native request,
response and tool IDs remain source-local evidence; SDK reconstruction owns logical
turns, pairing and cross-source identity.

`toolInvocationSerialized` yields a `ToolCall` with its exact `toolCallId` and
`toolId`. The closed known-name mapping currently normalises `read_file` to
`file-read`, file mutation tools to `file-write`, terminal tools to `shell`, and
VS Code search tools to `search`; unknown names retain no guessed family. Native
`isComplete`, `isConfirmed`, confirmation kind and `subAgentInvocationId` are kept
as structured lifecycle facts. `resultDetails`, when present and authorised by
`ContentAccess`, is a `ToolResult` with `Outcome::Unknown`; confirmation/completion
never becomes success. Original LM arguments, exit code and duration are
`NotCaptured`. An absent `resultDetails` is not an empty successful result.

Message text, reasoning, session titles and tool result details are retained only
when `ContentAccess` permits the corresponding field. Otherwise observations carry
`SensitiveOmitted`, not payload text or a fabricated empty value. Unsupported
response kinds remain `NotSupported` availability, and unknown session versions
return an unsupported source with no guessed request observations.

Repository association uses only evidence already supplied by the provider or the
v3 native `workingDirectory` when it is a valid local `file://` URI. Such a URI is
recorded as `AssociationBasis::NativeCwd`. Provider-supplied
`VerifiedWorkspaceMetadata` associations are preserved when their partition agrees.
`repoData.remoteUrl`, prompt mentions, equal basenames and the opaque
`workspaceStorage/<hash>` directory name never become a cwd. With no valid
association, the partition stays unassociated and carries `AvailabilityCode::Unassociated`.

## Native authority and supported dialects

The schema reference is VS Code commit
[`08d4889f9ec4a1685d257b9b95de036c8e1ce1e5`](https://github.com/microsoft/vscode/tree/08d4889f9ec4a1685d257b9b95de036c8e1ce1e5),
identified by the researched VS Code 1.135.0 bundle:

- [Serializable request/session types and legacy normalization](https://github.com/microsoft/vscode/blob/08d4889f9ec4a1685d257b9b95de036c8e1ce1e5/src/vs/workbench/contrib/chat/common/model/chatModel.ts#L1806-L2240).
- [Persisted request fields and v3 storage schema](https://github.com/microsoft/vscode/blob/08d4889f9ec4a1685d257b9b95de036c8e1ce1e5/src/vs/workbench/contrib/chat/common/model/chatSessionOperationLog.ts).
- [Mutation log replay](https://github.com/microsoft/vscode/blob/08d4889f9ec4a1685d257b9b95de036c8e1ce1e5/src/vs/workbench/contrib/chat/common/model/objectMutationLog.ts#L350-L506).
- [Tool serialization and usage scope definitions](https://github.com/microsoft/vscode/blob/08d4889f9ec4a1685d257b9b95de036c8e1ce1e5/src/vs/workbench/contrib/chat/common/chatService/chatService.ts).

All committed fixtures are authored synthetic data, not captured private
sessions. Structural research observed populated v3 documents and journal kinds
0/1/2. Legacy versions and kind 3 are source-defined, not claimed as observed in
that bounded local sample.

| Dialect | Supported interpretation | Fidelity classification |
| --- | --- | --- |
| Unversioned v1 whole JSON | Session identity/creation time; legacy string or parsed request text; persisted response Markdown | **Transformed partial projection**. Missing IDs/times remain absent, unlike VS Code's runtime-generated legacy defaults. |
| v2 whole JSON | v1 projection plus `computedTitle` under content opt-in | **Transformed partial projection**. Parser annotations, reference resolution and extension/UI state are not reconstructed. |
| v3 whole JSON | Request/response identities, native milliseconds, requested model, agent ID, text/reasoning, serialized tools and scoped usage | **Transformed partial projection**. Editing history, drafts, queued requests and sidecars are not a complete conversation archive. |
| Native mutation journal | Reduce all kind 0/1/2/3 operations in supplied order, then apply the versioned document projection | **Final-revision projection**. Intermediate states and overwritten/deleted content are intentionally absent; lines are not chat events. |
| Unknown session versions | Root provenance plus `UnsupportedRecord`, no guessed request mapping | **Unsupported**. No forward-compatible schema claim. |
| Copilot caches/SQLite, extension logs and editing sidecars | Not accepted by this adapter | **Unsupported**. Use an explicitly implemented dialect rather than relabeling cache rows as a transcript. |

## Storage and revision semantics

The document representation contains exactly one record with key `document`.
A journal contains the loader's ordered `journal:<ordinal>` records. The loader
owns framing and revision identity; the mapper neither sorts operations nor
interprets ordinals as byte offsets.

Native `.jsonl` is preferred over coexisting `.json` when VS Code log storage is
enabled. This is not an mtime comparison. A readable malformed journal does not
fall back to JSON. Selection belongs to the explicit caller/loader, not to this
pure mapper. VS Code documents flat JSON as pre-1.109 and log storage as >=1.109;
that chronology does not establish compatibility with every historical build.

Replay uses the exact native wire names `kind`, `k`, `v`, `i`:

| Kind | Meaning |
| --- | --- |
| `0` Initial | Replace the complete state with `v`; a later Initial resets earlier state too. |
| `1` Set | Replace/create the final property at `k`; never create missing intermediate parents. Empty `k` is a no-op. |
| `2` Push | Use the array at `k` or create it for a missing/falsy leaf; optional `i` sets its length, then optional `v` appends an array. This replaces a suffix, not `splice(i, deleteCount, ...)`. |
| `3` Delete | Assign native `undefined`: omit object properties from the JSON projection and leave null array slots without shifting later indices. Empty `k` is a no-op. |

Mixed string/integer paths and canonical decimal string array indices are
supported. Sparse extension holes are projected as JSON nulls. Their cumulative
allocation budget is `sum(raw record byte lengths) / 4` synthesized slots, charged
before resizing, including replacements later discarded by Initial/truncation.
This conservative budget prevents a tiny operation with a giant index from
allocating an unbounded array; exceeding it fails the whole mapping with
`BatchLimit`, not partial reconstruction. Loader limits still bound the supplied
raw representation. This is not a promise that replay's heap use equals raw
JSON byte size.

Unknown operations, malformed JSON/UTF-8, missing Initial/value/path, missing
intermediate parents, invalid array indices, non-array Push values and non-array
truthy targets fail the entire journal with a fixed `InvalidData` error. Empty
journals fail too. Incidental JavaScript behaviors outside serialized chat data
(custom array properties, negative/noninteger path numbers, empty-path Push,
missing Set values and object Push targets) are not emulated. There is no prefix
salvage, timestamp-based ordering or invented journal event timestamp.

## Projection and metadata

Every output record uses `unisphere.session.record` and numeric
`unisphere.profile.version = 1`. One session record precedes each request's user
record and, when a native response exists, assistant record. Empty request lists
still emit session provenance. Missing/null responses do not create assistant
turns. This preserves structural order and repeated native IDs without claiming
unique global identities or deduplication.

Required provenance is `unisphere.source.adapter`, `.path`, `.key`, `.revision`,
`.format`, and `.kind`. Formats are `json_document` or `json_journal`. Kinds are
structural `session`, `message`, `response` (or `unknown` for a malformed request),
not journal operation numbers. Keys identify the root or logical locations such
as `document#/requests/0/response` and
`journal:reduced#/requests/0/response`; the latter denotes the reduced state,
not the last physical mutation. **`unisphere.source.offset` is always absent.**

| Native data | Output and interpretation |
| --- | --- |
| `sessionId` | `gen_ai.conversation.id` and `unisphere.source.session.id`; never inferred from filenames. A supplied session selection must match this native ID or mapping fails with `InvalidInput`. |
| `requestId`, `responseId` | `unisphere.message.id`; an assistant's `unisphere.source.parent.id` is its native request ID. |
| `creationDate`, `timestamp`, `responseTimestamp` | Corresponding record's native epoch milliseconds converted with checked multiplication to nanoseconds. No response fallback to request/session time. Invalid/overflowing times yield `InvalidTimestamp` and no timestamp. |
| `modelId` | `gen_ai.request.model`, **not** `gen_ai.response.model`: a selected `auto` model is not an observed serving model. |
| `agent.id`, `responderUsername` | `unisphere.vscode.agent.id`, `.responder_username`; not a provider guess. |
| `isHidden`, `hiddenFromTranscript`, `isSystemInitiated` | Native booleans under `unisphere.vscode.request.*`; hidden requests are retained, not silently discarded. |
| `isCanceled`, `modelState.value` | Native cancellation flag/state code under `unisphere.vscode.response.*`; no invented final-session claim. |
| Tool `toolId`, `toolCallId`, `subAgentInvocationId`, `isComplete`, `isConfirmed` | Ordered `unisphere.vscode.tools` metadata array with `name`, `id`, optional `subagent_id`, `is_complete` and native boolean `is_confirmed` or `confirmation_kind`. Confirmation kind is not translated into execution success. |

No trace/span IDs, durations, provider attribution or completion timestamps are
invented. Metadata-only is not anonymization: paths, model/agent names and IDs can
still identify users or projects.

### Native usage, never inferred standard totals

Only response records carry `unisphere.vscode.usage`. Each retained field has
`{"value": ..., "scope": ...}`; this is an adapter-specific extension, not an
expansion of the Claude profile's closed `unisphere.usage.*` namespace.

| Persisted field | Scope | Interpretation |
| --- | --- | --- |
| `promptTokens` | `latest_model_call` | Native latest-call prompt count, not all calls in the turn. |
| `completionTokens` | `native_response_counter` | Persisted response completion counter; no inferred delta or whole-session meaning. |
| `copilotCredits` | `response_cost` | Native cost scoped to the response; summing turns is not established session cost. |
| `sessionCopilotCredits` | `session_cumulative` | Backend-reported whole-session cost, not a per-response increment. |
| `modelTotals` | `whole_turn_including_subagents` | Ordered native per-model `model`, `inputTokens`, `cachedTokens`, `outputTokens` components; sums include subagent calls. Does not identify one serving model for the assistant record. |

Counts must be nonnegative integers fitting `i64`; credits may be finite
nonnegative numbers encodable by the common writer. Invalid components/rows are
omitted with `InvalidField`; missing counters are not zero-filled. Counts are not
summed, cache-adjusted, deduplicated or converted to `gen_ai.usage.*`. Additional
fields such as output-buffer capacity and prompt breakdown labels/percentages
are not token totals and receive unsupported-field diagnostics rather than being
guessed as usage.

## Content and explicit losses

Default `MappingOptions` emits no bodies and no message text, reasoning,
invocation display, tool input/output details, titles or unknown payloads.
`unisphere.content.omitted=true` plus `ContentOmitted` identifies suppressed
projected content. Structural diagnostic keys never embed arbitrary unknown
property names or source payloads.

With `include_content`:

- Native request string/`message.text` becomes
  `{"role":"user","parts":[{"type":"text","content":"..."}]}`.
- Response Markdown `value` (or `markdownContent.content.value`) becomes a text
  part. Thinking `value` is retained as string or ordered string array in a
  reasoning part; Markdown trust/rendering metadata is not a text guarantee.
- `toolInvocationSerialized` becomes an explicit **extension part**
  `type: "unisphere.tool_invocation"`, retaining IDs/status and supplied
  `invocationMessage`, `originMessage`, `pastTenseMessage`, `resultDetails`,
  `toolSpecificData` with their native JSON types. These are stored UI details,
  **not** fabricated LM arguments or a complete tool-call-response pair. The
  native serialized interface explicitly does not retain original parameters.
  Nested details can contain paths, command text, encoded data and partial
  results; opt-in exports them as supplied, never dereferences them.
- v2 `computedTitle` / v3 `customTitle` is an `unisphere.session` extension body.
- Unsupported response parts retain a generic `unisphere.unknown` part and native
  discriminator plus `UnsupportedPart`, **never** their arbitrary payload.

Editing/undo parts, citations/references, attachments/variable values, queued
requests, draft input, rich extension/UI state and unknown fields have no complete
semantic reconstruction. Unknown top-level/request fields are flagged at the
safe enclosing structural key; known response parts still undergo the documented
lossy text/tool projection. Unknown request/response optional types receive
`InvalidField` without leaking their values. No filesystem sidecar is opened.

A journal's deleted/revised content disappears from this final-revision
projection. The composition service's scoped replacement manifest is required
for consumers to remove old records; append-only ingestion of successive exports
will duplicate/revive stale views. The descriptor's delayed-reconciliation flag
means revision replacement is available, **not** autonomous watching, persistent
history, exactly-once sink delivery, CLI-persisted resume or source finality.

## Location hints and proof boundary

The descriptor advertises symbolic workspace-storage locations:

- macOS: `~/Library/Application Support/Code/User/workspaceStorage/*/chatSessions/`
- Linux: `~/.config/Code/User/workspaceStorage/*/chatSessions/`
- Windows: `%APPDATA%/Code/User/workspaceStorage/*/chatSessions/`

`*.json` and `*.jsonl` are distinct native representations, both requiring
snapshot loading. These hints do not discover installations and are not an
exhaustive inventory of Insiders, portable/remote builds, custom user-data roots
or global/empty-window session stores. Windows is a location hint, not a claim
that this Unix export pipeline runs on Windows.

`tests/mapping.rs` and `tests/query.rs` use synthetic fixtures. Mapping coverage
includes native privacy, usage scopes, identity selection, journal replay bounds,
atomic errors and unsupported schemas. Query coverage includes v1/v2/v3 request
containment, exact current-revision journal provenance, serialized tool lifecycle
facts with unknown execution outcome, content omission, injected requests and
workspace-association evidence floors. The designated PM runs
`cargo test --locked -p unisphere-adapter-vscode-copilot --test query` after fleet
delivery, then the plan's composed native parity proof. Authored regressions are
not a passing execution claim; adapter-only evidence does not establish SDK/CLI
query behavior.
