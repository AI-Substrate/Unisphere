# Cursor transcript adapter

`unisphere-adapter-cursor::CursorAdapter` implements the pure `SessionAdapter`
port. `DESCRIPTOR.id` and `CursorAdapter::name()` are **`cursor-transcript`**.
The mapper accepts only supplied `SessionRef` and `NativeRecord` values; it never
opens a file, expands a location, reads a clock/environment, fetches an attachment,
queries SQLite, or writes output. The application owns registration and export.

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

- **Cursor IDE SQLite** (`state.vscdb`, `cursorDiskKV`, composer/bubble snapshots)
  is a distinct source representation. This JSONL delivery defines no
  `CursorIdeAdapter`, `IDE_DESCRIPTOR`, SQLite reader or fake byte offsets.
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

## Verification handoff

Regression tests and fixtures are authored in `crates/adapter-cursor`; they were
**not executed by the worker**. The PM owns formatting, dependency/lockfile
integration and coordinated proof. After integrating the package in the workspace:

```sh
cargo test -p unisphere-adapter-cursor
cargo run --locked -p unisphere-testkit --bin unisphere-arch-check
```

The PM must also register `DESCRIPTOR` and `CursorAdapter` together and run actual
SDK/CLI export over the synthetic transcript in both content modes. Unit tests
alone do not prove that application composition. The regression cases defend
outer-role interpretation, content privacy, absent facts, physical duplicates,
control-record separation, structured tools, malformed inputs and replay.
