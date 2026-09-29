# Reviewer packet — Plan 028 phase-2 composition review, round 1

**From** PM `pij-native-tick` · **Reviewer** `pij-panicky-anteater` (the same independent seat as the phase-1 and decomposition reviews) · **Date** 2026-09-29

**You own (write only these two files):**
- Report: `/Users/jordanknight/substrate/unisphere/unisphere-prep/docs/plans/028-prep-canonical-tables/assets/reviews/composition-p2-r1.md`
- Receipt proposal: `/Users/jordanknight/substrate/unisphere/unisphere-prep/docs/plans/028-prep-canonical-tables/assets/reviews/composition-p2-r1.receipt.dd.json`

**You may read:**
- Your clone `/Users/jordanknight/substrate/unisphere/unisphere-028-review` (`git fetch origin && git checkout --detach <subject>`).
- The numbers-only evidence under `docs/plans/028-prep-canonical-tables/assets/evidence/` at the subject.
- The execution log `assets/execution-log.dd.json` (lg-0008 onward).

You may run read-only commands, tests and `harness builder on-track` in your clone. Do not read or copy real transcript content. The PM ran the real-corpus proofs into gitignored scratch and committed only numbers.

**Your job:** independently review the phase-2 **composition**: the exact verified artifact below (verify receipt committed at `1e66603`), checked against the product intent (the ph-8b1c ACs and the phase-2 parts of ph-ed41), guide v3 (including its `meta.updated` composition delta) and the phase-2 tasks. Decide approved, changes-requested or blocked.

## Bindings

| Binding | Value |
|---|---|
| Scope | composition |
| Exact subject SHA | the verified artifact `b4958ad6d4b795dcf968795f5d2cf03b16480520`, recorded as `artifact_sha` in `assets/team/composition.dd.json` at the verify-receipt commit |
| Product plan | `docs/plans/028-prep-canonical-tables/plan.dd.json` sha256 `7da3d4a4d05949ddba02368c80dbe145189a63214c43bd0335c381e36ce64e46` |
| Implementation guide | `docs/plans/028-prep-canonical-tables/assets/impl-guide.dd.json` sha256 `6d5131283c31d68f121b0c24701f641705f19af72a966ed4aac159477eaeac6d` |
| Requested role | `{"role":"reviewer","harness":"omp","model":"github-copilot/claude-sonnet-5.5","source":{"harness":"guide","model":"guide"}}` |
| Composition | `harness builder compose --import` replayed the 7 phase-2 deliveries on sealed baseline `cfd716b` (integration `8dc6a6b`: tk-000c, tk-0007, tk-0008, tk-0009, tk-000a, tk-000b including the PM ruling commit, tk-000d). PM composition tk-000e followed: `44b558c`, `9820bb5`, `6161ba6`, `42034b5` (Plan 029 extraction), `8428dc4` (prime rulings), guide delta `dfb8bf8`. `compose --verify` ran vd-0008 (prep proof) and vd-0015 (full boot including harness checks). |
| Evidence | `assets/evidence/`: `coverage-dfb8bf8.json` (vd-0017), `measure-dfb8bf8.json` (vd-000b), `recipes-9820bb5.json` (vd-000c), plus `live-4074389.json` (vd-0016) and `parity-dfb8bf8.json` (vd-000a); earlier runs kept for comparison (`coverage-9820bb5.json`, `measure-6161ba6.json` before the rulings) |

## What to assess

1. **Wiring and ownership.**
   - `crates/app/src/prep.rs` binds every catalogued session descriptor:
     - append JSONL sources through `FileSessionLoader`;
     - Copilot legacy, VS Code documents and journals, and Cursor IDE SQLite through `SnapshotPrepLoader`.
   - An unbound harness (`git-ai`) is reported as unsupported.
   - core, sdk and folds stay pure: arch-check is part of vd-0015.
   - The catalogue sets `cli_persisted_resume` to true for exactly the bound descriptors.
2. **Per-AC evidence.** Say which ACs this subject proves and which remain partial:
   - ac-0006: every harness, representation-appropriate change detection, explicit nulls;
   - ac-0007: honest coverage;
   - ac-0009: DuckDB recipes;
   - ac-000c: fleet-scale measurement and bounded memory;
   - ac-000d: docs.
   - Re-confirm that the phase-1 ACs did not regress.
3. **Prime rulings, landed in composition.** Judge correctness and test quality.
   - (a) Incremental discovery:
     - `unisphere_core::prep::walk_prep_root` re-lists only directories whose identity or mtime changed since the committed per-binding index.
     - Known sources are stat'ed directly.
     - A directory is "racy" if it was modified within 2 s of being listed; its listing is never reused.
     - Tests: `crates/loader-jsonl/tests/prep.rs::incremental_discovery_relists_only_changed_directories_and_matches_a_full_walk`, and the SDK index hand-back test.
   - (b) Commit waves:
     - `PrepRequest.max_run_bytes` (CLI `--max-run-bytes`, default 256 MiB) bounds the rows held before each commit.
     - A failure after a committed wave keeps that wave.
     - The committed state is no longer copied, either in the SDK or in the Parquet store (`Published` view).
     - Tests: in `crates/sdk/tests/prep.rs`, the wave budget, failure-after-wave and index hand-back tests.
   - Check the claim that sources plus skip counts from an incremental walk equal a full walk. Check whether an index-only change that goes uncommitted can ever hide a source; it should only cost a re-list.
4. **Measured results (vd-000b), which you accept or reject explicitly:**
   - cold full corpus: 13.99 GB read, 30.6 s, 57 commits, peak RSS 1,759 MB (footprint 1,408 MB);
   - unchanged re-run: 1.42 s wall, 0.42 s user CPU, 0 of 14,707 directories listed, 600 MB RSS;
   - append re-run: 0.66 s.
   - These were measured at load average ~73–109 on a shared machine.
   - Before the rulings: cold peak RSS was 3.97 GB and the unchanged re-run took 49.5 s (lg-000b).
   - Memory is now bounded by the documented, adjustable wave budget plus the committed state held in full. Is that an acceptable reading of AC-000c's "bounded by documented, adjustable limits"?
5. **Other composition decisions, which you accept or reject explicitly:**
   - (a) `--max-record-bytes` now defaults to the batch limit. Real records reach 13.3 MiB; there is a precedence test.
   - (b) Zero-byte VS Code documents are reported unreadable (exit 3). They are documented and not folded as empty sessions.
   - (c) Prep's glob `*` spans `/`, so nested Oh My Pi and Pi subagent files fold as their own sources. Loader-query uses literal separators.
   - (d) `compactions_v` coalesces unrecorded cache writes to 0 only when the basis is `none`.
   - (e) Snapshot `prep record` resolves `<record key>#<JSON pointer>` and bare document pointers; journal pointers are refused.
   - (f) coverage.py passes on supported, fully accounted sets, with unreadable sources counted by error code; measure.py accepts exit 3.
6. **Tests are behavioural, not wiring echoes. Fixtures are synthetic and content-free.**

## Return

Write the report and a `builder/team` DD receipt proposal:
- `id:"rv-028-composition-p2-r1"`, `scope:"composition"`;
- `subject_sha` = the artifact SHA above;
- `plan` and `guide` path+sha256 bindings from the table above;
- your observed runtime and your verdict;
- findings with `severity` high|medium|low, and `disposition` open unless verified fixed.

An approved receipt may leave only low findings open. Validate with ddocs. Then message the PM with the verdict, finding counts, both paths and their SHA-256. Record your decisions on 4 and 5(a)–(f) explicitly in the report.

## Boundaries

- Read-only; write only your two files.
- No commits, pushes or merges.
- No real transcript content in outputs.
- Stop and say so if you are not running as the requested model.
