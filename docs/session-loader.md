# Filesystem session loader

`unisphere-loader-jsonl::FileSessionLoader` implements the core `SessionLoader`
port. It is stateless (`Default` and `new()`); configuration and checkpoints are
supplied on each call. It does not parse JSON, interpret Claude content, open
attachments or sidecars, select an adapter, encode output, or persist cursors.

## Explicit scope

The initial filesystem loader supports Unix only. On other platforms **both**
`list_sessions` and `read_batch` return `PipelineErrorKind::Unsupported`
(`UNI-UNSUPPORTED`) before validation or I/O. This restriction does not apply to
pure adapters, core data types or output encoders operating on supplied data.

- SDK paths must be absolute UTF-8 paths. Relative or non-UTF-8 roots/session
  paths return `InvalidInput`; candidate paths are never converted lossily.
- `list_sessions(SourceScope { root, max_sessions })` requires an explicit **leaf
  project directory**. It returns only immediate regular files with the exact
  `.jsonl` extension, sorted by native path. It does not recurse, consult HOME or
  discover default storage locations. Selecting a parent directory may therefore
  return an empty list; select the actual directory containing the session files.
- Directory and symlink entries are skipped. A symlink selected as the root is
  rejected. Zero `max_sessions` is invalid; too many candidates return
  `ListingLimit`, never a silently truncated list.
- `read_batch` reads the explicitly selected regular file; listing is not a
  prerequisite and no extension restriction is imposed on that selected file.
  Unix `O_NOFOLLOW` rejects final symlinks, including leaf replacement during
  open. `O_NONBLOCK` prevents a FIFO from waiting for a writer before its file
  type is checked. Directories and special files are rejected.

**Trust boundary:** ancestor directories are caller-selected and trusted.
Directory listing is not an atomic snapshot, and this is not a hostile-filesystem
sandbox: concurrent directory replacement, ancestor symlinks and hard links are
not a confinement guarantee. No source contents are included in loader errors.

## Batches and checkpoints

```rust
use unisphere_core::{ReadLimits, SessionLoader, SourceScope};
use unisphere_loader_jsonl::FileSessionLoader;

fn inspect(root: std::path::PathBuf) -> Result<(), unisphere_core::PipelineError> {
    let loader = FileSessionLoader::new();
    for session in loader.list_sessions(&SourceScope { root, max_sessions: 100 })? {
        let batch = loader.read_batch(&session, None, ReadLimits::default())?;
        // Map and accept these native records before retaining batch.next_cursor.
        // Resume this exact session with Some(&batch.next_cursor).
        println!("{} complete native records", batch.records.len());
    }
    Ok(())
}
```

`ReadLimits` defaults to 128 physical records, 3,145,728 bytes (3 MiB) per physical record,
and 4,194,304 physical bytes per batch. All limits must be positive;
`max_batch_bytes >= max_record_bytes`. Validation precedes any Unix storage I/O.
Limits are caller-selected, not inferred from file size.

Physical framing uses LF. `NativeRecord.offset` is the original byte offset;
`NativeRecord.bytes` omits only the terminating LF and retains CR. Malformed JSON
and arbitrary non-UTF-8 content are returned unchanged for a separate adapter.
Lines containing only ASCII whitespace consume record and byte budgets and
advance the cursor, but produce no `NativeRecord`.

The file length is observed at the start of each call; reads never chase live
appends past that boundary. All complete records returned in a successful call
fit the selected budgets. The framer and its fixed-size read buffer are bounded
by the explicit limits, not the whole file length.

| Stop | Result |
|---|---|
| Exact observed EOF | `more=false`, `incomplete_tail=false` |
| Incomplete final physical record | `incomplete_tail=true`, `more=false`; cursor stays before that record |
| Record count or batch byte capacity | Prior complete batch, `more=true`; next record remains unconsumed in the returned cursor |
| Physical record exceeds `max_record_bytes` | `RecordLimit` at that record's **start** offset; no batch or next cursor returned |

A capacity stop may precede an unknown/incomplete tail; a following call can then
identify the tail. An all-blank batch can contain no emitted records and still
advance its checkpoint. Do not use returned record count as the EOF indicator.
An incomplete record exactly at the record-byte limit is retained; if appending
its LF would exceed the limit, that later call returns `RecordLimit`.

After a `RecordLimit` error, retry **the previous caller-owned cursor** (or `None`
for an initial read) with a larger `max_record_bytes` and compatible
`max_batch_bytes`. A failed call accepts nothing, even if it encountered smaller
complete records first. Retrying recovers those records as well as the oversize
one. CLI equivalents are `--max-record-bytes` and `--max-batch-bytes`.

`ReadCursor` binds the exact selected path, Unix device/inode identity and an
offset immediately after LF (or zero). A different path/identity, offset past
EOF or non-LF boundary returns `SourceChanged`. An unavailable resume identity
returns `Unsupported`; the loader never silently resets it. Removal, replacement,
or observable truncation during a call also invalidate that call. Starting again
with `None` after inspecting a changed source is an explicit caller decision.

**Detection ceiling:** the cursor contains no source history or content hash.
Same-inode rewriting, truncation above the checkpoint, truncation followed by
regrowth, inode reuse and edits after final observation cannot be reliably
identified. The supported input model is append-only regular files with stable
trusted ancestors. This is not a source journal, immutable snapshot or
exactly-once delivery mechanism. Retain a checkpoint only after downstream
acceptance; the SDK collection service coordinates that boundary. Repeatedly
polling an unchanged partial tail will not make progress: wait for an external
append signal or your own scheduling policy when `more=false`.

## Immutable query snapshots

`FileSessionLoader::read_query_snapshot` is the separate full-source path used by
the local query provider. It reuses the same LF framer, no-follow regular-file
checks, fixed observed-end boundary and before-publication identity checks as
`read_batch`, but it does not return or accept an append cursor.

The call must observe the complete file within one `ReadLimits` budget. A record
or aggregate limit, an incomplete tail, replacement or truncation fails the call;
no partial query input or checkpoint is returned. Empty files are valid empty
views. Nonblank physical records become query records with their native byte
offsets. Blank records do not become invented events, but their bytes still
participate in the revision and byte budget.

The revision is `sha256:` plus 64 lowercase hexadecimal digits over the literal
domain `unisphere.query.jsonl.v1\0` followed by every physical source byte in
order, including LF delimiters, CR and blank records. The path is excluded, so a
relocated byte-identical source retains the same revision. Any observed byte or
framing change changes the revision. This is a bounded replacement-view identity,
not append progress, retained history, exactly-once ingestion or session finality.

The existing `read_batch` cursor behavior is unchanged. Collection callers still
retain only an accepted `next_cursor`; query callers retain the source revision in
their immutable view and reopen the source when that evidence changes.

## Proof

`cargo test -p unisphere-loader-jsonl` exercises synthetic temporary files:
nonrecursive bounded listing, non-UTF-8 and invalid input rejection, raw framing,
blank-line budgets, retained batch boundaries, oversize retry, split-UTF-8 tails,
EOF/no-spin behavior, incompatible cursors, replacement/truncation and
nonblocking final-symlink/FIFO refusal. A conditional non-Unix test specifies
fail-fast behavior; a Unix run is not evidence of executing that platform lane.
No tests read private native session stores.

Non-UTF-8 root and `SessionRef` rejection is exercised without creating those
paths. Directory-candidate rejection requires a filesystem that permits
non-UTF-8 names. If fixture creation returns `EILSEQ`, that candidate case emits
`NOT EXERCISED` (visible with `-- --nocapture`) instead of claiming loader
coverage; every other fixture creation error fails the test. A passing run on a
filesystem that refuses such names does not prove candidate rejection. Use a
filesystem accepting non-UTF-8 names to exercise that branch.
