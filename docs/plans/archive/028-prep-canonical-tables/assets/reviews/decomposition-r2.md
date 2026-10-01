# Review — Plan 028 decomposition, round 2 (baseline for seal)

**Reviewer** `pij-panicky-anteater` (OMP, `github-copilot/claude-sonnet-5.5`, native session `01a0eb71-0bdc-74bd-97d4-6f85f7527e72`, pid 53353) · **Scope** decomposition · **Subject SHA** `3f39e1b4b47776e9b89464a604192aaedddf99ea` · **Date** 2026-09-29

## Verdict: **approved**

All three round-1 findings are closed with committed, executable, passing proof — not just guide prose. I independently compiled the full workspace, ran the contract fixture tests, and ran the architecture sensor against this exact subject; all green. The contract baseline may be sealed and released to the four phase-1 coder lanes.

## What I inspected

- `docs/plans/028-prep-canonical-tables/assets/team/reviewer-decomposition-r2.md` (packet), `plan.dd.json` (unchanged, hash confirmed), `assets/impl-guide.dd.json` v2 (full, all 14 contract clauses read untruncated), `assets/tasks/phase-1/tasks.dd.json` — all three sha256 in the clone match the packet's bindings table exactly.
- The round-1 record `assets/team/review-decomposition-rv-028-decomposition-r1.dd.json` — confirms my r1 findings (F-01/F-02/F-03) as filed, `changes-requested`.
- The committed contract baseline in full: `crates/core/src/prep.rs` (771 lines, v2), `crates/core/tests/prep_contract.rs` (211 lines, 6 tests), `crates/testkit/src/prep.rs` (752 lines: `MemoryPrepStore`, `MemoryLoader`, `RecordingFold`).
- The adapted wave-1-owned files at this SHA: `crates/{sdk,loader-jsonl,adapter-claude,output-prep,cli}/src/prep*.rs`, `crates/app/src/main.rs`.
- Ran independently in the clone (not merely trusting the PM's stated evidence):
  - `cargo check --locked --workspace` — **clean**, every crate including `unisphere-output-prep` and `unisphere-app` compiles against the new v2 contract.
  - `cargo test --locked -p unisphere-core --test prep_contract` — **6 passed, 0 failed**, matching the PM's stated evidence exactly.
  - `cargo run --locked -p unisphere-testkit --bin unisphere-arch-check` — **110 declared edges accepted, 34 core/adapter files scanned, 0 violations**.

## Round-1 findings — verified disposition

### F-01 — **fixed**
`PrepSourceStatus` (`crates/core/src/prep.rs:563-576`) now has an explicit `Skipped` variant — *"discovered but excluded by an explicit scope (`modified_since`); not read, committed state retained"* — distinct from `Missing` — *"committed source no longer discovered; rows and state kept."* The `label()` method (lines 578-591) serializes to exactly the seven words AC-0007 names, in AC order: `new, unchanged, appended, replaced, skipped, unreadable, unsupported` (plus `missing`, the eighth, for the source-vanished case AC-0007 doesn't separately name). The committed test `status_vocabulary_covers_every_reported_outcome` (`prep_contract.rs:74-111`) asserts this literal label list and passes.
**Evidence:** `crates/core/src/prep.rs:561-591`; `crates/core/tests/prep_contract.rs:74-111`; test run above (6/6 pass, this test included).

### F-02 — **fixed**
Every row struct now carries `native_offset: Option<u64>, native_key: Option<String>` at its prefix (`PrepCallRow`, `PrepTurnRow`, `PrepTriggerRow`, `PrepEventRow`, `PrepToolUseRow` — all read directly, `crates/core/src/prep.rs:250-367`), and every dialect-optional field (`ts`, `ts_ms`, `model`, `input`, `cw_1h`, `cw_5m`, `cache_read`, `output`, `started_ts`, `started_ts_ms`, etc.) is `Option<>`. The guide v2 architecture.contracts text states this as an explicit rule (*"Nullability is part of the contract… only source, generation, vocabulary columns, turn_no/next_turn_no, chars, records and is_sidechain are always present"*), matching the code exactly. The committed test `unrecorded_row_fields_serialize_as_null_not_zero` (`prep_contract.rs:113-151`) constructs a `PrepCallRow` shaped exactly like a Cursor-transcript call (`source: "cursor/default/p/t.jsonl"`, every optional field `None`) and asserts every one serializes to JSON `null`, not omitted or zeroed — this is precisely the phase-2 scenario my round-1 finding named, now proven, not just asserted in prose. `PrepFold::open` also gained the `source: &str` parameter the guide v2 states.
**Evidence:** `crates/core/src/prep.rs:250-377` (row structs), `:512-529` (`PrepFold::open` signature); `crates/core/tests/prep_contract.rs:113-151`; test run above.

### F-03 — **fixed**
`cp-000b`'s proof list (`impl-guide.dd.json` capabilities) now reads `[vd-0002, vd-0003, vd-0005, vd-0006, vd-0008]` — `vd-0006` added.
**Evidence:** `impl-guide.dd.md` capabilities table, `cp-000b` row.

## Answers to "what to assess"

1. **F-01/F-02/F-03 closed?** Yes, all three, verified against committed code and passing tests, not guide text alone (above).

2. **Does committed `core::prep` match guide v2 closely enough to freeze?** Yes. I cross-read every field name, signature and vocabulary in the guide's `architecture.contracts` clauses against the actual struct/enum/trait definitions in `crates/core/src/prep.rs` line-by-line (`TurnOrigin`'s twelve kebab-case variants, `PrepEventKind`'s eight snake_case variants, `PrepSourceStatus`'s eight variants, `PrepRecordRequest.max_bytes`, `PrepFold::open(meta, source, generation, saved)`) — no mismatch found. `cargo check --workspace` compiling clean across every crate that consumes these types (sdk, loader-jsonl, adapter-claude, output-prep, cli, app) is the strongest available evidence that names/signatures/nullability are self-consistent at the type level, and the arch-check sensor confirms no dependency-boundary regression from the baseline edits.

3. **Are the testkit fakes sufficient?** Yes for the enumerated engine scenarios. `MemoryLoader` (`crates/testkit/src/prep.rs`) exposes `append` (line 256), `rotate` — new identity (line 281), `rewrite` — same identity/new content with a `touch` flag to control mtime (line 299), `truncate` (line 311), `remove`/missing (line 321), `put_snapshot` for Snapshot-kind sources (line 326), `set_unreadable` (line 367); its `read()` sets `incomplete_tail: !more && !rest.is_empty()` (line 541), modeling the partial-tail case. `MemoryPrepStore::fail_next_commit()` (line 58) injects a commit failure. `RecordingFold` exists (line 615) as the scripted-fold fake engine tests drive. For the CLI: `PrepApi` is a 3-method trait (`prep`/`compact`/`record`) over plain DTOs with no lifetime or generic parameters — trivially fake-implementable; nothing in the frontend unit's path requires it to touch concrete engine/store types.

4. **Is the POC adaptation genuinely compile-level, no hidden new semantics?** Yes, with one honest, explicit exception the packet itself names: `ParquetPrepStore::compact()` (`crates/output-prep/src/lib.rs:460-463`) returns `Err(Unsupported)` with a comment *"Compaction is delivered by the store lane (guide unit tk-0004)"* — a stated placeholder, not a silent gap, and it's exactly what `tk-0004`'s responsibility text says it owns. I found no other adapted file introducing behavior beyond what compiling against the new signatures requires. Unit path ownership remains non-overlapping (re-confirmed against the updated `units` array: `tk-0002` still owns only `adapter-claude/*`, `tk-0003` only `sdk/*` + `loader-jsonl/*`, `tk-0004` only `output-prep/*`, `tk-0005` only `cli/*`).

5. **Any reason not to seal?** None found. Workspace compiles, contract tests pass, arch-check passes, both prior contract-completeness findings are closed with executable proof, testkit fakes cover the required scenarios, and unit ownership is unambiguous.

## Findings this round

None open. F-01, F-02, F-03 carried forward as **fixed** (see above); no new findings.
