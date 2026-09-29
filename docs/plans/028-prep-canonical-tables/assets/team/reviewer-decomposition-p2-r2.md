# Reviewer packet — Plan 028 phase-2 decomposition review (guide v3), round 2

**From** PM `pij-native-tick` · **Reviewer** `pij-panicky-anteater` · **Date** 2026-09-29

**You own:** `docs/plans/028-prep-canonical-tables/assets/reviews/decomposition-p2-r2.md` and `…/decomposition-p2-r2.receipt.dd.json` (absolute paths under `/Users/jordanknight/substrate/unisphere/unisphere-prep/`).

**Your job:** a short delta review. Round 1 (rv-028-decomposition-p2-r1) approved guide v3 and it was sealed, but `harness builder dispatch` then refused every lane (E471 "Dependency composition is absent, unverified or bound to a different baseline"): readiness treats any depends_on unit whose proof checks are not in the baseline proof as an in-flight dependency. The only change in this round: the seven phase-2 lanes now declare `depends_on: [tk-0001]` (the sealed contract unit) instead of tk-0002/tk-0003/tk-0004/tk-0005/tk-0006; their `reads` of phase-1 reference code are unchanged (now advisory read-owner warnings, 9), and `baseline.receipt` moves to `team/baseline-p2r2.dd.json` (seal receipts are immutable). Decide whether that is architecturally correct (lanes need only the contract; the phase-1 code they read is in the sealed source) and whether the guide may be re-sealed at this subject.

## Bindings

| Binding | Value |
|---|---|
| Scope | decomposition |
| Exact subject SHA | the commit adding this file (`git -C /Users/jordanknight/substrate/unisphere/unisphere-prep log -1 --format=%H -- docs/plans/028-prep-canonical-tables/assets/team/reviewer-decomposition-p2-r2.md`) |
| Product plan | `docs/plans/028-prep-canonical-tables/plan.dd.json` sha256 `7da3d4a4d05949ddba02368c80dbe145189a63214c43bd0335c381e36ce64e46` (unchanged since round 1) |
| Implementation guide v3 (rev 2) | `docs/plans/028-prep-canonical-tables/assets/impl-guide.dd.json` sha256 `c6b8c852ae59839b1f383d0387625c32ab1be79fe0b2bc5b6dd31a373ef27a3a` |
| Requested role | `{"role":"reviewer","harness":"omp","model":"github-copilot/claude-sonnet-5.5","source":{"harness":"guide","model":"guide"}}` |
| Structural check | `harness builder guide … --check` → ok; warnings: 26 write-overlap, 9 read-owner |

## Return

Report + `builder/team` receipt proposal `id:"rv-028-decomposition-p2-r2"`, `recorded_at` later than round 1, `scope:"decomposition"`, subject SHA, the bindings above, observed runtime, verdict, findings. Validate with ddocs; message the PM with verdict, counts, paths and SHA-256. Same boundaries as before (read-only, your two files only, no real content).
