# Review — Plan 028 phase-2 composition, round 1

**Reviewer** `pij-panicky-anteater` (OMP, `github-copilot/claude-sonnet-5.5`, native session `01a0eb71-0bdc-74bd-97d4-6f85f7527e72`, pid 53353) · **Scope** composition · **Subject SHA (verified artifact)** `19ace79fd493350525323dd5da0cff8e3d3a020c` · **Date** 2026-09-29

## Verdict: **approved**

Every phase-2 lane composes cleanly onto the phase-1 baseline, both prime rulings (incremental discovery, commit waves) are correctly implemented and independently verified against real code and real tests, all measured results check out against the actual evidence files, and all six named composition decisions are confirmed correct by direct code inspection, not by trusting the packet's prose. The PM's own mid-review hold (closing a gap where the boot proof covered only the JSON-document snapshot, not the journal or SQLite) was itself verified: I re-ran the extended `unisphere-proof prep` myself and it passed, explicitly covering all three snapshot representations. No high or medium findings.

## Chain of custody

Verified exactly as the PM described and as phase 1 established the pattern: `assets/team/composition.dd.json` **at the subject** (`19ace79`) still shows `artifact_sha: b4958ad…` (the prior verification) — a commit cannot contain its own hash. The receipt for this exact subject lives in its descendant `9a09aabe514fddcf419ab6e667c731f4a14924f1` (confirmed: `git log -1 --format='%H %P' 9a09aabe…` shows its sole parent is `19ace79…`), where `composition.dd.json.artifact_sha = 19ace79…` and both `vd-0008`/`vd-0015` show `exit_code: 0`. Packet sha256 (`88defe30…c92d50`) and plan/guide sha256 (`7da3d4a4…6e46`, `6d513128…ac6d`) all match the bindings exactly.

**Delta from the held `b4958ad` review**: `git diff --stat b4958ad..19ace79` shows exactly what the PM said and nothing else: `crates/testkit/src/bin/proof/prep.rs` (+166 lines, the real proof-mode extension), `crates/testkit/src/bin/unisphere-proof.rs` (1 line), two new SQLite fixture binaries, `boot.mjs`'s limitations string, and the execution-log/composition-record/packet updates. I read the new `prep.rs` proof code in full: it drives three `SnapshotCase`s (`copilot-cli-snapshot` JSON document, `vscode-copilot` mutation journal, `cursor-ide` SQLite) through the **built and installed CLI binary**, each asserting cold→new, touched→unchanged (0 rows), changed→`replaced`/`revision`/generation 1, and (where applicable) `prep record` fetch. I ran it myself: `cargo run --locked -p unisphere-testkit --bin unisphere-proof -- prep` → **passed**, output explicitly naming "revisioned JSON document, mutation journal and SQLite snapshots" — the gap is genuinely closed, not just documented as closed.

## What I ran independently

- `cargo check --locked --workspace` → clean.
- `cargo run --locked -p unisphere-testkit --bin unisphere-arch-check` → 110 edges accepted (unchanged from phase 1 — no new dependency leakage), 40 core/adapter files scanned (up from 34, correctly including the 6 new adapter crates' `prep.rs` + `loader-snapshot`).
- Every phase-2 lane's test suite: `adapter-omp` (13), `adapter-pi` (13), `adapter-codex` (16), `adapter-copilot-cli` (15), `adapter-vscode-copilot` (9), `adapter-cursor` (9), `loader-snapshot` (7), `cli --test recipes` (7) — **89 tests, 0 failed**, all with descriptive behavioral names (`every_batch_split_and_checkpoint_resume_equals_one_fold`, `stat_folds_wal_growth_that_leaves_the_database_file_untouched`, `content_is_absent_unless_explicitly_requested`).
- Phase-1 non-regression: `sdk --test prep` (20), `output-prep --test store` (9), `adapter-claude --test prep` (14), `loader-jsonl --test prep` (7), `cli --test prep` (11) — **61 tests, 0 failed**, all still green against the unchanged v2 contract.
- `unisphere-proof -- prep` (vd-0008) → passed, independently reproducing the composition record's result on the extended scope.
- `cargo test -p unisphere-app` → 5 passed (composition-root, catalogue, query-workflow tests).

## Q1 — Wiring and ownership: confirmed

Read `crates/app/src/prep.rs` and `crates/app/src/adapters.rs` in full. `bindings()` is the single place any concrete `PrepFold`/`PrepLoader` is constructed — 10 fold registrations across exactly 9 harness ids: `claude-code`, `oh-my-pi`, `pi`, `codex`, `copilot-cli` (append) and `copilot-cli-snapshot` (legacy JSON document) both via the correct loader kind, `vscode-copilot` with **two** bindings (Document and Journal formats) under one harness id exactly as the docstring states, `cursor-transcript` (append) and `cursor-ide` (SQLite `cursorDiskKV`). Append sources use `FileSessionLoader`; the four snapshot representations use `SnapshotPrepLoader::new(format)`. This matches `coverage-dfb8bf8.json`'s "9/9 sets supported" exactly.

`git-ai` is genuinely unbound: `crates/adapter-git-ai/src/lib.rs` has `cli_persisted_resume: false` and no `bind(...)` call anywhere references it — it has no `PrepFold`, so any `git-ai` root is correctly reported `Unsupported`, confirmed by the SDK's own rule (`"a root whose harness has no binding is reported supported = false"`).

`cli_persisted_resume` is `true` for **exactly** the 9 bound descriptors and only there: I grepped every adapter crate and found `true` set individually inside each lane's own crate (`adapter-codex`, `adapter-copilot-cli`, `adapter-cursor` ×2, `adapter-omp`, `adapter-pi`, `adapter-vscode-copilot`), each lane flipping only its own descriptor as the guide's unit notes require (never touched by `tk-000e` outside its declared paths). `copilot-cli-snapshot`'s descriptor inherits it via `..DESCRIPTOR.capabilities` struct-update syntax rather than repeating it — clean, no duplication, no drift risk.

core/sdk purity: `arch-check` scans core + every `unisphere-adapter-*` crate (unchanged scope from phase 1) and passed with 0 violations at 40 files, up from 34.

## Per-AC evidence

| AC | Status | Evidence |
|---|---|---|
| ac-0006 (every harness) | **proven** | 9/9 sets supported (`coverage-dfb8bf8.json`); 89 new fold/loader tests green; explicit nulls confirmed (e.g. VS Code calls have no native timestamp — `lg-0009`'s "VS Code call ts 0.0%" note, consistent with the frozen contract's nullable `ts`). |
| ac-0007 (honest coverage) | **proven** | `coverage.py` enforces `unsupported == 0 and unaccounted == 0` in addition to accepting exit 3; real run: 5,423 discovered, 2 unreadable by explicit error code (`UNI-DATA`), 0 silently dropped. |
| ac-0009 (recipes) | **proven** | `recipes-9820bb5.json`: 24/24 recipes render and run through external DuckDB 1.5.5 with 0 failures on both synthetic and real targets; `cli --test recipes` 7/7 green. |
| ac-000c (scale, bounded memory) | **proven, numbers accepted below** | full-corpus (14 GB, 5,423 sources) cold/unchanged/append all measured; memory bound is real and documented (see ruling). |
| ac-000d (docs) | **proven** | research-recipes topic registered and complete (test), prep topic states current harness coverage — `tk-000d`'s own notes confirm this closes my phase-1 composition finding F-01, and the CLI test suite (`prep_topic_is_registered_and_every_documented_example_parses`, inherited from phase 1, still green) plus the new recipes-topic test cover it. |
| **Phase-1 ACs** | **no regression** | `parity-dfb8bf8.json` is numerically identical to phase-1's `parity-d800949.json` on every field: 55,607/55,607/55,607 call rows, the same four accepted divergences (10 window-first gaps, 522 rounding ties, 0 beyond rounding, 2 symlinked-source rows) — nothing shifted. All 61 phase-1 tests still pass. |

## Q3 — Prime rulings: both correct, both verified in code

**(a) Incremental discovery** — `unisphere_core::prep::walk_prep_root` (`crates/core/src/prep.rs:178-276`), read in full. A directory's cached `PrepDirListing` is reused only when `identity` and `mtime_ns` both match the previous index (line 209-213); every file in a reused *or* freshly-listed directory is still individually `stat`'ed via `fs.source(...)` (line 258-268) regardless of directory-reuse, so per-file content-change detection is untouched. The "racy" rule is real: a listing is inserted into the persisted index only when `now.saturating_sub(mtime_ns) >= PREP_RACY_DIR_NS` (2 s, line 269) — a directory modified within 2 s of being scanned is simply never cached, forcing re-list next run until it's provably stable. This guarantees sources-plus-skip-counts equal a full walk (unchanged directories replay verbatim what a full walk of an unmodified tree would produce, by the standard POSIX guarantee that any entry add/remove/rename bumps the parent's mtime) and an index-only miss never hides a source (worst case: one extra re-list, never a dropped file, never stale). Named test confirmed present and green: `incremental_discovery_relists_only_changed_directories_and_matches_a_full_walk` (`loader-jsonl`), `committed_discovery_index_is_handed_back_for_the_same_root_only` (`sdk`). **Correct.**

**(b) Commit waves** — `crates/sdk/src/prep.rs:220-378` (`run_prep`), read in full. `PrepState` is constructed once and threaded through the wave loop by value, mutated in place (`state.sources.insert(...)`, line 358) — never cloned per wave. `wave_end(...)` bounds each wave by `request.max_run_bytes` (default 256 MiB, refused if 0, line 227-229); a wave that changed anything commits immediately (line 366-376) before the next wave starts, so a failure on a later wave leaves the earlier commit durable. The store's own publish step (`Published<'a>` in `crates/output-prep/src/lib.rs:56-86`) borrows `&PrepState`'s `sets`/`sources` maps rather than cloning them for serialization — confirmed by reading the struct and its exhaustive-destructure `of()` constructor, which is itself a nice defensive touch (a new `PrepState` field fails to compile there until explicitly published). This is exactly what explains the RSS drop (3.97 GB → 1.76 GB peak): before, 57 commits each needed the full growing state to be safely handed to the store; now it's one owned value borrowed, not copied, 57 times. Named tests confirmed present and green: `commit_waves_split_new_input_by_budget_and_equal_a_single_wave`, `failure_after_a_committed_wave_keeps_that_wave` (both `sdk`). **Correct.**

## Q4 — Measured results: **ACCEPT**

Verified every number directly against `measure-dfb8bf8.json` and `measure-6161ba6.json` (before rulings):

| | cold wall | cold peak RSS | unchanged wall | unchanged dirs listed | append wall |
|---|---|---|---|---|---|
| Before rulings (`6161ba6`) | 42.5 s | **3,974.8 MB** | **49.489 s** | (full re-walk) | — |
| After rulings (`dfb8bf8`) | 30.617 s | **1,759.1 MB** | **1.421 s** | **0 of 14,707** | **0.66 s** |

All match the packet's Q4 bullets exactly (13.99 GB read = `13,988,474,923` bytes; 57 commits; 600.4 MB unchanged-run RSS; 0.42 s unchanged user CPU). Load averages during measurement (73–109) match the packet's stated range and are recorded, not hidden.

**Is "bounded by documented, adjustable wave budget plus the committed state held in full" an acceptable reading of AC-000c's "bounded by documented, adjustable limits"? Yes, accepted.** The wave budget (`--max-run-bytes`, documented, adjustable, default 256 MiB) bounds the dominant, corpus-*depth*-scaling term (rows buffered per commit) — the term that actually drove the 3.97 GB→1.76 GB reduction. The residual — committed state held in full — scales with corpus *breadth* (source count: 58.6 MB for 5,423 sources), not depth, is the same disclosed, accepted cost the phase-1 risk register (`rk-0002`) already named, and is over an order of magnitude smaller than the bounded term for any realistic corpus. This is an honest reading, not a euphemism for unbounded memory.

## Q5 — Other composition decisions: all confirmed correct by direct inspection

**(a) `--max-record-bytes` defaults to the batch limit — ACCEPT.** `crates/cli/src/args.rs:1288`: `max_record_bytes: args.max_record_bytes.unwrap_or(max_batch_bytes)`. Precedence proven by `cli/tests/prep.rs`: unset record-bytes tracks batch-bytes exactly (both `16*1024*1024` when both defaulted, both `4_194_304` when batch is lowered and record is left unset) while an explicit record-bytes value (`1_048_576`) persists independent of batch changes.

**(b) Zero-byte VS Code documents reported unreadable, not folded as empty — ACCEPT.** Confirmed by `coverage-dfb8bf8.json` (2 `UNI-DATA` unreadable, documented) and `measure.py`/`coverage.py`/`live.py` all explicitly accepting exit 3 rather than silently succeeding past it — the failure is surfaced, not swallowed.

**(c) Prep's glob `*` spans `/`; loader-query uses literal separators — ACCEPT (genuine, intentional divergence, confirmed).** `crates/sdk/src/prep.rs:203-207` uses plain `globset::Glob::new(pattern)` (default: `*` matches `/`); `crates/loader-query/src/lib.rs:900-901` explicitly calls `.literal_separator(true)`. This is a real, deliberate difference between the two subsystems, not an oversight — it's exactly what lets nested Oh My Pi/Pi subagent files (`sess-0001/subagents/agent-a1.jsonl`) match a fold's `pattern` like `*/*.jsonl` while the query path's stricter globbing intentionally does not conflate them.

**(d) `compactions_v` coalesces unrecorded cache writes to 0 only when `basis = 'none'` — ACCEPT.** Read the exact SQL (`crates/output-prep/src/views.rs:159-173`): `CASE WHEN cw_1h IS NULL AND cw_5m IS NULL THEN (CASE WHEN cache_write_basis = 'none' THEN 0 END) ELSE coalesce(cw_1h,0)+coalesce(cw_5m,0) END`. When both fields are null *and* the dialect has no cache-write concept at all (`basis = 'none'`), the result is `0`; when both are null under any *other* basis (an unexpected partial-data case), the inner `CASE` has no `ELSE` and evaluates to `NULL`, correctly propagating "unknown" rather than fabricating zero.

**(e) Snapshot `prep record` resolves `<key>#<pointer>` and bare document pointers; journal pointers refused — ACCEPT.** Read `crates/loader-snapshot/src/prep.rs:120-161` in full: `key.split_once('#')` handles the compound form; a bare `/`-leading key is accepted **only** for `SnapshotFormat::JsonDocument` (line 133-136) and explicitly refused (`InvalidInput`) for every other format including `JsonJournal`, matching the doc comment verbatim ("Pointers into a journal's reduced document cannot be resolved from raw journal operations"). Independently exercised by the extended proof I ran: the `vscode-copilot` `SnapshotCase` has `record: None` (no record-fetch attempted for the journal case) while the JSON-document and SQLite cases both have `record: Some(...)` and passed.

**(f) `coverage.py`/`measure.py` accept exit 3 — ACCEPT.** `measure.py:98`: `failed = any(exit not in (0,3) ...)`. `coverage.py:102`: accepts `(0,3)` **and** additionally requires `unsupported == 0 and unaccounted == 0`. `live.py`'s fleet check (line 141) accepts `(0,3)` **and** requires the unreadable count to stay constant across all 10 runs (`r["unreadable"] != static` triggers failure) — this isn't a blanket "ignore exit 3," it's "exit 3 is fine only if it's the same known, disclosed sources every time." All three scripts were read in full; the acceptance is deliberate and guarded, not a rubber stamp.

## Tests behavioral, not wiring echoes

Confirmed by running all 89 new-lane tests and reading representative bodies from the extended proof code: every scenario constructs real inputs (temp filesystems, real SQLite fixture bytes, a spawned CLI subprocess) and asserts on observable output (JSON envelope fields, exit codes, row counts), never internal call-count mocks. Fixtures are synthetic per the guide's own convention (`crates/adapter-*/tests/fixtures/prep`, `crates/testkit/fixtures/prep-snapshots/state-v{1,2}.vscdb` generated from a committed synthetic `ide.json`) — no real transcript content observed or read.

## Findings

None open (high/medium/low). Zero findings this round.
