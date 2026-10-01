# Cursor adapters

`unisphere-adapter-cursor::CursorAdapter` implements the pure `SessionAdapter`
port. `DESCRIPTOR.id` and `CursorAdapter::name()` are **`cursor-transcript`**.
`CursorIdeAdapter` separately implements `SnapshotAdapter`, with
`IDE_DESCRIPTOR.id` **`cursor-ide`**. Both accept supplied data only: no file
opening, location expansion, clock/environment reads, attachment fetching,
SQLite queries or output writes. The application owns registration and export.

## Native schema basis

This dialect is Cursor's **agent-transcript JSONL**, not the IDE database or CLI
blob store. Schema facts were read from the installed Cursor **3.17.8**, distro
`d5c0e77a0214208f36b56d42e8e787de88d02ea4`, in
`out/vs/workbench/workbench.desktop.main.js`:

- `GVu` / `bqg`: messages are `{role, message:{content:[...]}}`. Content contains
  `{type:"text", text}` and `{type:"tool_use", name, input}`. The role is on the
  **outer record**, unlike Claude Code. There is no required tool-call ID.
- `HVu`: upstream reasoning is merged into text; images/files become textual
  markers; tool-call IDs and tool-result payloads are discarded before JSONL.
- `fqg`: optional leading `{type:"metadata", metadata:{overview}}`.
- `VVu`: optional `{type:"turn_ended", status, error?}`; statuses are `success`,
  `error`, `aborted`. This is a turn status, not session finality.
- `UVu` / `GVu` filter native summary/system messages; `bqg` may discard the first
  of two leading user messages. The adapter cannot restore those omitted inputs.
- `TranscriptStore.writeFromStateFull` and `writeFromStateIncremental` can replace
  a transcript, including falling back from incremental append to a full write.

These are structural implementation observations, not private session fixtures
or a published stable Cursor schema guarantee. Minified symbol names are specific
to that distribution. The committed fixture is synthetic. No private store or
transcript payload was read or copied to implement this mapper.

Typical symbolic locations:

- `~/.cursor/projects/<project>/agent-transcripts/<id>/<id>.jsonl`
- `~/.cursor/projects/<project>/agent-transcripts/<parent>/subagents/<id>.jsonl`
- Legacy flat `~/.cursor/projects/<project>/agent-transcripts/<id>.jsonl`

Hints describe native layouts, not recursive discovery capability or installation
detection. The current shared JSONL export loader is Unix-only. The pure mapper is
platform-independent. Neither conversation IDs nor parent IDs are inferred from
these paths.

## Projection and content policy

Each physical record produces `unisphere.session.record` with integer profile
version `1`, adapter `cursor-transcript`, supplied source path and byte offset.
The kind is the outer native `role` for messages, or `type` for control records.
Physical duplicates stay separate. Equal input and options produce equal records
and diagnostic order, including when a batch is split and replayed.

| Native fact | Projection | Fidelity |
| --- | --- | --- |
| Outer `role`: `user`, `assistant`, `tool` | `unisphere.message.role` | Preserved; never copied from a nested role or guessed from payload text |
| `message.content[].type=text` | `{type:"text", content}` part | Text preserved verbatim as supplied; already-merged reasoning cannot be separated |
| `message.content[].type=tool_use` | `{type:"tool_call", name, arguments}` | Native structured input preserved; no guessed call ID/result link |
| `metadata.overview` | `{type:"unisphere.cursor.metadata", overview}` body | Preserved under opt-in; not an ordinary turn |
| `turn_ended.status` | `unisphere.cursor.turn.status` | Valid native status preserved; not a session completion claim |
| `turn_ended.error` | `{type:"unisphere.cursor.turn_ended", error}` body | Preserved under opt-in, never in diagnostics/metadata |
| Timestamp/model/usage/message/session/call IDs | Absent | Not serialized by this native writer; never guessed from filenames, text or similarly named unverified fields |
| Tool results, structured attachments, original reasoning boundaries | Unavailable from native JSONL | Native writer already drops or flattens them |

The common message body is `{role, parts:[...]}`; metadata/turn-status bodies use
explicit extension types and have **no message role**. The new dialect-specific
attribute `unisphere.cursor.turn.status` is a string with the three statuses above.
Other common attributes and structured part meanings follow the
[telemetry profile](telemetry-profile.md).

Default `include_content=false` produces **`body=None` on every record**, including
overview and error records. Present supported content is marked with
`unisphere.content.omitted=true` and `ContentOmitted`. Opt-in retains only the
supported fields, not arbitrary native envelopes. Metadata-only is not
anonymization: explicit source paths and native role/type discriminators remain.

Invalid JSON/UTF-8 fails the entire batch with `InvalidData` at the physical byte
offset; the fixed error never contains source payload or parser text. Valid JSON
with malformed known fields emits `InvalidField`. Unsupported record kinds remain
provenance-only with `UnsupportedRecord`. Unsupported parts emit `UnsupportedPart`
and, under opt-in, only `{type:"unisphere.unknown", native_type}`; their payload is
not copied. Malformed parts do not erase valid siblings. Unknown extra fields are
not promoted to telemetry facts.

## Unsupported dialects and completeness ceilings

- **Cursor IDE SQLite** is handled separately by `CursorIdeAdapter` below; passing
  a database, whole document or blob to `CursorAdapter` is not supported.
- **Cursor CLI `store.db`** has mixed JSON/binary blobs. An opaque blob is not
  assumed to be JSON, and there is no semantic blob codec in this adapter.
- Legacy `.txt`, whole `.json` transcript exports, hook-event JSON, and raw
  provider messages are not this dialect. In particular, a Claude-shaped
  `{type:"assistant",message:...}` record is not accepted as Cursor JSONL.
- Compaction/custom/control records are never promoted to turns because they
  contain a message-like payload. A supplied native `type` takes precedence over
  `role`, even when malformed.
- No usage components or totals can be measured from the supported native schema.
  No zero defaults, cumulative sums, token estimates or standard usage claims.
- Cursor can rewrite its transcript. Caller-owned byte cursors are safe only
  while the source really remains append-only. Retention, in-place rewrites,
  truncate-and-regrow, deleted records and prior summaries are not reconciled.
  For a changed transcript, consumers must re-export the full explicit source and
  replace their own view; this adapter supplies no replacement transaction.
- This is a projection, not complete-session capture, a lossless archive, a
  reconstructed conversation, an inference span, or a durable exactly-once sink.

## Cursor IDE snapshot adapter

`CursorIdeAdapter::map_snapshot` accepts a bounded, validated `NativeSnapshot`
whose format is `SqliteKeyValue { table: "cursorDiskKV" }`. Any other table or
representation is rejected with `Unsupported`; no BLOB codec is guessed.
The shared snapshot loader, not this crate, owns storage consistency and limits.

### Authoritative spine, identity and provenance

The installed `conversationSearchMain.js` (`ei` parser and ordered bubble reader)
establishes `composerData:<id>`, matching JSON `composerId`, and
`fullConversationHeadersOnly` array order. The corresponding bubble key must be
exactly `bubbleId:<composerId>:<bubbleId>`. A supplied bubble JSON `bubbleId` must
agree, and its numeric `type` must agree with its header. `_v >= 2` is required on
composers, as in the installed parser. No search-index `slice(-2000)` export cap is
copied. The caller's explicit `session_id` selects a composer; otherwise composers
are visited in lexical native-key order, with each spine in its declared order.

One composer record precedes its mapped bubbles. All records use numeric profile
`1`, adapter `cursor-ide`, explicit path, exact source key, supplied revision,
format `sqlite_key_value`, and verified `unisphere.source.session.id` plus
`gen_ai.conversation.id`. Kind is `composerData` for the composer or the native
bubble type rendered as a string (`"1"`, `"2"`, or an unsupported integer).
**`unisphere.source.offset` is absent.** Message identity is the bound bubble ID,
not row position, a timestamp, or a cross-session guess.

Missing rows, malformed JSON/UTF-8, inconsistent IDs/types and duplicate spine
references produce key-based diagnostics and no invented substitute messages.
Unreferenced bubbles produce `UnsupportedRecord`, not alternate-branch turns.
Duplicate database keys are rejected as ambiguous `InvalidData`. Selected missing
composers produce an empty projection plus a keyed diagnostic; an empty unselected
snapshot returns an empty projection. The PM-owned full-replacement revision
manifest makes those empty results observable. No mapper state is retained across
revisions, and no persistent merge/deduplication/deletion transaction is claimed.

`composerHeaders` is a separate table, **not** a KV prefix. The frozen single-table
snapshot contract supplies no cross-table header rows, so this mapper performs no
fictional header join. It binds the composer key/JSON and referenced bubble keys/
JSON actually supplied. Header archive/subagent flags and their cross-table
consistency are not asserted. `conversationMap`, alternate branches and encrypted
conversation-state blobs are not reconstructed.

### IDE fields and interpretation

Additional installed Cursor 3.17.8 symbols resolve some research unknowns:
`aiserver.v1.ConversationMessage.MessageType` defines `1=HUMAN`, `2=AI`;
`ComposerCapabilityType` defines `22=SUMMARIZATION`, `15=TOOL_FORMER`. Bubble
creation uses `new Date().toISOString()` and composer constructor `RP` uses
`Date.now()`. The native tool projection returns
`name/toolCallId/params/rawArgs/result/error` in `toolFormerData`.

| Supplied field | Projection / limit |
| --- | --- |
| Bubble type `1` / `2` | `user` / `assistant`; unknown types retain provenance with `UnsupportedRecord` |
| Bubble `bubbleId` / `requestId` / `checkpointId` | Bound `unisphere.message.id`, optional `unisphere.cursor.request.id` and `unisphere.cursor.checkpoint.id`; not trace/span IDs |
| Composer `createdAt` | Checked epoch-millisecond conversion to nanoseconds; no `lastUpdatedAt`/clock fallback |
| Bubble `createdAt` | RFC3339-to-nanoseconds conversion; no numeric-string guessing or composer-time fallback |
| `modelConfig.modelName` / `modelInfo.modelName` | `unisphere.cursor.model_config.model_name` / `unisphere.cursor.model_info.model_name`; observed native selection information, not proven response-model attribution |
| `tokenCount.inputTokens` / `outputTokens` | Exact nonnegative `i64` values in `unisphere.cursor.token_count.input_tokens` / `output_tokens`, with scope `native_bubble_snapshot` when any is retained |
| Composer `usageData` | Nonempty native object under body field `unisphere.cursor.usage_data`, opt-in only, with `UnsupportedPart` because subfield/aggregation semantics are unproven |
| Bubble `text` / `richText` / `thinking.text` | Opt-in text / `unisphere.cursor.rich_text` / reasoning parts; no rich-text parsing or attachment fetch |
| `toolFormerData.name`, `params` (else `rawArgs`) | Opt-in structured tool call, optional `toolCallId`; raw argument strings are not parsed or repaired |
| `toolFormerData.result` / `error` | Opt-in structured response / `unisphere.cursor.tool_error`; not a guessed successful or final result |
| Nonempty `toolResults` object entries | Opt-in `unisphere.cursor.tool_result` parts retaining the native object, with `UnsupportedPart`; no guessed flattened legacy tool schema |
| `skipRendering`, `isDisplayOnly`, `isSimulatedMsg`, `isPlanExecution` | Same-cased native fields under `unisphere.cursor.*`; display flags do not silently remove source bubbles |
| `capabilityType` | Native integer `unisphere.cursor.capability_type`; summaries (`22`) and simulated bubbles have extension control bodies with no ordinary message role |

The native token-counter constructor includes zero defaults. Preserving an
explicit zero means only that the stored field is zero, **not** that a provider
measured zero tokens. Missing counters remain absent; repeated values are not
summed, differenced or deduplicated. Unknown counter keys emit `UnsupportedPart`
and are not mapped. Negative/fractional/out-of-range values emit `InvalidField`.
No standard `gen_ai.usage.*` totals or `gen_ai.response.model` are inferred.

Native objects retained only under opt-in remain explicitly native and
semantically unclassified; they are not claims of normalized complete tools or
usage. Attachments/context arrays and additional thinking blocks are unsupported
with diagnostics, never dereferenced. Composer drafts, alternate model
selections, archive flags, arbitrary unknown fields and blob-backed state are not
projected. Metadata-only always returns `body=None`, including tool errors and
unclassified usage objects. Paths, native keys/IDs/model names remain metadata and
can be sensitive; this is not anonymization.

Snapshot mapping diagnoses semantic incompleteness without claiming the snapshot
loader missed bytes. A malformed bubble may leave valid siblings available with
diagnostics. Consumers requiring complete semantic projection must inspect those
diagnostics, not just a successful raw snapshot read or revision digest.

## Query adapter facts

Both adapter types also implement the pure `unisphere_core::query::QueryAdapter`
port. They accept only caller-supplied `NativeQueryInput`; discovery, source I/O,
repository scoping, cross-source identity, logical turn reconstruction, filtering
and statistics remain outside this crate. Query policy versions are
`cursor-transcript-query-v1` and `cursor-ide-query-v1`.

### Transcript query view

Agent-transcript JSONL is one `SourceOnly` partition with unavailable membership.
Physical byte offsets define versioned source order and provenance only. Messages
retain their outer native role, but native message/session/turn/call IDs and all
timestamps remain absent. A native user role is an initiating-request marker;
`turn_id` remains unavailable for SDK-owned versioned reconstruction. Idless
`tool_use` parts can be retained as message evidence under content access, but do
not become `ToolCall`/`ToolResult` entities and are never paired by adjacency,
name, arguments or text. Tool results, outcomes, exit codes and durations remain
explicitly `not_captured`.

Supported sensitive message parts are retained only when `ContentAccess` permits
the relevant field or content emission. Otherwise the observation carries typed
`sensitive_omitted` availability. Metadata, turn-end and unsupported records stay
source/control observations; they do not manufacture sessions or turns.

### IDE query view

Each key/JSON-validated composer is a `MainSpine` partition with
`ValidatedHeader` membership. The existing composer/header/bubble validation and
`fullConversationHeadersOnly` order are authoritative; database row order,
lexical bubble IDs and timestamps do not reorder the spine. The supplied snapshot
revision and exact native keys remain on every source reference.

Cursor's IDE store is global and may contain multiple projects. A valid composer
therefore remains `unassociated` unless the supplied source evidence contains an
association for that exact partition. Selecting one composer does not silently
associate other store rows. Unselected composers, orphan/alternate bubbles and
malformed referenced rows remain `SourceOnly` branch/control observations with
typed availability; absent referenced keys remain explicit source issues.
Duplicate native keys reject the supplied snapshot as invalid rather than choosing
one row. No repository association is inferred from message text, native keys,
filenames or similarly named metadata.
IDE tool facts become tool entities only when a nonempty native `toolCallId`
exists. Start, result and error facts carry that exact ID so SDK reconstruction
can pair them only within the validated composer partition; this adapter does not
join invocations. Exact registered start names retain separately normalized
families (for example `read_file` → `file-read`); unknown names keep no guessed
family. A native `result` has outcome `unknown`, not assumed success; an explicit
native `error` has outcome `failed`. Exit code, duration and turn ID remain
unavailable. Idless tools stay
message/source evidence rather than receiving a synthetic call identity. Arguments,
results, errors, text and reasoning follow `ContentAccess`; no blob, attachment,
context or raw-argument string is decoded beyond its supported native shape.

## Verification handoff

Regression tests and fixtures are authored in `crates/adapter-cursor`; they were
**not executed by the worker**. The PM owns formatting, dependency/lockfile
integration and coordinated proof. After integrating the package in the workspace:

```sh
cargo test --locked -p unisphere-adapter-cursor --test query
cargo run --locked -p unisphere-testkit --bin unisphere-proof -- native
cargo run --locked -p unisphere-testkit --bin unisphere-arch-check
```

The PM must register both descriptor/runner pairs and run actual SDK/CLI exports
in both content modes. Transcript fixtures use the shared JSONL loader; IDE
fixtures require real synthetic SQLite `cursorDiskKV` rows and the shared snapshot
loader. Unit tests alone do not prove that composition. Regression cases defend
outer-role interpretation, privacy, native absence, duplicates, control records,
structured tools, spine order, identity binding, selection, timestamp/counter
boundaries, revision deletion, malformed inputs and replay.
