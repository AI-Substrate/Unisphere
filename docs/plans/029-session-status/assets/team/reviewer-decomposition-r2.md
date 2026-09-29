# Reviewer packet — Plan 029 decomposition review, round 2

**From** PM `pij-specific-kiwi` · **Reviewer** `pij-easy-seahorse` (same seat as r1) · C10.

**You own (write only):** `docs/plans/029-session-status/assets/reviews/decomposition-r2.md` and `docs/plans/029-session-status/assets/reviews/decomposition-r2.receipt.dd.json` (id `rv-029-decomposition-r2`, same shape as r1).

**Your job:** confirm r1 findings F-01..F-07 are closed as dispositioned and decide whether the baseline may be sealed and released to tk-0002 / tk-0003.

| Binding | Value |
|---|---|
| Scope | decomposition |
| Exact subject SHA | the commit adding this file (`git log -1 --format=%H -- docs/plans/029-session-status/assets/team/reviewer-decomposition-r2.md`) |
| Product plan | `docs/plans/029-session-status/plan.dd.json` sha256 `72145b14324e289f86c5365f93785f3d76d2024e09595685c3f92a2f8235fa39` (unchanged) |
| Implementation guide | `docs/plans/029-session-status/assets/impl-guide.dd.json` sha256 `e967cc78a50983e3beaaaef939f34c3a66b4aba0f179cab539ef8d8d078ee653` (`meta.updated` lists each disposition) |
| Requested role | `{"role":"reviewer","harness":"omp","model":"github-copilot/claude-sonnet-5.5","source":{"harness":"guide","model":"guide"}}` |
| r1 record | `docs/plans/029-session-status/assets/reviews/decomposition-r1.receipt.dd.json` (changes-requested) |
| PM evidence | status_contract 6 passed; testkit `--lib status` 1 passed (new vd-000a); clippy -D warnings clean on core + testkit |

Dispositions: F-01 `StatusService::new(bindings, roots: Vec<PrepSourceSet>)` + transcript rule in the SDK contract. F-02 ScriptedFold call step takes `sighting` (first|update) and `msg_id`; partial facts merge over defaults; baseline test vd-000a. F-03 `status_incremental(&StatusTarget, Option<&StatusCursor>, now_ms)`; cursor Clone+Send+Sync, compact; target-changed reset; incremental == cold except `source`. F-04 SDK fills `target` (incl. transcript read); CLI always fills `resolved` (explicit basis for --session); core doc comments say so. F-05 `core::status::UNKNOWN_FACTS` closed list; `SessionStatus::empty` lists all. F-06 cp-0001 proofs now vd-0003, vd-0004, vd-0006, vd-0008. F-07 negative/missing `ContextSample.total` → unknown; accumulators bounded.

Return: `pij send pij-specific-kiwi "<r2 receipt path>"`. Same boundaries as r1.
