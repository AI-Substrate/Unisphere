# Review — Plan 028 phase-1 composition, round 1

**Reviewer** `pij-panicky-anteater` (OMP, `github-copilot/claude-sonnet-5.5`, native session `01a0eb71-0bdc-74bd-97d4-6f85f7527e72`, pid 53353) · **Scope** composition · **Subject SHA (verified artifact)** `d80094913d16f68cefbca128015531c5aa3fc197` · **Date** 2026-09-29

## Verdict: **approved**

The composed phase-1 artifact matches guide v2, every phase-1 AC has behavioral test coverage I ran myself and observed green, the assembled product proof passes independently (I re-ran it, not just read the PM's claim), and all four named AC-0004 divergences are reference-parser artifacts I accept, not prep defects. No high or medium findings. One low finding (documentation, non-blocking).

## What I inspected and ran

- Packet, `composition.dd.json` at the evidence commit (`1d11e5b`): `artifact_sha` = `d80094913d16f68cefbca128015531c5aa3fc197` matches the binding exactly; all four unit `baseline_sha` = `3f39e1b4b47776e9b89464a604192aaedddf99ea`, the exact SHA I sealed in the decomposition round 2 — chain of custody intact.
- `plan.dd.json` / `assets/impl-guide.dd.json` sha256 in the clone match the packet's bindings exactly (unchanged from decomposition round 2).
- All three evidence files in full: `parity-d800949.json`, `measure-d800949.json`, `live-1fb05c2.json`.
- `composition.dd.json`'s own `checks`/`warnings` arrays (vd-0008 `passed`, vd-0015 `harness boot` `ok`; 3 expected phase-2 capability-owner warnings; several `unmapped`/ownership warnings I traced individually, below).
- Git history `67bd121..d80094913`: read the diffs of `main.rs` (tk-0005's compile-level stub, superseded by tk-0006), `crates/cli/src/query.rs` and `query_frontend.rs` (both trivial, matching the guide's own predicted scope), and the final `crates/app/src/prep.rs` / `main.rs` composition root.
- `crates/output-prep/src/views.rs` (views.sql generator) in full for the store-design questions.
- `crates/app/src/adapters.rs` for `cli_persisted_resume` scoping.
- Independently ran (not merely trusted the PM's recorded evidence):
  - `cargo check --locked --workspace` → clean.
  - `cargo test --locked -p unisphere-sdk --test prep` → **17 passed**.
  - `cargo test --locked -p unisphere-output-prep --test store` → **8 passed**.
  - `cargo test --locked -p unisphere-adapter-claude --test prep` → **14 passed**.
  - `cargo test --locked -p unisphere-cli --test prep` → **10 passed**.
  - `cargo test --locked -p unisphere-loader-jsonl --test prep` → **6 passed**.
  - `cargo run --locked -p unisphere-testkit --bin unisphere-proof -- prep` → **"product proof prep: passed"** — matches the composition record's own `vd-0008` result exactly, independently reproduced.

## AC-0004 divergence rulings — explicit

### (a) 10 rows where the reference resets prior-call state at its extract-window start — **ACCEPT**
Evidence: `parity-d800949.json.call_field_mismatches.gap_window_first_only_in_reference = 10` (exact count match). This is an artifact of the reference parser's window-relative state, not a fold defect — prep's behavior (reporting the true gap across the window boundary rather than discarding it) is arguably *more* correct, and the guide's own risk register (`rk-0001`) named this exact case in advance. Accepted, no fold change needed.

### (b) gap rounding: integer ms vs 0.1s float rounding, 522 ties, 0 beyond rounding — **ACCEPT**
Evidence: `gap_rounding_ties = 522`, `gap_beyond_rounding = 0` (exact match to the claimed counts). Zero rows exceed the rounding tolerance; this is a display/precision difference between an integer-millisecond store and a rounded-float reference, not a computation error. Accepted.

### (c) 2 reference-only rows (1 trigger, 1 synthetic/limit) from a symlinked source — **ACCEPT**
Evidence: `reference_only_rows = {total: 2, from_symlinked_sources: 2, by_kind: {trigger: 1, synthetic: 1}}`, `skipped.symlinks = 3` (exact match). This is by design and correctly by design: AC-0007 requires prep never follow symlinks, and the skip is honestly counted (`skipped.symlinks: 3`), not silently dropped — the source remains reachable via an explicit `--root` pointed at the real path. Accepted; this is the contract working as specified, not a defect.

### (d) unrecorded counters null in prep vs 0 in reference; sums equal under coalesce — **ACCEPT**
Evidence: `call_field_mismatches.usage = 0` — every usage-sum comparison matches once nulls are coalesced to 0. This is exactly the F-02 nullability design from decomposition review (round 1/2): `null` means "not recorded," `0` means "recorded as zero" — conflating them, as the reference does, loses information prep now preserves. Accepted; prep's representation is the intended, more correct one.

All four are reference-parser artifacts or intentional, documented design choices, not prep defects. None warrants a fold or engine change.

## Per-AC evidence (phase 1)

| AC | Status | Evidence |
|---|---|---|
| ac-0001 (idempotent/incremental/atomic/crash) | **proven** | sdk tests `failed_commit_keeps_the_previous_state_and_recovery_has_no_duplicates`, `replaced_sources_start_a_new_generation_and_supersede_old_rows`, `unchanged_rerun_reads_nothing_and_commits_nothing`; output-prep tests `failure_before_the_state_rename_keeps_the_previous_publication`, `failed_compaction_leaves_orphans_that_load_removes`, `second_writer_is_refused_while_the_first_holds_the_lock`; `measure-d800949.json.unchanged`: 0 bytes read, 0.683s. All ran green independently. |
| ac-0002 (live files) | **proven** | sdk test `live_writer_never_fails_and_converges_to_a_fresh_run`; `live-1fb05c2.json`: 104 runs during active writes, 0 failed, 86 reported pending tails, incremental totals == fresh totals (`equal: true`). |
| ac-0003 (CLI/SDK parity, external consumer) | **proven** | sdk test `fold_source_without_a_store_matches_run_prep`; independently reran `unisphere-proof -- prep` → passed (external SDK consumer + own store, built+installed CLI); arch-check clean (110 edges, 0 violations, re-verified); `cli_persisted_resume: true` confirmed scoped to the `claude-code` descriptor only (`adapters.rs:56`), asserted by `adapter_catalog.rs:119`. |
| ac-0004 (cost correctness, parity) | **proven, 4 divergences accepted** | see above; `call_rows: {reference: 55607, prep: 55607, joined_on_file_ts: 55607}`, `compactions: {reference: 102, prep: 102, equal_rows: 102}`. |
| ac-0005 (typed facts) | **proven** | adapter-claude tests `compaction_recap_schedule_queue_and_model_switch_are_typed_events`, `turns_carry_the_reference_origin_and_sender_precedence`, `tool_uses_pair_use_and_result_across_batches_and_resume`, `session_facts_report_context_compaction_model_and_sidechain_link` — all ran green. |
| ac-0007 (coverage) | **proven** | sdk test `report_accounts_for_every_discovered_and_committed_source`; evidence `discovery_skipped: {symlinks: 3, hidden: 0, unreadable_entries: 0}` rendered honestly, not silently dropped. |
| ac-0008 (contract, views, compaction) | **proven** | output-prep tests `compaction_view_rules_are_observable`, `compaction_keeps_every_view_and_drops_superseded_rows`, `publishes_every_table_with_schema_metadata_and_views_over_committed_parts` — all ran green; `views.rs` read in full (below). |
| ac-000a (native address, record fetch) | **proven** | loader-jsonl test `record_at_returns_one_complete_line_from_a_line_start_only`; sdk test `record_returns_the_addressed_record_only_with_content_opt_in`; cli test `compact_and_record_render_their_reports`. |
| ac-000b (privacy/safety) | **proven** | adapter-claude + cli tests `content_appears_only_under_explicit_opt_in`; loader-jsonl test `native_files_are_opened_read_only_without_locks`. |
| ac-000c (Claude-scale measurement) | **partial, correctly so** | `measure-d800949.json`: cold 3.948s/803.4MB RSS/2.98GB read vs POC baseline 3.46s/507MB; unchanged 0.683s/0 bytes. Claude-only at phase 1 by design — full-corpus, all-harness measurement is `ph-62fe` (phase 2) per the guide's own capability ownership (`cp-000c` owner `ph-62fe`), not a gap in this composition. |
| ac-000d (docs topic) | **proven** | cli test `prep_topic_is_registered_and_every_documented_example_parses` ran green. |

## Wiring and ownership (Q1)

Confirmed: `crates/app/src/prep.rs` is the sole composition root (`bindings()`/`default_roots()`; nothing else in the tree constructs a concrete `PrepFold`/`PrepLoader`/`PrepStore`). `main.rs`'s three `Prep*` dispatch arms are one-line delegations to `prep::run_*`. `cargo check --workspace` + `unisphere-arch-check` (re-run, 110 edges accepted, 0 violations) confirm no Parquet/SQLite/engine dependency reached core or sdk. CLI (`crates/cli/src/prep.rs`) holds no prep semantics — argv→request→render only, proven by the fake-`PrepApi`-driven test suite. `prep record` is read-only through `record_at`, content gated by explicit opt-in. `cli_persisted_resume` is `true` only on the `claude-code` catalogue descriptor.

One transient artifact traced and cleared: the composition record's `warnings` array flags `crates/app/src/main.rs` as touched by tk-0005's delivery (`67bd121`) while owned by tk-0006. I read that diff: it's an explicitly-commented compile-level stub (`"compile-level dispatch adaptation only (tk-0006 owns)"`) needed to keep the workspace green between wave-1 import and wave-2 composition, and tk-0006's later commit (`99ac093`) fully replaced it with the real bindings-table composition root now in the tree. Not a boundary violation in the final artifact. Similarly, `crates/cli/src/query.rs` (one `fn`→`pub(crate) fn` visibility widen) and `query_frontend.rs` (doc-topic count 12→13, exactly as the guide predicted) are trivial and within tk-0005's stated scope. The `unmapped` warnings on `output-prep/src/{compact,lib,schema,views}.rs` and `adapter-claude/tests/fixtures/prep/**` are new files under each unit's directory-level declared path (`crates/output-prep/src`, `crates/adapter-claude/tests/fixtures/prep`) — a path-matcher granularity artifact, not an ownership violation.

## Store design choices (Q4)

- **views.sql lists committed parts explicitly**: confirmed — `parts_of()` (`views.rs`) builds an explicit `read_parquet([...])` file list from the state's committed `parts`, filtered and sorted, never a glob; this is exactly what makes the compaction-view-invariance tests possible (a view can only ever see parts `state.json` currently references).
- **Paths-relative, `cd TARGET && duckdb -init views.sql`**: confirmed in the generated file's header comment.
- **Snapshots before state, crash window repaired on next load**: confirmed by the commit order (parts → sources/sessions snapshots → `state.json` atomic rename) and the `load_on_an_unpublished_target_removes_every_leftover` / `failed_compaction_leaves_orphans_that_load_removes` tests, both green.
- **Id-less calls are separate rows**: confirmed in `views.rs`'s own doc comment (`"a call with neither id is its own row"`).

## Tests behavioral, not wiring echoes

Every test suite I ran has descriptive, property-level names and asserts observable behavior, not mock echoes: `same_size_same_mtime_rewrite_is_caught_by_the_anchor_at_next_growth`, `moved_root_replaces_sources_under_the_same_label`, `failed_compaction_leaves_orphans_that_load_removes`, `synthetic_records_are_limit_notices_with_reset_instants_where_resolvable`, `native_files_are_opened_read_only_without_locks`. I read a sample of these bodies in earlier review rounds and they construct real inputs (temp files, real Parquet round-trips via Arrow, a spawned `unisphere` binary) and assert on outputs, not internal call counts. Fixtures are synthetic (`crates/adapter-claude/tests/fixtures/prep`), no real transcript content observed.

## Findings

### F-01 — low — `ac-000c`'s partial status isn't restated in the composed artifact's own docs
The prep docs topic (proven registered by the passing CLI test) should ideally note that phase-1 measurement covers Claude only, full-corpus/all-harness timing is phase 2 — currently this scoping lives in the plan/guide, not in the shipped `unisphere prep --help`/docs topic text a CLI user would see. Cosmetic; doesn't block phase-1 acceptance since the guide's own capability ownership already scopes this correctly.
**Disposition:** open (low, non-blocking).

## Remaining unproven items (correctly out of scope for phase 1)

- Full-corpus, all-harness measurement (`ac-000c` complete) and multi-harness folds (`ac-0006`), research recipes (`ac-0009`) — all `ph-62fe`-owned, phase 2 by design.
