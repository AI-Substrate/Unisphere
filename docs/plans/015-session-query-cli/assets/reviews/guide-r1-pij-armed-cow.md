# plan015 decomposition review — R1

Reviewer: `pij-armed-cow`. Observed runtime: OMP harness; exposed model id `github-copilot/claude-opus-5`; reasoning effort exposed as `high`; no provider attestation claimed beyond those strings. Observed native root `/Users/jordanknight/substrate/unisphere/unishpere-main` — not this worktree; no `cd`/native-rebind is claimed, and every file operation below used absolute reviewed-workspace paths.

## Basis

- Subject commit `e1da02513fd509c1d7ffac919a3bdb3f4ffbbd55`; `git rev-parse HEAD` in the reviewed workspace equals it, and the only untracked path is `assets/reviews/`.
- Plan `docs/plans/015-session-query-cli/plan.dd.json` recomputed `9fbb66391ed715e2a13909251664eb7eb726bc3441e6d6ad444ca26fc74d1f9d` — matches the packet.
- Guide `docs/plans/015-session-query-cli/assets/impl-guide.dd.json` recomputed `e55f44f9ffaf1732cf163c7e315aff2354af407ff8cd1d44772b9e9988b7f014` — matches the packet.
- Read: plan (24 ACs, 3 phases), guide C1–C19 in full plus `capabilities`/`units`/`baseline`/`fan_out`/`isolation`/`checks`/`composition`/`review`, `backpressure.dd.json`, `query-contract.md`, `documentation-design.md`, `command-catalog.json`, `synthetic-query-fixture.json`, `workflows-and-command-reference.md`, `verification/guide-authoring-v2.json`, phase task files, `team/` templates. Committed plan014 source read via `git show 9250b8f6…:<path>` in the plan014 worktree; never its working tree.
- Read-only. No product/plan/guide/task edit, no build, test, lint or formatter, no commit/push/merge, no allocation. Writes confined to the two report paths named in the packet.
- Canary timing, recorded honestly: the CLI's original dispatch command returned `E-RS-CANARY-PENDING` at its deadline for `dispatch-6362a00428d7fe36bf5f7d4e88a53a7d`. My acknowledgement arrived afterwards and correlated late; `pij ack` then reported `state: acked`, seat `pij-armed-cow`, packet sha `3ea9165624c2b69afa23766abcdda7662b4773c3b6391be99db29ca39c3c285c`, at `1789001509319`. The original command is not reclassified as a pass; the late correlated ack and the observed runtime are separate evidence.

## Verdict

**changes-requested** — two blocking findings (Q1/Q3 seams), four material findings, two low. This is a strong guide: C1–C19 are unusually concrete for a pre-implementation artifact, the honesty constraints are specific and falsifiable (typed unavailability rather than zeros, `DurationBasis`, closed metric set with a named `failure_rate` denominator, `matched` versus `emitted`, cumulative-snapshot non-summing), the privacy model is a default-deny field registry rather than a warning, and the fan-out rationale for keeping identity/reconstruction/filter/context/statistics under one owner is correct engineering rather than a worker-count quota. The blocking findings are seams that a nine-lane fan-out cannot absorb once coders are running, not disagreements with the design.

Nothing here is implemented, and this review does not approve implementation. All 24 backpressure rows are `unchecked` and every check is BUILD/EXTEND.

---

## Blocking findings

### F1 (high) — the `sessions` namespace and `--adapter` have two conflicting meanings, and no contract fixes the routing

Committed reality at the integration target `9250b8f6`:
- `crates/app/src/main.rs:33-43` routes `argv[1] == "sessions" || "adapters"` into the private app registry; everything else reaches `unisphere_cli::run` for config/help/version.
- `crates/app/src/adapters.rs` `dispatch` (lines 284-304) selects the registry row via `unisphere_cli::requested_session_adapter`, which scans argv for `--adapter` and **defaults to `"claude-code"`** (`crates/cli/src/sessions.rs:51-67`).
- The legacy `sessions list` parser accepts only `--root` and `--max-sessions` (`crates/cli/src/sessions.rs:22-28`).
- Plan014 added a third meaning: `run_git_notes` requires `--adapter git-ai` and takes `--repo` as a *Git repository*.

Proposed surface: `command-catalog.json:96` declares `unisphere sessions list --repo . --name "*Airspace*" --harness claude-code --include-content --format json`, and `workflows-and-command-reference.md:74` declares `--adapter` as a query **identity filter** alongside `--harness`. Under today's composition that invocation carries no `--adapter`, so it routes to the default `claude-code` legacy row and its parser rejects `--repo`/`--name`/`--harness` with exit 2. Conversely `--adapter claude-code` is simultaneously a dispatch selector consumed by the app and a row filter consumed by the SDK, and `--repo` means "association scope" on query leaves but "Git repository" on the git-ai leaf.

The guide does not resolve this. C1 asserts only that "Existing config, catalog, `--root` listing, JSONL export and replacement-snapshot export stay supported"; `tk-000d` says "docs/schema routed first" and "ADAPTERS remains the sole registered descriptor/source/runner/factory table". Neither states who owns `sessions <op>` argv, nor the precedence between legacy adapter dispatch and the query engine, nor the single meaning of `--adapter`/`--repo` per leaf. This is load-bearing for ac-0004 ("existing … invocations continue to work without a second application registry") and for Q6, because a suggested next action must be expressible in real grammar.

**Minimal fix.** Add a routing clause to C1 (or a new contract) fixing: (a) the component that owns `sessions <op>` argv and how query and legacy dispatch are discriminated — the honest options are a reserved legacy form (`--root`/`--input` present ⇒ legacy), a distinct query namespace, or an explicit mode selector; (b) the one meaning of `--adapter` per leaf, renaming the query filter if it stays overloaded; (c) `--repo` disambiguation between association scope and the git-ai Git repository. Then make it a named acceptance on `tk-000b`/`tk-000d` with a `bp-000b`/`bp-000f` regression asserting every existing invocation still exits 0 with unchanged output.

### F2 (high) — the types on the five-lane parallelism seam are named but never specified

`QueryAdapter::inspect(&self, input: NativeQueryInput<'_>, access: ContentAccess, limits: &QueryLimits) -> Result<InspectedSource, QueryFailure>` (C2) is implemented independently by `tk-0002`–`tk-0006` in wave 1. `InspectedSource` is its return type and the single artifact those five lanes must agree on, yet the guide never gives its fields — six mentions, no definition. The same applies to `Coverage` (referenced by `NativeQueryView` and by saved-input validation), `AvailabilityIssue` (the carrier of ac-0003's "typed availability"), `BranchEvidence` (load-bearing for ac-0008), `SavedFormat`, `RecoveryAction` (ac-0018's recovery payload) and `ContentAccess` (described behaviourally in C12, no shape). `QueryOutputOptions` is only partly given, in C19.

This is conspicuous because the guide *does* enumerate the comparable types exhaustively: `Observation` and every `ObservationFacet` variant (C4), `SourceEvidence`/`SourceRef`/`NativeLocator` (C3), `QueryRequest` (C9), `DatasetSchema` and all six row shapes (C18). The omissions sit precisely on the cross-lane boundary.

`tk-0001` is chartered to "freeze and review exported signatures, not a stub query implementation", which is the right mechanism — but no gate says the nine wave-1 lanes may not start until that frozen set is reviewed, and `composition` step 2 says only "Start the nine wave1 coder units in explicit isolated clones from the same baseline."

**Minimal fix.** Either enumerate those seven types in C2/C4 at the rigour already used for `Observation`, or name them explicitly as the wave-0 freeze set and add a gate to `composition`: wave 1 dispatch requires a reviewed `team/baseline.dd.json` receipt covering exactly those signatures, with signature changes returning to `tk-0001` and invalidating dependent proof (a rule `tk-0001` already states for itself).

---

## Material findings

### F3 (medium) — source selection cannot influence loading, so an excluded store can fail the query

`QuerySource::load(&self, scope: &QueryScope, limits: &QueryLimits, access: ContentAccess)` (C2) receives no selection predicate, while harness/adapter identity live in `QueryRequest.filters` (C9) and are applied by the SDK after load. Under `QueryScope::Repository`, `--harness claude-code` therefore still enumerates and decodes every registered store beneath the scope. C6 bounds that work, but C13 requires explicit failure on limit breach and C9 states `allow_partial` "only permits named source-read failures" — so an unreadable or over-budget store the user deliberately excluded either fails the query or forces the user to weaken the partial-read guarantee for the whole run. C6's only remedy is "Explicit `--source` remains the remedy", which is single-source, not filter-shaped. This is a selection/failure-semantics contradiction against ac-0001 and ac-000e, and it is cheapest to fix before `tk-0007` and `tk-0008` are dispatched, since it is a port signature.

**Minimal fix.** Either extend `load` with a typed `SourceSelection` the SDK derives from the request before I/O, or state in C6/C9 that identity filters are strictly post-load and require read failures in *unselected* stores to degrade to coverage diagnostics rather than a query failure. Whichever is chosen, name it in `tk-0007`'s interface.

### F4 (medium) — C8's global ambiguity rule contradicts C8's own context rule and the command catalogue

C8 states: "Range/context/linear text extraction requires an unambiguous selected session/branch; otherwise return AmbiguousBranch with valid local branch IDs." Two sentences later the same contract scopes context per row: "Context before/after is only for turns/messages, applied after matching over native conversation order in the same session/branch; union overlapping windows".

Those differ. `--range` genuinely needs exactly one session/branch, because C7 defines the turn ordinal as "stable one-based ordinal in a selected session/branch view". Context windows do not: each matched row expands within its own session/branch. Read literally, the first sentence rejects the catalogue's own example — `command-catalog.json:375`, `unisphere turns extract --repo . --has-tool-family shell --has-errors --context-before 1 --include-content --format jsonl` — which is repository-wide with a context window and no session selector, and which workflow 2 ("reconstruct the context of a failure") depends on. C16 requires recipes to compose these exact leaves, so the contradiction propagates into documentation proof.

**Minimal fix.** Restrict the uniqueness requirement to `--range` and linear-text extraction; state that context windows partition per matched row's session/branch; reserve `AmbiguousBranch` for a selector that resolves to multiple candidates.

### F5 (medium) — saved input validates coverage but has no completeness rule

Composition step lists "Saved input -> version/field/coverage validation -> query without live provider enrichment", and C9 fails offline operations whose required projected fields are absent. But a saved envelope is a *projection*: C19 defines it as `{…data:{…rows, coverage, matched, emitted, next_cursor}}`, so it may legitimately carry a `--columns`-reduced, `--limit`-truncated, cursor-paginated row universe. No contract states that `emitted < matched`, a present `next_cursor`, or a reduced column set constrains what may be computed from it. Two concrete consequences: C8's context union can only see rows present in the file, so offline neighbours can be silently fewer than online; and C11 statistics over a truncated universe can be reported with the same exactness language as a complete one. The envelope already carries the fields needed to detect this — nothing requires their use.

**Minimal fix.** Require the saved envelope to record applied selection/limit/column digests and a completeness flag, and require offline context, statistics and reconstruction either to refuse with a typed availability failure or to label results bounded-by-input; never exact.

### F6 (medium) — per-lane acceptance does not discriminate, and one unit holds most of the semantic risk

`tk-0002`–`tk-0006` each declare an identical acceptance set — `ac-0001, ac-0002, ac-0003, ac-0008, ac-0009, ac-000c` — so nothing states what the Cursor lane must prove that the Codex lane need not, even though C7 and C15 give those dialects very different capability floors (Cursor transcript has no native timestamps/IDs/results; VS Code journal is a reduced current revision only). Meanwhile `tk-0008` declares 15 of the 24 ACs and owns identity, reconstruction, filtering, time, ordering, continuation, branches, context, tools, duration, statistics, privacy projection and offline validation.

I agree with the `fan_out` rationale that splitting that engine "would disagree" — the single-owner decision is right. The finding is the accountability shape around it: five lanes with indistinguishable targets, and a critical path whose acceptance is a list rather than a sequence of provable increments.

**Minimal fix.** Give each parser lane dataset-specific done-conditions naming its own dialects' reconstruction, pairing and availability rows, and state which portions of ac-0008/ac-0009 each lane owns versus which the SDK owns. For `tk-0008`, split its acceptance into ordered internal milestones with the existing `bp-0008` test target, so progress is observable before the whole engine lands.

---

## Low findings

### F7 (low) — C17's registry anchor points at the pre-plan014 file

C17 cites "app main.rs:14-53 and adapters.rs:13-255 sole registry". At base `2fb5b15` that is accurate (`ADAPTERS` 27-186, `dispatch` 224-244, `pub fn run` 246, tests 255). At the integration target C15 binds — `9250b8f6` — the file is 516 lines: `ADAPTERS` 28-209, `dispatch` **284-304**, `pub fn run` 306, tests 315. The cited range therefore excludes `dispatch`, the exact function F1 requires changing. `main.rs:14-53` remains correct.

**Minimal fix.** Re-anchor to `9250b8f6` or cite symbols rather than line ranges.

### F8 (low) — CSV cannot carry the availability distinction the schema promises

C18 requires "projected absence is distinct from a present null"; C12 specifies CSV as "null empty". A CSV cell cannot distinguish absent, null and empty string, so ac-0003's typed availability is lossy on that channel. This is a declared consequence rather than a hidden one, but consumers will meet it silently.

**Minimal fix.** Name CSV as the lossy channel in the schema/docs and direct callers to JSON/JSONL where the distinction matters.

---

## Confirmations (no change requested)

- **Plan014 bindings in C15 are accurate.** I recomputed both cited hashes against committed `9250b8f6`: `crates/core/src/git_notes.rs` = `4261f117f598726da973fd6fc80ca546d65a368a7acc33d8982f7c5723c61571` and `crates/adapter-git-ai/src/lib.rs` = `57d902344041538cbaf96bec8a35348fa44b66e7b346c22825321e34e8567d60`; both also match that commit's successful `boot-attempt-2.json` source manifest. C15's characterisation — approved API, Main acceptance, landing and publication separate and not yet occurred — matches what I independently reviewed and what is now durable at `cf4931ed`.
- **`verification/guide-authoring-v2.json` is not overclaimed.** It records `new_query_implementation_executed: false` and `independent_review: "pending"`, and contains no architectural-judgement key at all; its `runs` are Builder structural and DD validation plus generated-Markdown drift checks. The guide relies on it only for document well-formedness.
- **Proof map is complete and honest.** All 24 ACs have exactly one backpressure row; every row is `unchecked`; the 17 checks are concrete crate-scoped test targets plus four `unisphere-proof` modes and `harness boot --json`, not boilerplate.
- **The planning fixture is correctly disowned as an oracle.** Its coverage is thin — no copied sessions, forks/branches, header membership, token usage or capability variation — but C16 already forbids substituting it for native fixtures: "its IDs/arithmetic are a design vocabulary, not a reconstruction oracle."
- **Ownership is genuinely disjoint.** Across all 13 units I found no path-pattern overlap and no same-wave sibling dependency; `crates/cli/src/git_notes.rs` (`tk-000c`) is correctly carved out of `tk-000b`'s file-level CLI ownership, and testkit is split by file rather than by glob.
- **Command surface is internally consistent in count.** 27 leaves and seven workflows are present in both `command-catalog.json` and `workflows-and-command-reference.md`, with matching argv examples.
- **Empty phase task files are expected here.** `assets/tasks/phase-{1,2,3}/tasks.dd.json` carry `tasks: []` and `done_when: {}`; task generation follows guide approval, so this is sequencing, not omission. It does mean the only AC-to-owner binding today is the guide's `capabilities` section.

## Answers to the packet's review questions

1. **Boundaries** — coherent and consistent with the real crates, with one exception: the CLI/app routing seam for `sessions *` is unresolved (F1). Dependency direction, single registry, pure-adapter and no-OTLP-round-trip rules are stated precisely.
2. **Contracts concrete enough** — yes for the enumerated types, no for the seven types on the adapter seam (F2). Missing invariants challenged rather than demanding baseline code, per the packet.
3. **Unit independence** — waves and ownership are real and disjoint; the SDK engine is deliberately and correctly single-owner, but acceptance does not discriminate per lane and the critical path is unstaged (F6).
4. **Dataset honesty** — internally consistent and unusually careful; identity, copies, forks, pairing, duration basis and cumulative usage are all evidence-gated. Native header membership is defined in guide C7 but absent from `query-contract.md`; the guide governs, so this is a planning-asset coherence gap rather than a design defect.
5. **Selection/privacy completeness** — complete except F3 (selection cannot reach the loader) and F5 (offline completeness), plus the declared CSV loss (F8).
6. **Actionable outcomes** — C14 is strong and explicitly rejects "a generic docs link/key-presence assertion" as proof. Its realisability depends on F1: a suggested action must be expressible in the real grammar, which is exactly what the routing collision puts in doubt. The catalogue and workflow reference also supply next steps unevenly across leaves; C14 is the governing contract and should be the acceptance source, not the prose.
7. **AC accountability** — every AC has an owner, a capability and named checks. Checks are planned BUILD/EXTEND, and the existing passing boot proves only native projections, exactly as C17 states.

## Limitations

I read documents and committed bytes; I executed nothing beyond read-only hashing, `git show`, and the canary acknowledgement. No proposed behaviour was run, and no finding here asserts that any code exists. Where a scout gathered inventory for me, I re-verified every claim I cite — including correcting one scout's description of `guide-authoring-v2.json`, which has no architectural-judgement field rather than one set to "not-performed". Proposed fixes are proposals; none is implemented.
