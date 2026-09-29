# Plan 029 decomposition review, round 1

**Verdict: changes-requested.** The core baseline (`core::status`, tests) is sound and needs no change. Four medium findings are fixable before seal: three in the guide/packets, one additive testkit change. None reopens the core DTO shape. Re-review can be a delta.

- Reviewer: `pij-easy-seahorse` (omp, `github-copilot/claude-sonnet-5.5`, requested = observed)
- Subject SHA: `a025ef0ff805c4882aec95ee6b4f70ae013bc12f`
- Plan sha256 `72145b14…5a39` and guide v1 sha256 `c84d28b7…2609` match the packet bindings (recomputed).
- Ran in the worktree: `cargo test --locked -p unisphere-core --test status_contract` 6 passed; `unisphere-arch-check` 110 edges accepted; `cargo clippy -D warnings` on core and testkit clean. Worktree clean.
- `crates/sdk/src/prep.rs` and `crates/core/src/prep.rs` here are byte-identical to Plan 028 HEAD `99ac093` / `67bd121` (diff empty), so the fold contract being consumed is the real one.

## 1. Does `core::status` match the guide?

Yes. Every DTO, field name, `Option` shape, `Basis`/`ResolveBasis` vocabulary, failure code (8 kinds) and port signature in `architecture.contracts` is present in `crates/core/src/status.rs`. `CompactionCounts`/`CompactionSample` are re-exported from `prep` (pre/post tokens present, so ac-0004 "last compaction time/pre/post" is expressible). `SessionFacts` supplies `latest_context` (model, stop_reason, total), `last_model_switch`, `last_event_ms`, `compactions`, `last_compaction`, so most facts need no row accumulator.

No serialized-shape mismatch that would force a post-seal core change. Gaps are in interface definitions around the core, not inside it (F-01, F-03, F-04, F-05).

## 2. Are the fakes enough for each wave-1 lane?

- tk-0003 (CLI + resolver): yes. `FakeStatusApi` (records calls, keyed by harness+session) and `FakeTargetResolver` are enough; resolver command/process/fs fakes are the coder's own inputs.
- tk-0002 (SDK): almost. `ScriptedFold` plus the existing `testkit::prep::MemoryLoader` (append/rotate/rewrite/truncate, mtime, inode) cover cursor reset, partial tail, replace and mtime fallback. One gap: F-02 below.

## 3. AC coverage and the `status_incremental` signature

| AC | capability | owner | real check |
|---|---|---|---|
| ac-0001 | cp-0001 | tk-0003 | vd-0003/0004 fakes, vd-0006 assembled; 50 ms by vd-0008 (not listed on cp-0001, F-06) |
| ac-0002 | cp-0002 | tk-0001 | vd-0001 |
| ac-0003 | cp-0003 | tk-0002 | vd-0002 (dedup untestable, F-02) |
| ac-0004 | cp-0004 | tk-0002 | vd-0002 |
| ac-0005 | cp-0005 | tk-0004 | vd-0008 |
| ac-0006 | cp-0006 | tk-0002 + tk-0004 | vd-0002/0005/0006/0007/0008 |
| ac-0007 | cp-0007 | tk-0003 (phase 1 slice) | vd-0003/0006; OMP/Codex deferred to guide v2 by plan |

Each AC has an owner and a runnable check. The embedding signature is close but needs F-01 and F-03 before it is right for pij-rs Plan 157 (explicit target, caller-held cursor, no sqlite, serde status: all satisfied by the DTOs; the graph rule is already enforced by arch-check).

## Findings

### F-01 (medium): `StatusService::new(Vec<PrepBinding>)` cannot locate a transcript
`PrepLoader::stat(root, path)` and `discover(root, accept)` both need a root (`loader-jsonl::prep::stat` does `path.strip_prefix(root)` and errors otherwise; `MemoryLoader::stat` rejects `root != self.root`). `PrepBinding` is `{fold, loader}` only. The guide says the default location for (harness, session id) with no path comes from "binding loader discovery under the catalogue root", but nothing supplies that root, and core/sdk must not read ambient state (`--pij`/`--pane` resolve to a session id with no path). Both `--pij` cases in ac-0005/vd-0008 depend on this.
**Fix (guide only):** `StatusService::new(bindings: Vec<PrepBinding>, roots: Vec<PrepSourceSet>)` (harness-keyed, built by `crates/app`, the way `prep_api` already builds `default_set`), and state the rule for an explicit `transcript`: it must lie under one of the harness's roots, else `UNI-STATUS-TRANSCRIPT-NOT-FOUND`. Update the tk-0002 interface line and composition step for tk-0004.

### F-02 (medium): scripted fold cannot emit an `Update` sighting or a shared `msg_id`
`ScriptedFold` always writes `CallSighting::First` with `msg_id = "msg-<offset>"`. ac-0003 and `SessionStatusApi` derive `last_call` from "deduplicated" main-chain calls, and `PrepCallRow` semantics are that `Update` rows raise counters of an earlier `First` (per-field max). tk-0002 cannot prove the max-merge with the frozen fake, and the packet forbids coders editing baseline files.
**Fix before seal (additive, testkit only):** accept optional `sighting` (`"first"`/`"update"`) and `msg_id` in the `call` step; add one baseline test proving the emitted rows.

### F-03 (medium): cursor is passed by value; error path loses it
`status_incremental(&self, target, cursor: Option<StatusCursor>, now_ms) -> Result<(SessionStatus, StatusCursor), StatusFailure>`: on `Err` the caller has no cursor back and must clone the (PrepResume + accumulators) value before every call, which is a real cost for 50 warm seats < 100 ms with per-turn accumulators. Two consequences to specify:
- Use `cursor: Option<&StatusCursor>`. Keep `Clone + Send`; add `Sync` (pij may share it across threads).
- A cursor built for another target/transcript path is rejected as a visible reset (`source.reset`), never silently applied.
Also state that the "incremental == cold" test compares everything except `source` (`bytes_read` legitimately differs; rk-0004 asserts 0 on unchanged).

### F-04 (medium): who fills `resolved.target.transcript`, and is `resolved` set for explicit queries?
ac-0001 says every result's `resolved` block carries the transcript path. The resolver (tk-0003) only learns a session id (pij registry / `~/.claude/sessions/<pid>.json`); the path is found by the SDK (tk-0002). Nothing says the SDK reports it back, and `core::status` documents `SessionStatus.resolved` as "Filled by the CLI layer when the query was a Pij id or pane" (contradicting ac-0001 for `--session --harness`). Two lanes will otherwise pick different answers.
**Fix (guide/packets; a doc-comment tweak in core is non-breaking):** SDK MUST set `SessionStatus.target.transcript` to the located path; the CLI copies it into `resolved.target.transcript`; the CLI fills `resolved` for every query, with `basis: explicit` for `--session/--harness`.

### F-05 (low): the `unknown` name vocabulary is not frozen
`unknown: Vec<String>` is free text ("dotted names"); `SessionStatus::empty` has `unknown = []` although every fact is `None`. The docs topic (tk-0003) and derivation (tk-0002) will each spell the names. Add a `pub const` list of unknown-fact names to `core::status` (or an explicit table in the guide) and state that `empty` leaves `unknown` unpopulated by design.

### F-06 (low): capability proof lists
cp-0001 omits vd-0004 (resolver) and vd-0008 (the 50 ms pane budget). cp-0006 should note the warm-seat number is measured in vd-0008 only. Doc fix.

### F-07 (advisory)
`ContextSample.total` is `i64`; `used_tokens` is `u64`. State that a negative or missing total maps to `None` (unknown). Keep the cursor compact (turn timestamps for last-hour counts are the only unbounded accumulator).

## Reason to seal
None beyond F-01..F-04. Fold-side inputs the guide assumes (`latest_context.model/stop_reason`, `last_model_switch`, `post_tokens`) already exist in `ClaudePrepFold`; the plan's "missing 028 facts stay unknown" risk is therefore narrower than stated.

## Next
PM: apply F-02 (testkit + one test), amend guide v1 → v1.1 for F-01/F-03/F-04/F-05/F-06 (packets inherit), re-run vd-0001/0005, then request a delta re-review; core baseline needs no change.
