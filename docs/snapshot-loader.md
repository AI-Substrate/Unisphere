# Snapshot loader

`unisphere-loader-snapshot::FileSnapshotLoader` implements the core
`SnapshotLoader` port. It reads exactly one explicit `SnapshotRef`; no discovery,
ambient configuration, native session selection, semantic mapping or cursor
persistence. The caller supplies `SnapshotLimits`. `session_id` is carried through
unchanged for the pure mapper, never used to filter storage rows.

## Representations

| Core format | Returned records | Completeness boundary |
|---|---|---|
| `JsonDocument` | One record, key `document`, byte-for-byte document including whitespace | Exactly one syntactically complete UTF-8 JSON value; empty/truncated/invalid input fails |
| `JsonJournal` | One record per LF-terminated JSON value, keys `journal:0`, `journal:1`, … in source order | Every frame must be valid UTF-8 JSON and end in LF; a partial tail or blank/malformed frame fails the entire snapshot |
| `SqliteKeyValue { table }` | All rows of the explicitly named main-database table, sorted by UTF-8 key bytes | One read transaction covers both size inspection and value extraction |

Journal LF delimiters are omitted; CR and other JSON whitespace remain in raw
record bytes. Structural ordinals are not file offsets. An empty journal or table
is a valid empty replacement snapshot with a revision; an empty document is not.
Journal operations are raw inputs to a pure adapter. The loader does not reduce
VS Code patches or project messages; that is `SnapshotAdapter`'s responsibility.

SQLite requires a real ordinary table with stored `key` and `value` columns, a
nonempty UTF-8 TEXT key, and a TEXT or BLOB value. Value bytes are preserved,
including opaque/non-JSON BLOBs and embedded NULs; keys are never coerced from
BLOB/numeric/NULL values. Duplicate keys fail instead of overwriting records.
Views, virtual/shadow tables and generated key/value columns are rejected. The
database must use UTF-8 encoding; UTF-16 is explicitly `Unsupported`, not silently
converted under the wrong byte budget. No native key prefixes or JSON schemas
are interpreted by this crate.

## Bounds and failure behavior

Input validation precedes filesystem or SQLite access. Every returned snapshot
passes `NativeSnapshot::validate(limits)`:

- `max_records` bounds the complete record count, not a resumable page.
- `max_record_bytes` bounds each raw value; LF framing is not value content.
- `max_snapshot_bytes` bounds the sum of native UTF-8 key bytes plus raw value
  bytes, including `document` or journal ordinal keys. A single oversized key fails.
- Exceeding any limit fails the whole read; nothing is skipped or partially
  published. Retrying with larger compatible limits rereads the whole source.

File readers cap growth before copying each buffered chunk. SQLite first checks
`typeof`, `octet_length` and row count incrementally, inside the same transaction
that later returns values. [SQLite's `octet_length` on stored columns](https://sqlite.org/lang_corefunc.html#octet_length)
uses record metadata without loading complete TEXT/BLOB payloads. No
`length(TEXT)`, `CAST(TEXT AS BLOB)`, `SELECT *`, SQL sorting, aggregation or
unbounded result collection is used for this preflight. Sorting happens only
on the bounded Rust records after extraction.

SQLite's connection-local `SQLITE_LIMIT_LENGTH` is set before schema access to
`max(64 KiB, max_snapshot_bytes + 1024)`, saturating at SQLite's supported integer
limit. The fixed floor permits schema rows when the selected data budget is tiny;
data limits remain independently enforced. This caps individual SQLite
strings/BLOBs/rows, not total process RSS or arbitrary schema complexity. SQLite
can reject a structurally oversized row/schema with `BatchLimit` before a more
specific per-value diagnostic is available.

Errors are core `PipelineError` values with no native payload, database message,
SQL text or fictitious byte offset. Invalid shape/partial JSON is `InvalidData`;
count/aggregate overflow is `BatchLimit`; oversized values are `RecordLimit`;
I/O/locking failures are `Read`. No retries or waiting are hidden in the loader.

## Consistency and revision identity

File access is Unix-only, like `FileSessionLoader`. Final symlinks and nonregular
files are rejected using `O_NOFOLLOW | O_NONBLOCK`; FIFOs do not wait for a writer.
Ancestor directories are caller-trusted. Open-descriptor and path metadata are
checked before publication: device/inode, length, mtime and ctime (including
nanoseconds) must still match for JSON files. Replacement, truncation, append,
observed in-place changes or removal return `SourceChanged`, even if the bytes
read before detection happened to form valid JSON. This is detection across the
read, not a filesystem lock or a guarantee against privileged metadata forgery or
mutations after the final check.

SQLite uses [read-only/no-follow open flags](https://docs.rs/rusqlite/0.37.0/rusqlite/struct.OpenFlags.html),
no create/URI mode, disabled trusted-schema/views and a deferred read transaction.
Only the caller-trusted parent directory is canonicalized before SQLite opens the
database: ancestor symlinks work, while the final filename remains no-follow.
There are no database mutation statements, checkpoint requests or `immutable=1`
shortcuts. Committed WAL content participates normally; writes committed after
the reader's transaction snapshot do not mix old sizing with new values.
Concurrent commits are valid, not `SourceChanged`; path removal/replacement is
still checked against the held descriptor. SQLite manages its normal WAL/SHM
locking protocol; a read-only database connection is not a promise that SQLite
never touches shared-memory sidecars. A writer/permission/recovery condition
preventing a consistent read fails safely.

Revision strings are `sha256:` followed by 64 lowercase hex digits. SHA-256 hashes
an unambiguous, versioned representation:

1. Literal bytes `unisphere.snapshot.v1\0`.
2. A length-prefixed format tag: `json_document`, `json_journal` or
   `sqlite_key_value`; SQLite additionally includes the length-prefixed table name.
3. Record count as unsigned 64-bit big-endian.
4. Each ordered key, then raw value, each prefixed with its unsigned 64-bit
   big-endian byte length.

Documents/journals retain source order; SQLite uses raw key byte order,
independent of row insertion order or database collation. Any key/value change,
record removal or journal reordering changes the representation. Mandatory
journal LF is implicit in the format and record boundaries. Whitespace/CR changes
are not normalized. Source path, selection, timestamps and SQLite page layout
are excluded: relocation and mapper selection preserve content identity. Revision
identity is not append-only progress, session finality or complete native fidelity.

## Synthetic regression surface

PM-owned proof command, after PM-managed dependency/lockfile update:

```text
cargo test -p unisphere-loader-snapshot --all-targets --locked
```

`tests/filesystem.rs` exercises explicit-source rejection, exact raw bytes,
structural order, complete LF boundaries, byte/count budgets, empty replacements,
symlinks and FIFO rejection. `tests/sqlite.rs` covers key/value preservation,
revision stability/change, Unicode/NUL byte accounting, oversized TEXT/BLOB/key
preflight, invalid schemas/types/keys, safe corrupt-data errors and live WAL
visibility. Internal boundary regressions place a file mutation between read and
publication and commit a WAL write between size preflight and extraction. They
exercise the real filesystem/SQLite boundaries without timing-dependent races.
These authored checks are not an execution claim; the PM records observed proof
against the delivered commit under guide check `vd-0002`.
