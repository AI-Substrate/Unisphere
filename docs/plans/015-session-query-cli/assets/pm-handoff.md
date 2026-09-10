# CLI product plan — handoff for the operator-nominated PM

## Status and authority

**Resume here:** `compaction-handoff.md` is the current self-handover. Jordan requested a compaction pause. The last proved first-wave source is `5ee631b46142b88dea2a2742c9cb8087642c4d25` (327 tests/full boot); newer PM P4/P5/P6 edits are unformatted and unrun WIP. The received independent review is `reviews/first-wave-pij-armed-cow.json`, changes-requested, with P1 exact-source external proof still gating acceptance.

Tiger remains the sole PM. Jordan explicitly authorized the product-only manual CLI helper with “ye”. Peer `pij-unfortunate-rat` is held in `/Users/jordanknight/substrate/unisphere/unisphere-query-coders-015/tk-000b-manual` at checkpoint `f4c8b3570a851f9008183fff3f045d8f30a6307b`, branch `work/015-manual-cli`; it reports a clean worktree and no pending jobs. This is not accepted CLI delivery or a Builder dispatch. Preserve/reuse the same seat, clone, branch and packet; no replay or reallocation after compaction. The Builder receipt cycle remains unresolved; no automatic reconciliation, global repair, push or merge is authorized.

Subsequent ownership reconciliation confirmed that Tiger remains the sole canonical plan015 writer. Mammal owns a distinct private future-state Distro synthesis that assumes this CLI exists and interviews its owner; this is not a plan015 succession or competing implementation plan. Reptile is Distro's designated eventual coder, with no product allocation yet. The agreed split is recorded in `pm-appointment.json`; the temporary planning-write freeze is resolved.

Builder allocated plan015 in the `unisphere-session-query-cli` sibling worktree, branch `builder/015-session-query-cli`, from main `1469998cd750d23d2420feb83cf8e93a0841b347`. Allocation `al-015-5470067d-b5e9-4de3-aa42-809f81d5f5f0` is owned by harness, with authority in the main Git directory outside the worktree. Keep this plan separate from plan014 Git-ai source implementation and plan016 Dependabot configuration.

The teaching guide has been replaced by the real `assets/impl-guide.dd.json`: 13 owned units, 10 coder lanes and a complete 24-AC proof map. Jordan subsequently selected OMP `github-copilot/gpt-5.6-sol-fast` high coders and OMP `github-copilot/claude-opus-5` high reviewers; `team/operator-role-amendment.json` records the role-only change from the approved guide basis. `baseline-followups.json` carries the mandatory typed cursor-reason refinement before sealing. Installed Builder dispatch is map-first/warning-first, not the stale acknowledgment lifecycle, and rejects linked-worktree coder allocations; use explicit coder clones with observed runtime bindings, never a claimed shell-cd rebind.

## What we are building and why

A developer should be able to ask: what happened in this repository, where is the conversation I remember, which requests led to failures, and what are the actual timings of the shell calls? They should not need to know seven storage formats, write a one-off parser or accept fabricated completeness.

The core product is a coherent query/extraction capability in the Rust SDK, surfaced by a thin CLI. Commands are views over shared source/identity/time/privacy semantics—not independent scripts with similar names. A user moves through learn → discover → select → inspect → extract → measure. Results retain provenance and distinguish unknown/partial evidence.

**This requires a substantial SDK upgrade.** Discovery, identity/lineage, dataset construction, turn/call reconstruction, filtering, ordering/view freshness, context selection, privacy-aware projection and statistics are SDK capabilities with public injected Rust APIs. CLI argument parsing, terminal presentation and bundled CLI docs sit outside that semantic core. Extraction serializers can remain reusable output adapters; the CLI must not become the only place where a query or privacy rule is implemented.

In the delivery breakdown below, the discovery through statistics lanes are primarily SDK work. Extraction owns SDK selection/projection plus reusable output adapters. The docs lane includes executable external SDK examples. Composition proves SDK consumers and CLI handlers observe the same result, error and coverage semantics. Existing collection APIs are inputs to this upgrade, not assumed to already provide it.

The PM owns code quality and acceptance. A coder delivery is a candidate: inspect correctness, maintainability, dependency direction, allocations/copies and source-evidence honesty; reject or rewrite weak code instead of forwarding it. Compilation or a coder's completion claim is not acceptance. Require the real composed SDK/CLI/native/offline-docs/error/action scenarios and independent review; tests must defend observable behavior, not implementation snapshots or mock echoes. Model speed does not lower that bar.

## Read in this order

1. `../plan.dd.json`: outcome-level scope, ACs, phases and non-goals.
2. `workflows-and-command-reference.md`: seven motivating workflows and all27 command leaves, each with question, purpose, example, synthetic result, interpretation and failure/limit.
3. `query-contract.md`: shared identity, discovery, filtering, time, turn/call reconstruction, output, privacy, aggregation and resource semantics.
4. `documentation-design.md`: Flowspace3-backed first-class offline docs design and proof obligations.
5. `command-catalog.json` + `synthetic-query-fixture.json`: machine-readable design examples. These are NOT claims that the current executable implements those commands.
6. Existing source/SDK/CLI/telemetry/fidelity docs and the actual completed native adapters. Reuse their contracts; do not introduce a second collector or application registry.

## Preserve the important boundaries

- No retired harness telemetry (`refs/harness-telemetry`, old segments/rollups, bridges or migration).
- Plan014 implements Git-ai-format notes independently of Git AI installation/code/libraries. Coordinate only its final registered source/API contract. Attribution can enrich provenance and known session identity; it cannot fabricate messages, turns or tool timings. Other CLI lanes must proceed using fake providers without waiting for that source's implementation.
- Existing native session export remains OTLP LogsData. Query JSON/JSONL is a versioned user-facing view format; do not label it OTLP.
- Metadata is not anonymity. Never infer privacy from a source harness's capture setting; empirically test what Unisphere emits in every mode.
- No mandatory daemon, hosted store or model service. A persistent index is not required; measure before proposing one. If later justified it is rebuildable and separate from source truth/user state.
- Do not guess missing duration, cost, turn boundaries, session identity, causality or source finality.

## Shared contract decisions before fan-out

The guide must settle exact Rust public types/methods, error vocabulary, schema/ID/version grammar, field availability, supported source capability matrix, filter evaluation, query-view freshness, ordering/pagination, privacy-safe projections and aggregation units. It must name hard resource bounds and how each refusal is exercised.

Two cross-command choices are settled in the product contract: CSV defaults to spreadsheet-safe string escaping with an explicit raw opt-in, and multiple output-mode selectors (`--json`, `--human`, `--format`) are rejected rather than given hidden precedence. Preserve these rules and their examples in the reviewed guide; do not let individual handlers redefine them.

Choose the minimum inward contracts needed for real independence: injected source/view provider, typed query request/row model, source/provenance identity, availability/error types, output/projection policy and an executable synthetic fixture vocabulary. Keep mutable shared files (core exports, Cargo workspace, app registry, parser routing and docs topic registry) under named single owners. Interface changes return to that owner instead of concurrent same-file edits.

## Candidate independent lanes — not yet allocated

| Lane | Owned outcome | Shared contract it consumes | Observable proof |
|---|---|---|---|
| Discovery | Registered local source enumeration and repository associations; sources list/check | Source descriptor, association evidence, bounds/error vocabulary | Exact/subtree/worktree matches, sibling-prefix rejection, absent/unreadable/unassociated distinctions |
| Identity and lineage | Stable entity/source identities, proven-copy reconciliation, fork/subagent relationships | Source/native identity and relation records | Duplicate representations do not inflate counts; parent-ID reuse does not collapse a child; conflicts/cycles are explicit |
| Filter and ordering | Typed selectors, text/time semantics, sort/projection and continuation freshness | Query request, field schema and source-view identity | AND/OR, date boundaries, null behavior, regex limits, tie-breaks and stale cursor refusals |
| Sessions | Session listing/detail/tree and metadata/availability views | Session/view provider and lineage interfaces | Metadata-only sessions remain visible; source counts differ honestly from logical sessions |
| Turns | Source-qualified turn reconstruction, stable ordinals and turn views | Native event/relationship vocabulary and reconstruction policy | Tool results/compaction do not become extra user turns; unresolved fragments remain events |
| Messages and events | Role/content selection and exact source-event inspection | Typed row/provider interfaces and shared filter policy | Event-time windows, exact-ID lookup, unsupported fields and no hidden content emission |
| Tool invocations | Native start/result/progress pairing, families, outcomes and duration evidence | Scoped call identity, timestamp/basis and event vocabulary | Interleaving, retries, missing/duplicate completions, clock reversal and unknown outcomes |
| Statistics | Reductions over the same logical matched datasets | Row schemas, availability, dedup and units | Null-aware mean/nearest-rank percentiles, named denominators, cumulative/replayed usage not double-counted |
| Extraction and rendering | Match/context windows, JSON/JSONL/CSV/text/Markdown and create-new output | Typed rows, content/projection/output policy | Context labels, merged windows, privacy sentinels, encoding and partial-output failures |
| Offline docs and examples | Bundled docs list/get, topic registry, SDK/CLI workflow examples and drift guards | Parser/schema contracts and synthetic cases | Installed artifact works offline; nested commands/options and real workflow output are checked |
| Composition and consumer proof | CLI/app wiring, external SDK, source capability matrix and full workflows | All frozen contracts, using fakes before concrete deliveries | Installed CLI/SDK agree; seven user workflows and meaningful hostile/partial cases run end-to-end |

This supports roughly ten or more workers if the actual guide demonstrates independent ownership. It is not a quota. Merge tiny lanes, split a genuinely large one, and keep composition/shared-contract mutation serialized. The PM remains responsible for the common experience and adjudicating contracts; do not outsource the top-level design to disconnected coders.

## Suggested waves

1. **PM-owned contract baseline:** settle schemas/semantics and the small synthetic fixture/provider contract; obtain independent guide review; implement and prove the minimal shared baseline. This is the true dependency, not completion of every handler.
2. **Independent implementations:** lanes consume frozen interfaces/fakes. Docs, renderers, filters and statistics do not wait for live native handlers if their input contracts are agreed. No worker runs project-wide validation while others edit.
3. **Composition:** named owner integrates in dependency order, migrates all affected callers, removes obsolete paths, and runs the full relevant quality/behavior lanes once on the committed candidate.
4. **Independent review and operator proof:** verify actual workflows, installed offline docs and SDK examples, privacy and failure paths. Only then plan closeout/ship under explicit current authority.

## Proof and handoff requirements

Every lane returns its exact commit, changed files, public symbols, actual commands/cwd/exits, covered ACs, source capability limits and documentation examples. No stubs, mocked success, source-text-only privacy assertions or new tests merely for appearance. Keep permanent tests where plausible edge failures earn them; use throwaway real-program smoke for ordinary feature proof.

The PM's final demo must show one fixture passing across commands: discovery agrees with session identity; turn/message/tool filters select the intended records; context extraction labels additions; tool statistics match exported rows including missing durations; SDK and CLI agree; docs examples execute from the installed artifact. Redaction/default omission must be tested across every output/error channel.

Record product progress through the existing DD/Builder records, not observation buffers. Capture actual engineering friction separately. Use `harness commit` for owned commits. Preserve main, governance, other plans, real user stores and global/deployed tooling. Public disclosure/remediation and repository visibility changes require their own decision; this plan contains only synthetic examples and public design references.

## First PM response expected

Confirm workspace/branch, current runtime/isolation capability and ownership, then listen to Jordan's additional preamble in your own session. When clarification is needed, ask exactly **one question per turn: one sentence of context, then one sentence for the ask**. No question batches, modal questionnaires or parent proxy. Do not start product edits or fan-out merely because the candidate table exists; first settle the user's direction and the actual reviewed guide/proof topology.
