# Plan015 independent decomposition re-review — R3

Re-review the two R2 findings F9/F10 and confirm no regression of the eight already-fixed R1 findings; this is guide-only review, not implementation/baseline approval.

Subject b3d758c1ba4dec5fbad01c549fd54d5bfd9f52ef; workspace /Users/jordanknight/substrate/unisphere/unisphere-session-query-cli.
Plan docs/plans/015-session-query-cli/plan.dd.json SHA256 9fbb66391ed715e2a13909251664eb7eb726bc3441e6d6ad444ca26fc74d1f9d.
Guide v4 docs/plans/015-session-query-cli/assets/impl-guide.dd.json SHA256 68229e734a08f1ba17ec42297e32caa27cf513e107adb7075043b70b9e420b39.
Reviewer remains pij-armed-cow, OMP github-copilot/claude-opus-5/high (guide setting source). Preserve actual main native root, absolute reviewed-workspace paths, late-canary history and no provider attestation.

Read current C3/C5/C19/C21/C23/C25 plus affected unit/composition freeze text; query-contract.md digest/universe clauses; command-catalog.json sessions-list argv and matching workflow example; assets/reviews/guide-r2-dispositions.json; verification/guide-authoring-v4.json. R1/R2 original reports/proposals are immutable and both canonical changes-requested receipts were accepted by harness builder review.

F9 change: Coverage no longer owns any universe; QueryResponse emits exactly one data.universe beside data.coverage, constructed after hashing. Closed ViewDigestBasis explicitly binds schema/reconstruction versions, admitted scope/roots, SourceSelection, source IDs/revisions/representation/policy version, selected associations, only source-read facts, ContentAccess/actual retained fields and immutable input origin. It explicitly excludes ResultUniverse/current request/response/format/cursor/action data. C25 defines canonical encoding, no recursive preimage, and metamorphic baseline proof distinguishing view identity from request identity. Challenge any remaining cycle or request-scoped leak rather than assuming code will fix it.
F10 change: sessions list's primary catalogue argv includes --source-adapter claude-code, matched in the workflow reference. tk-000b explicitly owns the full routing regression matrix including query --source-adapter git-ai versus native --adapter git-ai, not just catalogue-derived leaf coverage. Still 27 leaves and 24 unchecked ACs.

Allowed writes ONLY:
/Users/jordanknight/substrate/unisphere/unisphere-session-query-cli/docs/plans/015-session-query-cli/assets/reviews/guide-r3-pij-armed-cow.md
/Users/jordanknight/substrate/unisphere/unisphere-session-query-cli/docs/plans/015-session-query-cli/assets/reviews/guide-r3-pij-armed-cow-receipt.json
No other edits, commits, source implementation, builds/tests/linters/formatters, allocations or user interview. Follow C10 at skill://pij/references/00-routing.md:207-219.
Return approved|changes-requested|blocked plus bound root-field record_type=review proposal (plan/guide/report path+sha256, subject, requested/observed identity and gaps, findings severity high|medium|low and disposition open|fixed|accepted). Preserve prior finding assessments in the report; raw supplements are fine. PM imports canonical DD. Do not pre-check the future wave0 baseline or any product behavior. Send only verdict/report/proposal pointers and material IDs.
