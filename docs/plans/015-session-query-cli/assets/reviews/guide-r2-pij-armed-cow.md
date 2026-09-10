# plan015 decomposition re-review — R2

Reviewer: `pij-armed-cow`. Observed runtime: OMP harness; exposed model id `github-copilot/claude-opus-5`; reasoning effort exposed as `high`; no provider attestation is claimed beyond those strings. Observed native root `/Users/jordanknight/substrate/unisphere/unishpere-main` — not this worktree; no `cd` or native rebind is claimed, and every operation below used absolute reviewed-workspace paths.

## Basis

- Subject `42a6cc0ecf82716827a29f42a4a84496e18bf67f`; `git rev-parse HEAD` in the reviewed workspace equals it, and the only untracked path is the R2 packet.
- Plan recomputed `9fbb66391ed715e2a13909251664eb7eb726bc3441e6d6ad444ca26fc74d1f9d` — unchanged from R1 and matching the packet.
- Guide v3 recomputed `d2c7533744f283d8edb6d69afdb19d6f242bdb0e89312caf9dff81f6e9983f70` — matching the packet.
- My R1 artifacts are preserved byte-identical: `guide-r1-pij-armed-cow.md` still hashes `bab9e359…0f1d` and `guide-r1-pij-armed-cow-receipt.json` still hashes `a0079a4f…9666`.
- Delta reviewed: 13 files between `e1da0251` and `42a6cc0e`, all under `docs/plans/015-session-query-cli/`. No crate, manifest or harness file changed; the packet's claim that no source behaviour exists is consistent with the diff.
- Read: `guide-r1-dispositions.json`; guide contracts C1–C24 with the full `units`, `composition`, `checks`, `capabilities`, `baseline` and `fan_out` sections; `query-contract.md` delta and new sections; `command-catalog.json` (27 leaves); `workflows-and-command-reference.md` delta; `backpressure.dd.json` (24 rows); `verification/guide-authoring-v3.json`; `team/review-decomposition-r1.dd.json`. Committed plan014 grammar re-read via `git show 9250b8f6…:crates/cli/src/sessions.rs` and `:crates/cli/src/git_notes.rs`.
- Read-only. No product, plan, guide, task, flow or team-record edit; no build, test, lint or formatter; no commit, allocation or interview. Writes confined to the two packet-named paths.
- Canary history unchanged and unretouched: the original dispatch command returned `E-RS-CANARY-PENDING` at its deadline for `dispatch-6362a00428d7fe36bf5f7d4e88a53a7d`; the matching acknowledgement arrived afterwards and correlated late (`state: acked`, seat `pij-armed-cow`, at `1789001509319`). That command is not retroactively a pass.

## Verdict

**changes-requested** — narrowly. All eight R1 findings are genuinely fixed in the guide, and I verified each against the frozen bytes rather than against the disposition text. One new high finding (F9) and one low (F10) arise from the v3 edits themselves. F9 is exactly the class of defect the fixes were meant to eliminate: it sits inside the C21/C23 type set that `composition` now seals before wave 1, and the guide's own rule is that a changed shared field returns to `tk-0001` and invalidates dependent proof — so it is much cheaper now than after the seal.

This approves no implementation and no baseline. All 24 backpressure rows remain `unchecked`, and `verification/guide-authoring-v3.json` correctly records `new_query_implementation_executed: false` and `independent_reapproval: "pending"`.

## R1 disposition assessment

| id | claimed | assessed | evidence |
|---|---|---|---|
| F1 | fixed_in_guide | **fixed** | C20 |
| F2 | fixed_in_guide | **fixed** | C21, `tk-0001`, `composition` step 2 |
| F3 | fixed_in_guide | **fixed** | C2, C6, C22, `tk-0007` |
| F4 | fixed_in_guide | **fixed** | C8, `query-contract.md` |
| F5 | fixed_in_guide | **fixed** | C23, `query-contract.md` |
| F6 | fixed_in_guide | **fixed** | C24, `tk-0002`–`tk-0006` notes, `tk-0008` |
| F7 | fixed_in_guide | **fixed** | C17 |
| F8 | fixed_in_guide | **fixed** | C12, C18, C21 |

**F1.** C20 replaces argv scanning with one root parser and an enumerated `ParsedCommand`, and states discrimination explicitly. I tested the rule against the committed legacy grammar rather than against the prose. `SessionCommand::List` declares `#[arg(long)] root: PathBuf` with no default, so every currently-valid `sessions list` already carries `--root`; C20's "`--root` is legacy" branch therefore steals no working invocation. Legacy `List` has no `--adapter` field at all, and `run_git_notes`'s `SourceArguments` requires both `--adapter` and `--repo` and has no `--root`, so C20's "`--adapter` without `--root` is git-ai notes listing" matches what the binary actually accepts today. The residual `--adapter git-ai --root PATH` combination is resolved by C20's "but no `--root`" ordering and exits 2 either way, as it does now. All six catalogued `sessions` leaves route unambiguously: `list --repo`, `show`, `tree`, `stats` and `extract` are query; `export --adapter claude-code --input` is native, matching C20's "`sessions export` is always native" and its `--input` disambiguation. The `--adapter` → `--source-adapter` rename is coherent because no catalogued query leaf ever used `--adapter`; the only catalogue occurrences are `adapters list --json` and the native export. `tk-000b` and `tk-000d` now own the routing work explicitly, including deleting `requested_session_adapter` and migrating callers, and C20 requires regression proof over old forms, new forms and rejected mixtures.

**F2.** C21 enumerates every type I named — `InspectedSource`, `Coverage`, `AvailabilityIssue`, `BranchEvidence`, `SavedFormat`, `ContentAccess`, `RecoveryAction`, complete `QueryOutputOptions` — plus `SourcePartition`, `AssociationObservation` and `FormatCapability` that the fixes introduced. The enumeration is at the rigour I asked for: closed variant lists, no free-form diagnostic strings, ordering derivations named for the types used in sets and maps. The gate I asked for now exists verbatim in `composition`: "Only after that reviewed, digest-bound baseline receipt exists, start the nine wave1 units", with sealing in `team/baseline.dd.json` and the return-to-`tk-0001` invalidation rule. `tk-0001` restates the freeze set by name.

**F3.** `QuerySource::load` now takes `&SourceSelection` (C2), C22 defines the include/exclude algebra and forbids enumerating or reading excluded registrations, and C6 requires the provider to prune registrations before globbing, opening or decoding. The precise failure-semantics sentence I asked for is present: "Excluded registrations cannot fail the query or force allow-partial." C22 also closes an adjacent hole I had not raised — unknown live identifiers return typed alternatives before source reads, and offline filters do no live registry lookup. `tk-0007`'s interface carries the new signature.

**F4.** C8 now reads "Only `--range` requires exactly one selected session and branch", with context partitioning per admitted group and "multiple sessions alone are not ambiguous". The catalogue example that the old wording rejected — `turns extract --repo . --has-tool-family shell --has-errors --context-before 1` — is now valid. `AmbiguousBranch` is correctly narrowed to an actually ambiguous selector or unresolved membership. C8 adds a constraint I did not ask for and consider right: context may expand past time filters but never past the admitted repository/source/participant authority.

**F5.** C23 introduces `ResultUniverse` with separate `rows_complete_for_selection` and `partitions_complete`, digests for selection and columns, `applied_limit` and `bounded_by_input`. It states the three things I asked for: complete selected matches do not imply a complete partition universe; saved-subset statistics are bounded-by-input and never totals; offline context or reconstruction refuses with `InputSubset`/`UseCompleteInput` rather than silently shortening neighbours. It also closes the EOF-completeness illusion for standalone JSONL and forbids inventing a summary row or opening sidecars, and it bounds `execute_view` against widening. This is a more complete fix than my finding required.

**F6.** Each parser lane now carries distinct, falsifiable done conditions in its `notes` — Claude/Codex initiating-user versus tool-result discrimination and exact `tool_use.id`/`tool_result.tool_use_id`; OMP/Pi header/parent/compaction retention with Pi duplicate IDs as conflicting evidence; Copilot current-versus-legacy view separation with unavailable chat time; VS Code v1/v2/v3 containment agreeing with reduced journal state at exact revision provenance; Cursor idless/timeless/result-less facts and key-validated main-spine membership. Each states the same fact-versus-SDK boundary ("lane contributes … to ac-0008/ac-0009; SDK owns …"), which is the discrimination I asked for. The shared acceptance-ID arrays are unchanged, but the acceptance arrays are no longer the only per-lane target, so the finding is satisfied. C24 adds `tk-0008`'s five ordered milestones M1–M5 against the same SDK test target, with an explicit prohibition on fake completed units, stub releases and test quotas.

**F7.** C17 now names `app::adapters::{ADAPTERS,dispatch,run}`, distinguishes the plan015 base tree from accepted plan014 `9250b8f6`, and cites `dispatch is adapters.rs:284-304` — the value I measured.

**F8.** C12 states CSV is explicitly lossy for projected absence, null and empty string, requires per-result guidance pointing to JSON/JSONL, and denies any CSV round-trip claim; C18 carries `formats: Vec<FormatCapability>` with a `FormatLoss` enum including `AbsenceNullEmptyCollapse`; `schema show` now exposes format losses.

## New findings

### F9 (high) — the QueryView digest basis is circular and response-scoped after C21/C23

C5 is unchanged from v2: "QueryView is immutable; its digest covers schema/reconstruction versions, sorted source IDs/revisions/**coverage** and selected repository associations." The v3 edits changed what "coverage" contains, and C5 was not adjusted. Four consequences, each anchored:

1. **Self-reference.** C3: `NativeQueryView owns … coverage:Coverage`. C21: `Coverage {…, universe:ResultUniverse}`. C23: `ResultUniverse {source_view_digest:Option<Digest>, …}`. If `source_view_digest` denotes this view, the digest is an input to its own preimage. `Option` and `UniverseBasis=LiveView|SavedSelection|ProvidedRows` suggest an intended escape — populate it only for saved input — but nothing states that, and C23 explicitly puts the universe on the live view too: "QueryView records its admitted scope, SourceSelection, fields and universe." Even under the charitable reading, if the field is populated on the emitted envelope after hashing, the digest stops being reproducible from serialized output.

2. **Response-scoped fields inside a view-scoped hash.** `applied_limit`, `columns`/`columns_digest`, `rows_complete_for_selection`, `bounded_by_input` and `basis` are per-request values, not properties of the loaded sources. Hashing them transitively makes the view digest change when only `--limit` or `--columns` changes. That collides with C10, which deliberately separates "view digest" from "normalized request digest" and already places "selection, projection/content policy, sorting, branch/range/context and operation" in the request digest. With C5 as written, a legitimate re-issue at a different page size is indistinguishable from genuine source change, and C10's "changed source view … refuse StaleCursor" mis-fires on ordinary pagination.

3. **Two contradictory placements of the same value.** C21 nests `universe` inside `Coverage`; C19 and `query-contract.md` both put it beside coverage — `data:{…rows,coverage,universe,matched,emitted,next_cursor}`. C23 says only that "serializers preserve it". Whether `data.universe` and `data.coverage.universe` are the same value, duplicated, or divergent is undefined, and it determines what the digest covers.

4. **The basis does not bind what now determines the view.** C5's enumerated basis names neither `SourceSelection` (C22) nor `ContentAccess`/retained fields (C21), although C23 says the view records both. Selection currently leaks in only incidentally through coverage counts and `excluded_adapters`, which is coincidence rather than contract; retained-field capability is not bound at all, so two views over identical sources with different `inspect_fields`/`emit_content` can share a digest while C23 requires `execute_view` to refuse widening to unavailable fields.

**Minimal fix.** Restate C5's basis as an explicit closed field list over view-scoped inputs only — schema/reconstruction versions, sorted source IDs/revisions, selected repository associations, the source-read facts of `Coverage`, plus `SourceSelection` and the retained-field capability of `ContentAccess` — and exclude `ResultUniverse`, cursors and next actions by name. Then resolve the placement: either move `universe` out of `Coverage` to match C19 and `query-contract.md`, or keep it nested and correct those two. This belongs in the wave-0 freeze set, before `team/baseline.dd.json` is sealed.

### F10 (low) — no catalogued leaf exercises `--source-adapter`

`--source-adapter` is the flag that makes C20's native/query discrimination observable, and C20 cites `sessions list --repo . --source-adapter git-ai` as its own worked example. `command-catalog.json` is unchanged in v3 and contains zero occurrences of it across all 27 leaves. Since bp-000d proves "all command leaves" and C20's regression obligation is framed as "every new catalogue form", the one flag that distinguishes a logical query from native note inventory would not be exercised by the catalogue-driven proof unless a lane adds it independently.

**Minimal fix.** Add the C20 example as a catalogue leaf, or state in C20 that the routing regression set is `tk-000b`'s and not derived from the catalogue.

## Accepted correction to my R1

`guide-r1-dispositions.json#factual_notes[0]` is right and I accept it. My R1 receipt said "every declared check is BUILD or EXTEND". That is inaccurate as written: `bp-000f` is `RUN` and `bp-000e`/`bp-0011` are "EXTEND then RUN" — and all three were already so at `e1da0251`, so this was my error, not a v3 change. The backpressure modes are 16 BUILD, 7 EXTEND and one EXISTS (`bp-0015`, human-judgement). The substantive claim the shorthand supported is unaffected and still verified: all 24 rows are `state: unchecked` and no new query behaviour is proven.

## Process and representation checks

- **My R1 was preserved, not softened.** `team/review-decomposition-r1.dd.json` reproduces all eight findings with byte-identical `description`, `severity`, `disposition` and `evidence`, the same `verdict`, `subject_sha`, `reviewer_id`, `recorded_at`, `scope` and `requested` block, and an `observed` block whose 10 evidence entries and 5 gaps — including the canary-timing gap — are identical. The only difference is the documented `session` → `native_session` key rename and hoisting `bindings.{plan,guide,report}` to root. The raw proposal is retained unchanged alongside it.
- **The dispositions file does not overclaim.** It records `fixed_in_guide`, not `fixed`, and states explicitly that this "records design fixes awaiting R2, not a retroactive approved verdict".
- **`guide-authoring-v3.json` is scoped honestly** to "Guide R1 design corrections and DD/structural validation only", with `independent_reapproval: "pending"`. Its five recorded basis digests are correct: I recomputed `backpressure.dd.json` = `fade96da…`, `query-contract.md` = `679833ac…` and `workflows-and-command-reference.md` = `1063cf84…`, all matching. Its runs are Builder `guide --check` and `ddocs validate`/`build` only — document well-formedness, nothing else.
- **No regression in the honesty apparatus.** 24 criteria, 24 rows, one row each, every one `unchecked`; C17 still limits the existing passing boot to native projections and still declines to treat catalogue/launch configuration as provider attestation.
- **The `--repo` overload is now resolved rather than merely renamed** — C20 and `query-contract.md` both state that `--repo` is association scope on query forms and the Git repository on explicitly native git-ai list/export, and `--input` is likewise split.

## Limitations

I read documents and committed bytes and executed nothing beyond read-only hashing and `git show`. No proposed behaviour was run; no finding asserts that any code exists. F9's fix is a proposal and is not implemented. I did not re-derive the R1 findings from scratch — for each I checked the specific v3 text against the mechanism the original finding named, and against the real committed grammar where the fix made a compatibility claim.
