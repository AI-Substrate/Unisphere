# HOVR/2 — plan015 compaction handover

## Meta / intent
PM `pij-empirical-tiger`; Main `pij-female-varl`.
Workspace: `/Users/jordanknight/substrate/unisphere/unisphere-session-query-cli`, branch `builder/015-session-query-cli`.
Native PM root: `/Users/jordanknight/substrate/unisphere/unishpere-main`; explicit cwd/absolute writes required.
Relative paths use the product workspace; plan assets live in `docs/plans/015-session-query-cli/assets/`.
Goal: SDK-owned queries, thin 27-leaf CLI, seven workflows, offline recipes, useful actions and safe recovery.
User “ye” authorized the manual CLI. Latest: “Tidy everything up and write yourself a handover”. **Paused; user controls compaction.**

## Timeline / proof
Nine coder lanes delivered and were integrated/corrected.
Last proved source: `5ee631b46142b88dea2a2742c9cb8087642c4d25` — 327 tests, clippy/rustdoc and full boot.
Boot scope: `configuration-and-native-session-projections`, **not CLI readiness**.
Previous PM publication: `4f4a6eef348fac18508b438ed9b216a8c982d460`.
This checkpoint adds **unformatted/unrun P4/P5/P6 WIP**; never assign the 327-pass result to it.
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
Plan `assets/reviews/first-wave-pij-armed-cow.json`: **changes-requested**, code sound; no rebaseline required. Reviewer `pij-armed-cow`, OMP `github-copilot/claude-opus-5`, high.
- **P1 gates acceptance:** rerun external composition and both SDK recipes against exact proved source above. Working-tree receipt naming base `843b3bb4` is insufficient binding.
- P2/P3: real CLI docs mounting and executable grammar/examples belong to manual coder. Git-ai adapter absent; no invented execution claims.
- P4: bidi escaping + regression added in `crates/output-query/src/lib.rs` and `crates/output-query/tests/query_writer.rs`; **unrun**.
- P5/P6: `crates/sdk/src/query/engine.rs` now uses non-JSON `b"\0absent"` grouping sentinel and typed `Outcome` metric comparisons; **unrun**, grouping regression needed.
- P7: four JSONL safety comments remain to restore; exact wording in report.
- P8 staging accepted. A1/A2/A3/A4/A6 closed; A5 clarified in `assets/query-contract.md`, outstanding in frozen guide enumeration.

## Immutable boundaries
Baseline `72d9d0cf` and historical receipts stay immutable. Guide v6 hash: `5caed35ffa6f28570c16b6bc42baf47ce4403012069e429eeeaeb4e02f04c88f`.
Builder0.14 has a confirmed cycle: CLI readiness needs predecessor composition; composition needs all ten coders. `--already-integrated`, `--integration-sha`, `--adopt-peer` do not repair it.
Manual CLI is **product-only/external**, not Builder dispatch. No fake receipts, automatic reconciliation, global repair, push/merge or governance writes.
SDK owns semantics; one app registry; clean raw streams; explicit consent/provenance. PM owns proof; no validation during coder edits. Use `xd://pij_send`, never transport logs. Main owns Git-ai convergence.

## Resume
1. Close P1 while CLI stays held. Recreate removed smoke packages from plan `assets/verification/first-wave-proof-working.json` fields `external_consumer_source`/`external_manifest_template`; use committed `crates/testkit/fixtures/query-docs/{Cargo.toml.template,sdk-schema.rs,sdk-query.rs}` at the proved SHA.
2. Review/test PM WIP; finish P7/A5. Preserve failed receipts.
3. Resume the **same** CLI peer from held HEAD; finish scope, migrate app callers, run actual parser/output/SDK/installed scenarios and independent review.
CLI task file: `/Users/jordanknight/substrate/unisphere/unisphere-session-query-cli/docs/plans/015-session-query-cli/assets/tasks/phase-2/tasks.dd.json`; local `tk-0003`, guide unit `tk-000b`.
Command: `/builder 6 implement --plan "/Users/jordanknight/substrate/unisphere/unisphere-session-query-cli/docs/plans/015-session-query-cli/plan.dd.json"`.

## Refs
Plan `assets/team/{unit-deliveries,implementation-fleet,delivery-status}.json`; `assets/verification/{first-wave-proof-5ee631b4,builder-staging-gap}.json`; `assets/verification/c25-*`.
Retro `.harness/records/retro/2026-09-10/001-plan015-first-wave.md` saved ten observations; only Tiger’s bucket cleared. Old smoke scratch removed; coder clones retained.

## Terminal cleanup
Eleven completed plan015 coder seats/windows were retired; their processes and descendants are gone. Main also retired plan014 Araminta. All clones/refs remain. Only held `pij-unfortunate-rat` remains under Tiger; keep PM/Main/reviewer sessions. Do not send work to retired coders without explicit revival. Details: plan `assets/team/terminal-cleanup.json`.
