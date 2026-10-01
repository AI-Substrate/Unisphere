# plan015 guide R3 — scope addendum

Reviewer: `pij-armed-cow`. Same subject and basis as R3; this is not a new review round.

## What this document is

R3 approved the guide decomposition at subject `b3d758c1ba4dec5fbad01c549fd54d5bfd9f52ef` and guide `68229e73…` with two findings recorded `open` and described in prose as non-blocking. Live Builder rejects that combination with **E475 "Approved review contains unresolved material findings"**, because `open` on a `medium` finding reads as unresolved at any scope.

The contradiction is representational, not substantive. My judgment is unchanged: F11 and F12 do not block the guide decomposition, and both are mandatory before the independent wave0 baseline seal. R3 said exactly that in prose; `open` was the wrong disposition token to carry it, because `open` cannot distinguish "unresolved and blocking here" from "unresolved here by design, blocking later".

This addendum restates F11 and F12 at `disposition: accepted`, scoped to decomposition. **R3 is preserved unchanged** at `3bd22e100b8e7513c35df02929e393c2cebd93d0e154d6b1b19caa931a007a49` (report) and `9834f3204f90d06b50c4af4701dc08546169d665a649b1d5a3c0fbf1c386ed30` (receipt); both were re-hashed at the time of writing and are byte-identical. Nothing in R3 is withdrawn, softened or superseded — this narrows a disposition token, and adds nothing to the approval.

## Basis re-confirmed

`git rev-parse HEAD` still equals `b3d758c1…`; guide recomputed `68229e734a08f1ba17ec42297e32caa27cf513e107adb7075043b70b9e420b39`; plan recomputed `9fbb66391ed715e2a13909251664eb7eb726bc3441e6d6ad444ca26fc74d1f9d`. No guide, plan or product byte changed since R3. No new review was performed and none was requested.

## What `accepted` means here, and what it does not

**Accepted** means: the finding is acknowledged, is not fixed, and I accept it as not blocking *the decomposition verdict*. It is a scope statement about R3's verdict only.

It is not a waiver, not a fix, and not a claim that anything was implemented. Both findings remain **mandatory before the independent exact-baseline review and seal** in `composition` step 1. An unfixed F11 at that gate is a blocking condition there, and this addendum gives no one — including me — licence to treat that gate as pre-cleared. If the wave0 declarations are sealed without F11's distinction present in the frozen types, this acceptance does not cover it.

## F11 — accepted for decomposition; the recorded requirement satisfies the finding

`pij-empirical-tiger` has recorded a concrete wave0 requirement: `StaleCursor` exposes a closed `CursorMismatchReason::{QueryOptionsChanged, SourceViewChanged}`; the check tests query binding before view binding; guidance is cause-specific; SDK and CLI tests cover it; malformed tokens continue to fail as `InvalidArgument`.

I checked that against the frozen contracts rather than accepting it on description, and it satisfies F11 as written. Four points, each verified in the guide at `68229e73…`:

- It is the second of the two minimal fixes I named — a closed reason discriminant on `StaleCursor` — and so needs no change to C14's closed code list, which already contains both `StaleCursor` and `InvalidArgument`. Keeping malformed tokens on `InvalidArgument` preserves the existing split rather than reworking it.
- A closed enum is not a free-form diagnostic, so it satisfies C21's "No arbitrary diagnostic detail string is allowed" instead of colliding with it.
- Cause-specific guidance is already expressible: C21's `RecoveryAction` contains both `StartFreshQuery` and `ReopenView`, so the two causes can carry different actions with no new variant.
- Ordering query-binding before view-binding resolves a case my finding did not reach — where request *and* sources both changed — by reporting the cause the caller controls. That is a genuine addition to what I asked for.

The net effect is that the wave0 fix is **smaller and better contained than F11 implied**: one new closed enum plus a precedence rule, touching no code list, no `RecoveryAction` variant, and not requiring C10's sentence to be reworded, since the discriminant sits beneath its existing single outcome.

One boundary, stated plainly: the requirement is *recorded*, not present. The guide at `68229e73…` contains none of it, and I verified coherence with the frozen contracts, not presence in bytes. Whether it lands correctly in the wave0 declarations is for the baseline review to establish, and I make no claim about it.

## F12 — accepted for decomposition

`bp-0015`'s reference will include C25. F12 was a stale cross-reference in a gate-facing note, not a substantive hole, and it was already `low`. Accepted for decomposition on the same terms: still to be corrected before the seal.

## Scope and limitations

Guide decomposition only. This addendum approves no implementation, no baseline, no dispatch and no product behaviour; no code exists on the strength of it. All 24 backpressure rows remain `unchecked`, and `verification/guide-authoring-v4.json` still records `new_query_implementation_executed: false` and `independent_reapproval: "pending"`.

Read-only. No product, plan, guide, task, flow or team-record edit; no build, test, lint, formatter or proof rerun; no commit. Writes confined to the two paths this addendum was authorised to create.

Observed runtime: OMP harness; exposed model id `github-copilot/claude-opus-5`; effort `high`; settings source per guide — exposed strings, not provider attestation. Observed native root `/Users/jordanknight/substrate/unisphere/unishpere-main`, not this worktree; no `cd` or native rebind is claimed, and all operations used absolute reviewed-workspace paths. Canary history stands as recorded in R3: the original dispatch command returned `E-RS-CANARY-PENDING` at its deadline for `dispatch-6362a00428d7fe36bf5f7d4e88a53a7d`, and the matching acknowledgement correlated late. That command is not retroactively a pass.
