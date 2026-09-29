# Plan 029 decomposition review, round 2

**Verdict: approved.** r1 findings F-01..F-07 are closed as dispositioned. The baseline (tk-0001) and guide v1.1 may be sealed and released to tk-0002 and tk-0003. No blocking findings; two non-blocking advisories.

- Reviewer: `pij-easy-seahorse` (omp, `github-copilot/claude-sonnet-5.5`; same seat as r1)
- Subject SHA: `aac28a053732a494fafe5219842de6d3aeaf06b6` (recomputed from the packet path; worktree clean)
- Plan sha256 `72145b14…5a39` unchanged; guide sha256 `e967cc78…e653` matches the packet (recomputed).
- Ran here: `status_contract` 6 passed; testkit `--lib status` 1 passed (`scripted_steps_emit_rows_facts_and_resume`); `unisphere-arch-check` 110 edges accepted; `cargo clippy --all-targets -D warnings` on core + testkit clean.
- Reviewed `git diff a025ef0..HEAD`: only `core/src/status.rs`, `core/tests/status_contract.rs`, `testkit/src/status.rs`, the guide, and r1 records/r2 packet. Plan 028 files untouched.

## Closure of r1 findings

| Finding | Evidence | Status |
|---|---|---|
| F-01 roots | Guide contract 6/interface: `StatusService::new(Vec<PrepBinding>, Vec<PrepSourceSet>)`; explicit path read under the containing root else its parent as root; no path discovers under harness roots and matches the file stem, else `TranscriptNotFound`. `PrepSourceSet {harness, label, root}` exists in `core::prep`, so the composition root can build it as `prep_api` does. Parent-as-root removes the `strip_prefix` failure I flagged. | closed |
| F-02 fake | `call` step takes `sighting` (first default / update; unknown value → InvalidData) and `msg_id`; partial `facts` merge over `SessionFacts::default()`. Test proves two call rows, second `Update` with `msg_id = "a"`, turn origin, compaction event, facts, checkpoint resume equality, `FAIL`. Guide adds vd-000a; tk-0002 explicitly tests max-merge of update sightings. | closed |
| F-03 cursor | Signature is `status_incremental(&StatusTarget, Option<&StatusCursor>, now_ms)`; caller keeps cursor on error; opaque, Clone + Send + Sync, compact accumulators listed; `target-changed` reset reason; "incremental == cold except source". | closed |
| F-04 ownership | SDK fills `target` including the transcript path read; CLI always fills `resolved` (basis explicit for `--session`) and copies `target.transcript`. Core doc comments updated to match; no shape change. | closed |
| F-05 vocabulary | `core::status::UNKNOWN_FACTS` (13 names); `SessionStatus::empty` lists all; contract test asserts equality and distinctness. | closed |
| F-06 proofs | cp-0001 now cites vd-0003, vd-0004, vd-0006, vd-0008. | closed |
| F-07 total/accumulators | Negative or missing `ContextSample.total` → unknown; accumulators bounded (model spans, turn pairs, last-call row, limit notices). | closed |

## Advisories (non-blocking, for the tk-0002 coder)

- A-01: `model.pending_switch` starts in `UNKNOWN_FACTS`. When the fold ran and there is no switch, the SDK should remove the name (known: none); only harnesses without switch markers keep it. Say so in the tk-0002 packet if not already covered by "removes each as filled".
- A-02: the closed list has no name for turn counts; that is correct only while turn counts are always known from the fold. If a dialect ever lacks turns, add a name in a contract bump, not ad hoc.

## Seal
Nothing further needed before seal. No delta findings.
