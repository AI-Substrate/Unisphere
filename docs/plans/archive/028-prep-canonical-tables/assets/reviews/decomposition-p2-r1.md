# Review — Plan 028 phase-2 decomposition (guide v3), round 1

**Reviewer** `pij-panicky-anteater` (OMP, `github-copilot/claude-sonnet-5.5`, native session `01a0eb71-0bdc-74bd-97d4-6f85f7527e72`, pid 53353) · **Scope** decomposition · **Subject SHA** `f82d7b502712dcabad8b7ba66043e4fa47600267` · **Date** 2026-09-29

## Verdict: **approved**

Guide v3 may be sealed and its seven coder lanes released. Lane independence is real (verified against the `reads`/`depends_on` graph, not assumed), the unchanged v2 contract is architecturally sufficient for every planned representation including the Cursor-IDE many-composers case, and I directly tested the CLI recipe lane's DuckDB pipe mechanism end-to-end with the installed `duckdb` binary — it works exactly as documented. No high or medium findings.

## What I inspected and ran

- Packet; `plan.dd.json`, `assets/impl-guide.dd.json` (v3, all 12 sections read in full via JSON, not truncated markdown cells), `assets/tasks/phase-2/tasks.dd.json` — all three sha256 in the clone match the packet bindings exactly.
- `harness builder guide docs/plans/028-prep-canonical-tables --check` → `status: ok`, **26 `write-overlap` warnings**, matching the packet's claim exactly. I grouped all 26 by file and confirmed they resolve to exactly 13 distinct files, each pairing one **phase-1 historical unit** (`tk-0005` or `tk-0006`, both already delivered and composed) with exactly one **phase-2 unit** (`tk-000d` or `tk-000e`) — sequential ownership across the phase boundary, never two phase-2 lanes colliding on the same file.
- Full JSON for every phase-2 unit (`tk-0007`…`tk-000e`): `paths`, `reads`, `depends_on`, `wave`.
- `crates/core/src/snapshot.rs` (existing `SnapshotFormat`/`NativeSnapshot`/`SnapshotRecord`/`SnapshotLoader` — pre-dates this plan, backs the existing `query` command's VS Code/Cursor snapshot support) and `crates/loader-snapshot/src/unix.rs` (existing SQLite/JSON snapshot reader).
- Backpressure survey refresh (`assets/backpressure.dd.json`): certainty upgraded `Partial` → `Confident` since phase-1 landed and DuckDB is now installed (`rk-0007` resolved with Jordan's approval).
- Confirmed no phase-2 fold/loader source files exist yet (`find crates/adapter-{omp,pi,codex,copilot-cli,vscode-copilot,cursor}`, `crates/loader-snapshot` → no `*prep*` files) — correct for a pre-coding decomposition review.
- Independently ran: `cargo check --locked --workspace` → clean (no regression from the phase-1 composed baseline).
- **Directly tested the recipe-pipe mechanism** with the installed `duckdb 1.5.5`: wrote a real Parquet file and a `views.sql` defining a view over a relative `read_parquet([...])` path, then piped the exact documented script shape (`SET file_search_path = '<abs dir>'; .read '<abs dir>/views.sql'; <query>`) into `duckdb` from an **unrelated working directory** (`/`) — it resolved the relative path correctly and returned rows. This is not a plausible-sounding design; it's verified DuckDB behavior.

## What to assess

### 1. Lane independence — verified, no hidden coupling
All seven coder lanes (`tk-0007` OMP+Pi, `tk-0008` Codex, `tk-0009` Copilot CLI, `tk-000a` VS Code Copilot, `tk-000b` Cursor, `tk-000c` snapshot loader, `tk-000d` CLI recipes) are `wave: 3`. Their `reads` arrays name only frozen, already-sealed/composed files (`tk-0001` core contract, `tk-0002` Claude fold as a *reference pattern*, `tk-0003` JSONL loader, `tk-0004` output-prep, `tk-0005` CLI prep frontend) — never each other. `depends_on` for every fold lane is `[tk-0001, tk-0002, tk-0006]` (contract + composed phase-1 baseline), for `tk-000c` it's `[tk-0001, tk-0003, tk-0006]`, for `tk-000d` it's `[tk-0001, tk-0004, tk-0005, tk-0006]` — zero lane-to-lane dependencies. The only wave-4 unit, `tk-000e` (PM composition), reads from all seven — correct integration ordering, not a lane coupling.

Specific coupling risks named in the assess list, checked directly:
- **Snapshot record keys / `native_key` conventions**: `SnapshotRecord{key, bytes}` already exists (`crates/core/src/snapshot.rs`) and is the same type the *existing, working* query-side SQLite/JSON snapshot loader (`crates/loader-snapshot/src/unix.rs`) already produces one row per composer/document. `tk-000c` wraps this proven reader behind `PrepLoader`; no new key convention needs inventing.
- **JsonJournal reduction**: `SnapshotFormat::JsonJournal` and its `revision()` computation already exist and are exercised by the existing snapshot reader's test suite (`unix.rs:253-316`) — `tk-000a`'s VS Code journal-to-document reduction reuses this, not novel territory.
- **SQLite table naming**: `SnapshotFormat::SqliteKeyValue{table}` is an existing, explicit, named-table variant (no ambient discovery) — `tk-000b`'s Cursor IDE lane and `tk-000c`'s loader both reference the same explicit `table` string, not an implicit convention that could drift.

### 2. Contract sufficiency — sufficient, no delta needed
The v2 contract (`Option<>`-everywhere rows, `PrepSourceKind::Snapshot`, `NativeAddress{offset, key}`) already covers every representation phase 2 needs, and I found no case where a fold lane's stated behavior requires a field the frozen contract lacks.

**Cursor IDE many-composers-per-database (rk-000c), specifically assessed**: one `state.vscdb` file is one `PrepSourceStat`/one `PrepSourceState` (one row in `sources`/`sessions` tables, one `SessionFacts`), but each composer is a separate `SnapshotRecord` with its own `key` inside that file — the existing key-value reader already returns a `Vec<SnapshotRecord>`, not one blob. `tk-000b`'s calls/turns rows key off `native_key` = the composer id (per-record, not per-file), so individual composer sessions remain distinguishable in the `calls`/`turns` tables even though the file-level `sessions` row necessarily describes the database as a whole. This is architecturally sound: the *table* granularity (one row per interesting event) is finer than the *source* granularity (one row per file), exactly as the phase-1 contract already treats multi-record JSONL files. No contract delta needed — this is the same pattern already proven for Append sources, just keyed instead of offset-addressed.

### 3. AC-0006/0009/000c/000d coverage
| AC | Owner | Entrypoint | Proof |
|---|---|---|---|
| ac-0006 (every harness) | `tk-000e` (composition; each fold lane feeds it) | `crates/app/src/prep.rs` binds every catalogued representation | `vd-000d`…`vd-0013` (per-lane fold/loader tests) + new `vd-0017` (real-corpus per-harness coverage: sources by status, rows per table, share of rows with model/usage/ts/ids present) + `vd-0008` |
| ac-0009 (recipes) | `tk-000d` | `unisphere prep recipe NAME --target DIR \| duckdb` | `vd-0014` (catalogue tests) + `vd-000c` (external DuckDB, real + synthetic target) |
| ac-000c (scale) | `tk-000e` | full-corpus cold/unchanged/append measurement | `vd-000b`, now all-harness not Claude-only |
| ac-000d (docs) | `tk-000d` | research-recipes + updated prep topic (states current harness coverage — closes my own phase-1 composition finding F-01) | `vd-0005`, `vd-0014`, `vd-0008` |

`vd-0017` plus the per-lane fold tests (`vd-000d`…`vd-0013`) is adequate for "every registered harness … explicit nulls with per-source coverage": the fold tests prove each dialect's *shape* (which fields are null, which are present) on synthetic fixtures; `vd-0017` proves the *real-corpus share* of non-null rows per harness — together these cover both the unit-level contract claim and the real-world honesty claim, matching the pattern already proven for Claude in phase 1 (`vd-0002` + `vd-000a`).

I note `tk-000d`'s own notes explicitly state it *"closes the phase-1 composition review's low finding"* — my `F-01` from `composition-p1-r1` (the prep docs topic should state current harness coverage). Confirmed addressed at the guide level; will re-verify against the actual shipped docs text at phase-2 composition review.

### 4. CLI recipe output contract and one-command pipe — sound, tested
Confirmed and directly tested above: `SET file_search_path = '<abs TARGET>';` then `.read '<abs TARGET>/views.sql'` then the query, documented as `unisphere prep recipe NAME --target DIR | duckdb`. This works from any invoking directory because `file_search_path` (a real DuckDB session setting) resolves every relative path inside `read_parquet([...])` against it, independent of process cwd. No engine is linked into the Rust build; the CLI only prints text (`tk-000d`'s notes: *"No engine is linked and the CLI never shells out"*), matching the plan's non-goal on embedding an analytical engine. `--max-snapshot-bytes`/`--max-snapshot-records` (ac-000c adjustable bounds, `rk-000d`) are correctly placed on `tk-000d` since they're CLI argv surface over the already-existing `PrepReadLimits.snapshot` field, not new semantics in a fold or loader.

### 5. Any reason not to seal?
None found. Contract unchanged and already sufficient; all seven lanes provably independent by the dependency graph, not merely by narrative; the one genuinely new mechanism (the recipe pipe) I verified by direct execution, not by trusting the prose; the 26 structural warnings are all benign sequential-ownership artifacts I traced individually, not real conflicts.

## Findings

None open (high/medium/low). Zero findings this round.
