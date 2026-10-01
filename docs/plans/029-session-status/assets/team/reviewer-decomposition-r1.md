# Reviewer packet — Plan 029 decomposition review, round 1

**From** PM `pij-specific-kiwi` · 2026-09-29 · Wire discipline: C10 (line 1 = verdict/action).

**You own (write only these):**
- Report: `/Users/jordanknight/substrate/unisphere/unisphere-session-status/docs/plans/029-session-status/assets/reviews/decomposition-r1.md`
- Receipt proposal: `/Users/jordanknight/substrate/unisphere/unisphere-session-status/docs/plans/029-session-status/assets/reviews/decomposition-r1.receipt.dd.json`

**You may read:** the worktree `/Users/jordanknight/substrate/unisphere/unisphere-session-status` (read-only apart from the two paths above), Plan 028's worktree `/Users/jordanknight/substrate/unisphere/unisphere-prep` for the fold contract it consumes.

**Your job:** decide whether the committed contract baseline (unit tk-0001) and the guide may be sealed and released to the two wave-1 coder lanes (tk-0002 SDK StatusService, tk-0003 CLI + target resolver).

## Bindings

| Binding | Value |
|---|---|
| Scope | decomposition |
| Exact subject SHA | the commit adding this file (`git log -1 --format=%H -- docs/plans/029-session-status/assets/team/reviewer-decomposition-r1.md`) |
| Product plan | `docs/plans/029-session-status/plan.dd.json` sha256 `72145b14324e289f86c5365f93785f3d76d2024e09595685c3f92a2f8235fa39` |
| Implementation guide v1 | `docs/plans/029-session-status/assets/impl-guide.dd.json` sha256 `c84d28b7b92b21dbba384b756e37a9aab9f69bb68e847e391b87b29176262609` |
| Requested role | `{"role":"reviewer","harness":"omp","model":"github-copilot/claude-sonnet-5.5","source":{"harness":"guide","model":"guide"}}` |
| PM evidence | `cargo test --locked -p unisphere-core --test status_contract` → 6 passed; clippy -D warnings clean on core + testkit; `unisphere-arch-check` → 110 edges accepted |
| Baseline files | `crates/core/src/status.rs`, `crates/core/tests/status_contract.rs`, `crates/testkit/src/status.rs` (commit 4ddd838) |

## What to assess (keep it short — Jordan asked for KISS)

1. Does `core::status` match the guide contracts closely enough to freeze? Any mismatch that would force a post-seal contract change.
2. Are the fakes (ScriptedFold, FakeStatusApi, FakeTargetResolver) enough for each wave-1 lane to prove its behaviour independently?
3. Is every product AC (plan ac-0001..ac-0007) covered by a capability, owner and real check? Is the SDK signature (`status_incremental`) right for an embedding caller (pij-rs Plan 157: explicit target, caller-held cursor, no sqlite, serde status)?
4. Any reason not to seal and dispatch.

## Return

Persist the report and a raw ReviewReceipt proposal (`record_type`, `id` = `rv-029-decomposition-r1`, `recorded_at`, `scope`, `subject_sha`, `plan`/`guide` path+sha256, `reviewer_id`, `requested`, `observed`, `verdict` approved|changes-requested|blocked, `report` path+sha256, `findings` [{id, severity, description, disposition}]). Look at `/Users/jordanknight/substrate/unisphere/unisphere-prep/docs/plans/028-prep-canonical-tables/assets/team/review-decomposition-rv-028-decomposition-r2.dd.json` for the accepted shape. Then `pij send pij-specific-kiwi "<receipt path>"`.

Never edit the plan, guide, flow files (the-flow.json, the-flow.md, .the-flow-state.json), source, or team receipts. No push/merge.
