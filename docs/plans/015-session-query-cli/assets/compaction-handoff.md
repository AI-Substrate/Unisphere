# HOVR/2 — plan015 compaction handover

## Meta / intent
PM `pij-empirical-tiger`; Main `pij-female-varl`.
Workspace: `/Users/jordanknight/substrate/unisphere/unisphere-session-query-cli`, branch `builder/015-session-query-cli`.
Native PM root: `/Users/jordanknight/substrate/unisphere/unishpere-main`; explicit cwd/absolute writes required.
Relative paths use the product workspace; plan assets live in `docs/plans/015-session-query-cli/assets/`.
Goal: SDK-owned queries, thin 27-leaf CLI, seven workflows, offline recipes, useful actions and safe recovery.
User “ye” authorized the manual CLI; latest “continue” lifts the compaction pause. Current continuation truth: `assets/team/continuation-status.json`.

## Timeline / proof
Nine coder lanes delivered and were integrated/corrected.
Last proved source: `5ee631b46142b88dea2a2742c9cb8087642c4d25` — 327 tests, clippy/rustdoc and full boot.
Boot scope: `configuration-and-native-session-projections`, **not CLI readiness**.
Previous PM publication: `4f4a6eef348fac18508b438ed9b216a8c982d460`.
Corrections source `771f94a6d478dd50d74b661b2592d67bd0eeaa1a`: formatted P4/P6/P7 corrections and three-way grouping regression. Package checks exercised; exact-source boot capture pending. Never assign the prior 327-pass result to it.
Checkpoint lookup: `git log -1 --format=%H -- docs/plans/015-session-query-cli/assets/compaction-handoff.md`.

## Held manual CLI — reuse, never recreate
Peer `pij-unfortunate-rat`, OMP `github-copilot/gpt-5.6-sol-fast`, high.
Clone: `/Users/jordanknight/substrate/unisphere/unisphere-query-coders-015/tk-000b-manual`; branch `work/015-manual-cli`.
Held HEAD: `f4c8b3570a851f9008183fff3f045d8f30a6307b`; source is the previous PM publication above.
Worker reports clean worktree, no pending jobs, landed note. **Checkpoint only, not accepted delivery.** Commit preserved in the PM repository under `refs/builder/015/manual-cli-checkpoints/`.
Full scope/status: plan `assets/team/manual-cli-packet.json` and `manual-cli-allocation.json`; clone packet `.harness/temp/manual-cli-packet.json`.
Grammar, docs/schema, staged output, actions and parser tests are authored; all unrun. Still needs corrections, legacy/app handoff and final `ProductOnlyDelivery`.
Published APIs: `parse`, `ParsedCommand`, `run_query`, `run_docs`, `run_schema`, `emit_parse_failure`. Exact signatures are in packet/clone. `requested_session_adapter` removed without shim; PM migrates app callers.
Canary timed out pending; root/source/model binding observed, no provider attestation. Preserve the recorded distinction.

## Review / WIP
Original review remains historical; read it with `assets/reviews/p1-a5-supplemental-pij-armed-cow.json`.
- P1 closed: external composition and both SDK recipes ran against exact clean `5ee631b4`; proof publication `5fb8949112ea98cbafe42cd9506d49b1726fbeb9`. The query recipe demonstrates typed missing-source recovery, not returned rows.
- A5 resolved by `assets/reviews/schema-binding-addendum.json`; frozen guide stays unchanged.
- P2/P3: runtime docs, parser/examples and existing Git-ai convergence remain pending.
- P4: bidi regression fails before correction and passes after. P6: typed outcomes. P7: loader security comments restored. Source `771f94a6` awaits exact boot and follow-up source review.
- P5 withdrawn: FieldValue serializes with a kind tag, so the alleged null collision never existed. Original stable group keys restored; consumer regression proves absence/null/literal-null separation.
- P8 bounded staging remains accepted.

## Immutable boundaries
Baseline `72d9d0cf` and historical receipts stay immutable. Guide v6 hash: `5caed35ffa6f28570c16b6bc42baf47ce4403012069e429eeeaeb4e02f04c88f`.
Builder0.14 has a confirmed cycle: CLI readiness needs predecessor composition; composition needs all ten coders. `--already-integrated`, `--integration-sha`, `--adopt-peer` do not repair it.
Manual CLI is **product-only/external**, not Builder dispatch. No fake receipts, automatic reconciliation, global repair, push/merge or governance writes.
SDK owns semantics; one app registry; clean raw streams; explicit consent/provenance. PM owns proof; no validation during coder edits. Use `xd://pij_send`, never transport logs. Main owns Git-ai convergence.

## Resume
1. Finish captured boot for `771f94a6` (`bg_2`); the first Eval240s attempt timed out without a persisted result. No success inferred.
2. Publish supplemental review/A5 binding and obtain P4/P6/P7 source review.
3. Resume the **same** CLI peer from held HEAD; finish typed frontend scope, migrate app callers, run actual parser/output/SDK/installed scenarios and independent review. Main owns the existing Git-ai convergence boundary. New optional Pij-ID lookup is research only, outside this frozen implementation.
CLI task file: `/Users/jordanknight/substrate/unisphere/unisphere-session-query-cli/docs/plans/015-session-query-cli/assets/tasks/phase-2/tasks.dd.json`; local `tk-0003`, guide unit `tk-000b`.
Command: `/builder 6 implement --plan "/Users/jordanknight/substrate/unisphere/unisphere-session-query-cli/docs/plans/015-session-query-cli/plan.dd.json"`.

## Refs
Plan `assets/team/{unit-deliveries,implementation-fleet,delivery-status}.json`; `assets/verification/{first-wave-proof-5ee631b4,builder-staging-gap}.json`; `assets/verification/c25-*`.
Retro `.harness/records/retro/2026-09-10/001-plan015-first-wave.md` saved ten observations; only Tiger’s bucket cleared. Old smoke scratch removed; coder clones retained.

## Terminal cleanup
Eleven completed plan015 coder seats/windows were retired; their processes and descendants are gone. Main also retired plan014 Araminta. All clones/refs remain. Only held `pij-unfortunate-rat` remains under Tiger; keep PM/Main/reviewer sessions. Do not send work to retired coders without explicit revival. Details: plan `assets/team/terminal-cleanup.json`.
