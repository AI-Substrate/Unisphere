# plan015 decomposition re-review — R3

Reviewer: `pij-armed-cow`. Observed runtime: OMP harness; exposed model id `github-copilot/claude-opus-5`; reasoning effort exposed as `high`; settings source per guide. No provider attestation is claimed beyond those strings. Observed native root `/Users/jordanknight/substrate/unisphere/unishpere-main` — not this worktree; no `cd` or native rebind is claimed, and every operation used absolute reviewed-workspace paths.

## Basis

- Subject `b3d758c1ba4dec5fbad01c549fd54d5bfd9f52ef`; `git rev-parse HEAD` in the reviewed workspace equals it, and the only untracked path was the R3 packet.
- Plan recomputed `9fbb66391ed715e2a13909251664eb7eb726bc3441e6d6ad444ca26fc74d1f9d` — unchanged since R1.
- Guide v4 recomputed `68229e734a08f1ba17ec42297e32caa27cf513e107adb7075043b70b9e420b39` — matching the packet.
- My R1 and R2 artifacts are preserved byte-identical at their original digests: R1 report `bab9e359…0f1d`, R1 raw proposal `a0079a4f…9666`, R2 report `b55a8e8f…148a`, R2 raw proposal `54c34b15…40bb`.
- Delta reviewed: one commit, 14 files, all under `docs/plans/015-session-query-cli/`. No crate, manifest or harness file changed, consistent with the packet's claim of no new source code.
- Read at v4: contracts C1–C25 in full with `units`, `composition`, `checks`, `capabilities`, `baseline`, `review` and `fan_out`; the exact character-level delta of C3/C5/C21/C23 against v3; `query-contract.md` diff; `command-catalog.json`; `workflows-and-command-reference.md`; `backpressure.dd.json` (24 rows); `verification/guide-authoring-v4.json`; `reviews/guide-r2-dispositions.json`; `team/review-decomposition-r2.dd.json` against my raw R2 proposal. Committed plan014 registry re-read via `git show 9250b8f6…:crates/app/src/adapters.rs` to check the new catalogue argv against real adapter ids.
- Read-only. No product, plan, guide, task, flow or team-record edit; no build, test, lint or formatter; no commit, allocation or interview. Writes confined to the two packet-named paths.
- Canary history unchanged and unretouched: the original dispatch command returned `E-RS-CANARY-PENDING` at its deadline for `dispatch-6362a00428d7fe36bf5f7d4e88a53a7d`; the matching acknowledgement arrived afterwards and correlated late. That command is not retroactively a pass.

## Verdict

**approved** — for the guide decomposition only, at guide v4 and subject `b3d758c1`.

Both R2 findings are genuinely fixed, the fix is surgical rather than broad, and no R1 finding regressed. Two new findings are recorded open — F11 (medium) and F12 (low) — and neither blocks. F11 is a typed-vocabulary gap that must be settled where the declarations are actually written, in `tk-0001` before the baseline is sealed; that independent exact-baseline review is a gate that already exists in `composition` step 1, and this approval does not pre-approve it.

This approves no implementation, no baseline, no dispatch and no product behaviour. All 24 backpressure rows remain `unchecked`, and `verification/guide-authoring-v4.json` correctly records `new_query_implementation_executed: false` and `independent_reapproval: "pending"`.

## R2 disposition assessment

| id | severity | claimed | assessed |
|---|---|---|---|
| F9 | high | fixed_in_guide | **fixed** |
| F10 | low | fixed_in_guide | **fixed** |

### F9 — closed nonrecursive view basis

I raised F9 as four legs. All four are closed, and I checked each against the character-level delta rather than the disposition text.

1. **Self-reference gone at the source.** C21 deletes `universe:ResultUniverse` from `Coverage` — that is the literal delete in the diff, not a re-wording — and adds "Coverage has NO universe field. QueryResponse alone owns universe:ResultUniverse beside coverage". C5 no longer enumerates its own basis; it delegates to C25 and adds the negative rule "Never hash QueryView, Coverage or QueryResponse transitively, and never include a ResultUniverse, cursor or next action in that basis." C25 hashes a dedicated `ViewDigestBasis`, not the live graph, and states "The view digest is calculated before QueryResponse/ResultUniverse construction" and "no digest is in its own preimage". C23 supplies the matching ordering: "ResultUniverse is created per response after the view digest exists; its source_view_digest is Some(current_view.digest)". `ResultUniverse.source_view_digest` survives as a back-reference, which is correct — it now points at a digest whose preimage provably excludes it.

2. **Request-scoped leak gone.** C25's exclusion list is explicit and names every leaking field I identified: "ResultUniverse in its entirety (including its source_view_digest, selection/columns digests, applied_limit, per-response completeness/bounded_by_input), current row predicates/time/sort/range/context/limit/output columns, rendered format, cursor values, next actions, and matched/emitted response counts." The one field that could still have leaked silently is handled: `source_read_facts` admits `selected_sources` only as "(admitted, pre-row-filter)", and C21 independently states "selected_sources means admitted sources before row predicates". C25 preserves C10's split by name and states the required metamorphic proof: changing only limit/columns within retained capability leaves the digest equal; changing source revision, admitted selection or retained fields changes it; creating or changing a universe, action or cursor cannot.

3. **One placement.** C21, C25 ("Query JSON emits exactly one data.universe, never data.coverage.universe"), C23 ("There is exactly one wire placement: data.universe beside data.coverage") and `query-contract.md`'s new output-contracts bullet all agree, and C19's envelope — unchanged — already had `universe` beside `coverage`, so the ambiguity was resolved toward the wire format rather than away from it.

4. **Basis now binds what determines the view.** `source_selection:SourceSelection` and `retained:RetainedCapability {access:ContentAccess, fields_by_source}` are in the basis, with the reason stated: they "define the admitted view, not merely its presentation". Three bindings I did not ask for were added and are right: `admitted_scope`/`admitted_repository_roots`; per-source `query_policy_version` in `ViewSourceBinding`, carried through a matching C3 addition to `SourceEvidence`; and `ViewInputBasis::Saved{format,input_sha256,rows_complete,partitions_complete}`, which binds saved input by its own bytes and its *supplied* completeness rather than by the current response's metadata.

Two details show the edit was thought through rather than patched. C5's hashing sentence was narrowed from "Hashes use SHA-256 over domain-tagged…" to "**Entity/source-ID** hashes use…", so the ID rule no longer silently competes with C25's view-digest encoding; and C25 defines that encoding concretely — SHA-256 over ASCII domain `unisphere/query-view/v1`, NUL, then canonical JSON with lexical keys, no floats, no unknown fields, streamed rather than materialised.

Ownership followed the fix: `tk-0001` freezes `ViewDigestBasis`/`ViewSourceBinding`/`RetainedCapability`/`ViewInputBasis` and the canonical encoding with the metamorphic proof "part of the actual baseline check before seal"; `composition` step 1 becomes "C21/C22/C23/C25"; `tk-0008` carries the runtime distinction; the `review` anchors move to C1–C25.

### F10 — routing coverage

`sessions list`'s primary catalogue argv now carries `--source-adapter claude-code`, and the rendered workflow example is the identical string, so the two cannot drift apart silently. The catalogue is still 27 leaves and no other leaf changed. `tk-000b` now owns the matrix explicitly rather than inheriting it from one example per leaf: "explicitly cover query `--source-adapter git-ai` versus native `--adapter git-ai`, legacy `--root` and all rejected mixtures". Both alternatives I offered were taken.

One residual, non-blocking and not a finding: the catalogue example pairs `--harness claude-code` with `--source-adapter claude-code`, and `claude-code` is a real registered adapter id in the committed registry, so the example is valid — but with coincident values it cannot by itself discriminate an implementation that treats `--source-adapter` as an alias of `--harness`, which is the confusion C22's distinct `AdapterId`/`HarnessId` types and C20's "no `--adapter` alias" exist to prevent. The discriminating pair does exist in the owned matrix (`tk-000b`'s named acceptance uses `--source-adapter git-ai` against `--harness claude-code`), which is why this is an observation rather than a finding.

## R1 disposition assessment — carried forward, re-checked for regression

All eight remain **fixed**, and at v4 this is close to mechanical: of the anchors those findings rest on, C1, C2, C4, C6–C20, C22 and C24 are byte-identical to v3, so F1, F3, F4, F6, F7 and F8 cannot have regressed. The two changed anchors were checked field by field.

| id | anchor | v3→v4 |
|---|---|---|
| F1 | C20, `tk-000b`, `tk-000d` | byte-identical contract; `tk-000b` strengthened |
| F2 | C21, `tk-0001`, `composition` | freeze set enlarged, nothing removed |
| F3 | C2, C6, C22 | byte-identical |
| F4 | C8 | byte-identical |
| F5 | C23 | strengthened; no completeness rule weakened |
| F6 | C24, lane notes | C24 byte-identical; `tk-0008` strengthened |
| F7 | C17 | byte-identical |
| F8 | C12, C18 | byte-identical |

C21's only deletion is the `universe` field; every frozen type I enumerated in F2 — `InspectedSource`, `SourcePartition`, `AvailabilityIssue`, `BranchEvidence`, `SavedFormat`, `ContentAccess`, `RecoveryAction`, complete `QueryOutputOptions`, `AssociationObservation`, `FormatCapability` — is still declared. C23's changes are all insertions plus one narrowing of "fields and universe" to "retained-field capability and immutable input-origin completeness"; the saved-input completeness rules, the `InputSubset`/`UseCompleteInput` refusal and the `execute_view` no-widening bound are intact. `units` still 13 across waves 0–3, `checks`, `capabilities`, `baseline`, `isolation`, `roles`, `risks` and `fan_out` byte-identical, catalogue still 27 leaves, backpressure still 24 rows with 16 BUILD / 7 EXTEND / 1 EXISTS and every row `unchecked`.

## New findings

### F11 (medium) — "query mismatch" is required proof but has no typed spelling

C25 requires that "a changed request may reject continuation as query-mismatched, **not be mislabeled source change**". `bp-0007` makes this a proof obligation — its `proof` field builds "stale/malformed/**query-mismatched** continuation cases" and its note now ends "Query mismatch remains distinct from source change" — and `query-contract.md` states the user-facing form: "changing a page size or available output columns is not a source change".

The closed vocabulary cannot express that distinction. C10 — unchanged — collapses both causes into one outcome: "changed source view **or options** refuse StaleCursor rather than silently restart." C14's code list is closed and offers only `StaleCursor`; `ViewScopeMismatch` is already spoken for by C23's `execute_view` widening refusal, so reusing it would be a different mislabel. C21's `RecoveryAction` offers only `StartFreshQuery` for this path, C14's fixed explanation is "stale cursor says start a fresh query", and C21 forbids any free-form diagnostic detail string. Both digests are opaque to the consumer, so a caller who changed only `--limit` and a caller whose sources moved receive an identical typed answer.

I am not calling this a contradiction: `StaleCursor` is a cause-neutral name, so nothing in the guide is false as written. It is an unimplementable proof obligation — a lane writing `bp-0007`'s query-mismatched case has nothing contract-visible to assert on, and would have to invent either a code or a forbidden detail string.

**Minimal fix.** Add one enumerated distinction to the wave0 declarations — a distinct `QueryFailureCode` (e.g. `RequestMismatch`), or a closed reason discriminant on `StaleCursor` — and align C10's single-outcome sentence. This belongs in `tk-0001` before `team/baseline.dd.json` is sealed, because C21's own rule returns a changed shared failure enum to `tk-0001` and invalidates dependent proof.

### F12 (low) — the seal-gating row still names the freeze set as C21–C23

`composition` step 1, `tk-0001` and the `review` anchors all moved to include C25. `bp-0015` did not: its note still reads "Wave1 requires the actual complete **C21-C23** DTO/schema/fake freeze set independently reviewed and sealed at its committed baseline". `bp-0015` is the EXISTS / human-judgement row whose entire purpose is to state what must be independently reviewed and sealed before dispatch, and it is the row a PM or baseline reviewer reads at that gate. The normative statements are correct, so this is a stale cross-reference rather than a substantive hole — but it points the gate at a smaller set than the guide now freezes.

**Minimal fix.** Extend the `bp-0015` note to `C21-C23/C25`.

## Process and representation checks

- **My R2 was represented faithfully.** `team/review-decomposition-r2.dd.json` carries `record_type: "review"`, the same `verdict`, `subject_sha`, `recorded_at`, `scope`, `reviewer_id` and `requested` block, findings byte-identical to my raw proposal, and an `observed` block whose every key and value — including the canary gap and my own accepted self-correction — matches exactly. The only omissions are my supplemental `prior_review`/`prior_findings` keys, which the dispositions file discloses as schema-unsupported and which survive unchanged in the raw proposal and the immutable R2 report.
- **The dispositions file does not overclaim.** Scope is "Guide-only corrections awaiting R3; no implementation, baseline or dispatch approval", disposition is `fixed_in_guide`, and the R1 line reads "All eight independently verified fixed by R2" — attributed to my review rather than asserted by the author.
- **`guide-authoring-v4.json` is honest and its digests are real.** Scope is "R2 design corrections, structural/DD/render checks and planned argv consistency only". I recomputed all four mutable basis entries — `backpressure.dd.json` `14868b79…`, `query-contract.md` `91cff3b9…`, `command-catalog.json` `13f9b0f9…`, `workflows-and-command-reference.md` `190814bf…` — and all match, as do the plan and guide entries. Its five runs are `builder guide --check` and `ddocs validate`/`build` only: document well-formedness, nothing executed of the query product.

## Limitations

I read documents and committed bytes and executed nothing beyond read-only hashing, `git show` and `git diff`. No proposed behaviour was run; no finding asserts that any code exists. F11's and F12's fixes are proposals, not implementations. I did not pre-check the future wave0 baseline, and this approval confers nothing on it: the independent exact-baseline review in `composition` step 1 remains a separate gate, and F11 should be resolved there or before it.
