# Prep canonical tables

## Motivating question

How do I turn every local agent session into research-ready tables once, then keep them current by reading only what changed?

## Run it

`unisphere prep --target DIR` folds native sessions into canonical metadata tables under `DIR`. A re-run reads only records appended since the committed cursor; an unchanged source costs one `stat` and a run where nothing changed commits nothing.

Roots are explicit and reported:

- Every harness's adapter-catalogue default root, labelled `default`, unless `--no-default-roots`. `--harness H` (repeatable) restricts the run to those harnesses.
- Every `--root HARNESS[:LABEL]=DIR` (repeatable), for example a second Claude account under `~/.claude-alt`. Without `LABEL` the label is `root-` plus the first 8 hex digits of SHA-256 over the absolute directory. `default` is reserved; each harness/label and harness/directory appears once. With `--harness`, every explicit root must name a listed harness.
- Every source is keyed `<harness>/<label>/<path relative to the root>` in every table.
- A root whose harness has no prep binding in this build is reported `unsupported` with its sources counted, never silently dropped.

Limits: `--max-record-bytes` (default 3145728), `--max-batch-bytes` (default 16777216, at least the record limit), `--threads` (default 8). `--modified-since RFC3339` reports older sources `skipped`: not read, committed rows and state kept.

## Table contract

- `TARGET/state.json`: the committed cursor, anchor, generation, checkpoint and facts of every source. Published last, by atomic rename, after the rows it references are durable.
- `TARGET/tables/{calls,turns,triggers,events,tool_uses}/*.parquet`: append-only parts.
- `TARGET/tables/{sources,sessions}.parquet`: current snapshots.
- `TARGET/views.sql`: DuckDB definitions of the canonical views. Parquet key-value metadata `unisphere.table_schema_version` carries the table schema version (2).

Every row starts `(source, generation, native_offset, native_key)`. Timestamps, token counters, model, ids and native addresses are nullable: null means not recorded by that dialect, never zero or an estimate.

Raw parts hold every sighting of every generation. Query the canonical views instead:

| View | Rows |
|---|---|
| `calls_v` | One per current-generation API call, deduplicated on `(source, generation, msg_id, request_id)` with per-field maximum and the last non-null `stop_reason` |
| `turns_v` | Turns with origin, sender and opener metadata |
| `triggers_v` | What opened each turn (`kind` uses the turn-origin vocabulary) |
| `events_v` | Typed events: compaction, recap, scheduled_fire, limit_notice, queue_op, model_switch, api_error, system_other |
| `compactions_v` | Compaction events with pre/post tokens and the first context after the boundary |
| `tool_uses_v` | Tool use and result merged: name, family, sizes, outcome, native duration |
| `sources_v` | Current committed state of every source: key, set, generation, status |
| `sessions_v` | Session facts per source: first/last native event time, counts, latest context, compactions |

## Incremental and generation semantics

- **Append sources** (JSONL transcripts) resume from the committed byte cursor. Reads stop at the last complete LF; bytes after it are a *pending tail*, reported per source and read by a later run once complete. A live session is never half-read.
- **Snapshot sources** (whole documents or databases) are re-read on any stat change; an equal native revision is `unchanged`.
- A source that was rotated, truncated or rewritten in place, whose snapshot revision changed, whose fold policy or table schema changed, or whose set root moved is `replaced`: it is folded again into a new **generation**. Views select the current generation, so superseded rows never double count.
- A source that fails keeps its previous committed rows and state; the others still commit. A crash between rows and state leaves orphan parts that the next run removes. One writer holds a target at a time.
- `unisphere prep compact --target DIR` rewrites current-generation rows into fewer parts and drops superseded ones; every view result is unchanged.

## Coverage vocabulary

Each run reports, per set, `discovered`, counts by status and discovery-time skips (symlinks are never followed; hidden entries and unreadable directory entries are skipped). Nothing is silently excluded.

| Status | Meaning |
|---|---|
| `new` | First fold of this source |
| `unchanged` | Stat matched the committed state; nothing read |
| `appended` | Only bytes after the committed cursor were read |
| `replaced` | New generation; `reason` is rotated, truncated, rewritten, revision, policy, schema or root_moved |
| `skipped` | Excluded by `--modified-since`; committed state kept |
| `unreadable` | Read failed; previous committed state kept; exit 3 |
| `unsupported` | No prep binding for the harness |
| `missing` | Committed source no longer discovered; its rows and state are kept |

JSON output lists every source that is neither `unchanged` nor `skipped` in `data.sources`; human output summarises and lists the sources needing attention.

Exit: 0 completed, 3 completed with at least one unreadable source, 2 invalid arguments (including `prep record` without `--include-content`), 1 other failure.

## Privacy defaults

Tables are metadata only: counts, sizes, token usage, timestamps, ids, hashes and vocabulary columns. `--include-content` adds the only content column, `triggers.content_head`; without it that column is absent. `unisphere prep record` returns one native record, which is content, so it is refused without `--include-content` before any native read. Native stores are opened read-only and are never locked, renamed or written.

## SDK reuse

The CLI holds no prep semantics; it builds a `PrepRequest` and renders the `PrepReport` of the injected `PrepApi`. An external Rust consumer runs the same incremental prep in process:

- `unisphere_sdk::prep::Preparer::new(bindings, store)` implements `PrepApi` over `PrepBinding { fold, loader }` values and any `PrepStore`, including your own in-memory store; no Parquet, SQLite or engine dependency is required.
- `unisphere_sdk::prep::fold_source(loader, fold, stat, source, generation, resume, options, limits, sink)` folds one source without a store or target directory and returns its checkpoint, `SessionFacts`, cursor and pending tail, the same fold `prep` runs.

## Examples

Every `unisphere` line below is parsed by the test suite with `$TARGET` and `$HOME` bound.

```sh
unisphere prep --target $TARGET
unisphere prep --target $TARGET --human
unisphere prep --target $TARGET --harness claude-code --json
unisphere prep --target $TARGET --root claude-code:alt=$HOME/.claude-alt/projects
unisphere prep --target $TARGET --no-default-roots --root claude-code=$HOME/archive/projects
unisphere prep --target $TARGET --modified-since 2026-01-01T00:00:00Z --threads 4
unisphere prep --target $TARGET --max-record-bytes 8388608 --max-batch-bytes 33554432
unisphere prep --target $TARGET --include-content
unisphere prep compact --target $TARGET
unisphere prep record --target $TARGET --source claude-code/default/project/session.jsonl --offset 0 --include-content
```

Query the views with an external DuckDB:

```text
cd $TARGET && duckdb -init views.sql -c "SELECT model, count(*) AS calls, sum(output) AS output FROM calls_v GROUP BY model ORDER BY calls DESC"
```

**Next step:** run `unisphere prep --target DIR --human`, then open `DIR/views.sql` in DuckDB.
