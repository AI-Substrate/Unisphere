# plan014 Git-ai ingestion guide — independent review

Reviewer: `pij-armed-cow` (OMP; exposed model id `github-copilot/claude-opus-5`; reasoning effort exposed as `high`; no provider identity claimed beyond that string).
Root reviewed: `/Users/jordanknight/substrate/unisphere/unisphere-git-ai-notes`, branch `builder/014-git-ai-notes`, HEAD `2fb5b15349873cd62ccd75c94fdfedae40d561f5`.
Native root confirmed via `git worktree list --porcelain`: `refs/heads/main` at `/Users/jordanknight/substrate/unisphere/unishpere-main`.

Digests recomputed before reading, both match the packet:
- `docs/plans/014-git-ai-notes/plan.dd.json` — `e832077b4c7a3c27ea5d3f444a13862c1984170193626dfc7b4ae3f20676ced8`
- `docs/plans/014-git-ai-notes/assets/impl-guide.dd.json` — `ccf665e85aafe43a4cd201f2a10297a81b548a0b8a6da147d40d29b5ae72aea7`

Read-only review. No product/plan/guide/task mutation, no formatter/lint/build/test run, no commit, no baseline proof rerun, no Builder review receipt. Sources read: plan, guide, brief, backpressure, phase-1 tasks, reconnaissance; existing product source under `crates/`; `.harness/extensions/{checks,boot}`; `docs/telemetry-profile.md`; and research-only git-ai spec/source under `/Users/jordanknight/github/git-ai`.

## Verdict

**CHANGES REQUESTED.** The architecture direction is sound — SDK-first hexagonal composition, pure adapter, no git-ai dependency, no fabricated timing/statistics, explicit limits with failure instead of truncation — and it is materially stricter than git-ai's own reader in the right place (git-ai fabricates `total_additions: 0` etc. when projecting session records into a legacy `PromptRecord`; this guide forbids that). Two findings block product edits because the contract as written cannot be implemented or proven against the existing composition; four more are material.

Known pending refinements in the packet (concrete `GitObjectLoader` name; output exclusion covering the canonical worktree root and common Git dir) were excluded from findings.

### R1 adjudication — corrections to this record

Main's R1 adjudication (`/tmp/unisphere-worker-cleanup-3kSogq/git-ai-guide-adjudication.json`) is accepted on three points, verified against primary sources before amending:

1. **F5a withdrawn as invalid.** `specs/git_ai_standard_v3.0.0.md:193` — "Implementations SHOULD accept 7-character hashes for backward compatibility with versions prior to v1.0" — read directly and confirmed. The guide's `16/7-hex` is spec-conformant, and my implication of a private-cache dependency was unfounded. See F5.
2. **F2 narrowed.** Additive optional flags do not by themselves breach `ac-0002`; the routing/registry/catalog gaps remain material. See F2 item 1.
3. **Output sizing claim withdrawn.** A 16 MiB input ceiling need not fit a 32 MiB encoded batch; the guarantee is explicit `OutputLimit`. See the final "checked and sound" entry.

F1, F3, F4, F6 and F7 stand as written. The verdict below is otherwise unchanged.

---

## F1 — BLOCKER: the composed proof cannot resolve `git`; nothing injects the Git executable

Guide contract: "Concrete `GitNoteLoader::new(PathBuf)` receives explicit absolute Git executable; **app resolves standard git from PATH**, no Git AI probing."

Every product execution inside the proof binary runs through `run_product` → `sealed_command` (`crates/testkit/src/bin/unisphere-proof.rs:85`), and `sealed_command` sets `PATH` to the empty string (`crates/testkit/src/sealed.rs:26`) after `env_clear()`. Under that harness a `PATH` lookup for `git` cannot succeed, so the ordinary success path of vd-0004 (`ac-0002`, `ac-000a`, `bp-0002`/`bp-000a`) is unreachable as specified. The CLI contract in the guide defines no `--git-executable`, no env input, and no other injection seam; `CliContext` (`crates/cli/src/lib.rs:28-35`) carries only `cwd`, `stdout_is_terminal`, `version`.

The sealed harness would happily prove the *negative* cases (missing Git, Git AI absent) and silently make the positive case impossible — the worst failure mode, because "Git AI unavailable" proof would pass while ordinary ingestion was never exercised.

Smallest fix: name the explicit injection seam in the guide — an absolute-path CLI input (e.g. `--git-executable ABSOLUTE_PATH`) or a single named environment variable the app validates as absolute — and state that the git-notes proof mode passes the real Git path into the sealed child, keeping unresolvable/missing Git as its own asserted failure case.

## F2 — BLOCKER (narrowed after R1 adjudication): routing, registry representation and catalog vocabulary are unspecified

Guide contract: "CLI uses existing command family: `unisphere sessions list --adapter git-ai --repo PATH [--notes-ref …] [--commit FULL_OID …]`; `sessions export` same selection…".

Against the actual frontend and composition root:

1. `SessionCommand::List` accepts only `--root` and `--max-sessions` (`crates/cli/src/sessions.rs:22-28`); `Export` requires `--input` and has no `--repo`. `requested_session_adapter` would select the git-ai registration, then `Arguments::try_parse_from` rejects `--adapter`/`--repo`/`--notes-ref`/`--commit` on `list` and returns `InvalidInput` exit 2. **Corrected:** my R1 text argued that widening the shared `List`/`Export` enums therefore violates `ac-0002`. That inference was wrong and is withdrawn — optional additive flags leave every existing invocation working, so `ac-0002` is not breached by addition alone. What remains material is only that the guide states a command shape the current frontend rejects without naming the parser change that makes it real, and does not say how `--repo`/`--notes-ref`/`--commit` behave when a non-git adapter is selected.
2. `run_sessions` takes `&dyn CollectionApi` (`crates/cli/src/sessions.rs:100-105`). A `GitNotesApi` collector needs a third frontend entry point beside `run_sessions`/`run_snapshot_sessions`; the guide never names it.
3. `AdapterRegistration.run` is `fn(SourceRepresentation, …)` and `SourceRepresentation` has exactly three variants, none repository-shaped (`crates/app/src/adapters.rs:12-25`). `ADAPTERS` is `[AdapterRegistration; 9]` (line 27).
4. `AdapterRegistration` carries `#[cfg(test)] fixture: &'static [u8]` (line 24) and `production_catalog_ids_match_exported_provenance` (line 345) iterates **every** registration, writes `registration.fixture` to a file and runs `sessions export --adapter <id> --input <file> --include-content` expecting exit 0 (lines 352-403). A repository-backed source cannot satisfy that as a byte-blob `--input` fixture.
5. `crates/app/tests/adapter_catalog.rs:33-46` pins the exact nine-id `BTreeSet`, and constrains every published location hint to `base ∈ {home, appdata}` and `storage_format ∈ {jsonl, json_document, json_journal, sqlite_key_value}` (lines 52-60). The guide states nothing about the git-ai descriptor's `locations`, `capabilities` (`sdk_caller_owned_cursor`, `cursor_source_assumption`, `cli_persisted_resume`, `lossless_archive`) or storage-format vocabulary, although `adapters list --json` is a published contract.

Smallest fix: state in the guide (a) the frontend entry point and parser branch that serve the git-notes port, including the declared behaviour of the new flags under other adapters, (b) how the registry represents a repository-scoped source — new `SourceRepresentation` variant plus the explicit rule for the `fixture`/export-provenance regression, and (c) the exact descriptor `locations`/`capabilities` values, including empty locations if that is the decision.

## F3 — MATERIAL: the output contract omits the cross-profile required attributes and the closing-manifest field set

`docs/telemetry-profile.md:99-100`: "All formats retain `unisphere.profile.version` as the **integer** `1`, `unisphere.source.adapter`, `unisphere.source.path` and `unisphere.source.kind`." That set is enforced for every registered adapter by `production_catalog_ids_match_exported_provenance` (`crates/app/src/adapters.rs:411-418` asserts `unisphere.source.adapter == descriptor.id` on every emitted record).

The guide's projection contract names only `unisphere.git_ai.note` / `.identity` / `.attribution`, provenance repo/ref/tip/commit/blob, and a closing `unisphere.git_notes.snapshot`. It never states the four required cross-profile keys, and `unisphere.source.path`/`.kind` are genuinely ambiguous for a repository-scoped source (repository path? notes ref? note blob?). The closing event is likewise undefined field-by-field, whereas the existing `unisphere.session.snapshot` manifest has a documented extension table (`semantics`, `records`, `selection`, `include_content`, `finality`) at `docs/telemetry-profile.md:114-126`.

Smallest fix: add the four required keys with their git-notes meanings to the contract, and enumerate the `unisphere.git_notes.snapshot` fields (including the honest `finality`/`semantics` analogues) before implementation, so `ac-0007`/`ac-0008` honesty claims are checkable.

## F4 — MATERIAL: hard-failing unresolved attestation keys rejects ordinary, valid notes

Guide contract: "Malformed shapes, **unresolved attestation keys** and unsupported versions/variants fail explicitly."

Unresolved keys are a normal, expected condition in this format, not corruption: git-ai's own reader skips them and says why — `// h_ hash not found locally (foreign cherry-pick) — skip this entry` (`/Users/jordanknight/github/git-ai/src/authorship/authorship_log_serialization.rs:250`) and `// Session hash not found locally — skip this entry` (line 280); legacy keys fall back to searching other notes (lines 444-460). Cherry-pick, rebase and partially-fetched note histories produce attestation keys whose metadata entry lives in another note.

Failing the whole note discards the attribution evidence the feature exists to surface, and no acceptance criterion requires it: `ac-0003` requires that missing data is not synthesized and unsupported variants are explicit; `ac-0006` requires distinguishability, not failure.

Smallest fix: emit the attribution record with the native key retained and the identity explicitly unresolved (no invented identity, no fabricated agent), reserving hard failure for structurally malformed notes. Apply the same explicit-record-vs-fail decision to the guide's "validate … target commit type" rule, which currently reads as a whole-listing failure when a notes ref carries a non-commit-targeted note.

## F5 — MATERIAL (reduced after R1 adjudication): two format claims in the parser contract are imprecise; the 7-hex finding is WITHDRAWN

Guide contract: "…exact standalone divider, **quoted file paths**, positive sorted inclusive ranges, **legacy 16/7-hex prompt keys**, `s_<14hex>::t_<14hex>` session keys, `h_<14hex>` human keys."

- ~~**`7-hex` legacy keys do not exist in this note format.**~~ **WITHDRAWN — I was wrong.** `specs/git_ai_standard_v3.0.0.md:193` states "Implementations SHOULD accept 7-character hashes for backward compatibility with versions prior to v1.0", immediately after the `MUST be 16 characters in length` bullet I cited. I read lines 189-191 and stopped short of 193, then reached for `src/git/repo_storage.rs:553-560` as the origin of a rule that is in fact spec-normative. `16/7-hex` in the guide is correct and spec-conformant, it is not evidence of private-cache dependency, and the plan's no-private-cache non-goal is unaffected. The migration code I cited is corroboration of the same pre-v1.0 legacy era, not its source. Recommended addition to the guide only if the coder wants it: cite the `SHOULD` at spec:193 next to `16/7-hex` so the intent is legible to the next reviewer.
- **Quoting is conditional, not universal.** Paths are quoted only when they contain spaces, tabs or newlines (`specs/…:68`); git-ai's parser treats an unquoted line as the normal case (`authorship_log_serialization.rs:441-447`). All notes in the reconnaissance set use unquoted paths. "Quoted file paths" must read "optionally quoted".
- **Bare single line numbers are a legal range form** (`specs/…:98`, `| Single line | A single line number | 42 |`), as seen in real data (`  <16hex> 36,246,251`). "Inclusive ranges" must explicitly cover single-line entries so a valid note is not rejected.

Smallest fix: correct the quoting and single-line-range phrases in the contract before code; they are the parser's acceptance boundary. No change is requested for legacy key lengths.

## F6 — MATERIAL: readiness ownership gap — the boot proof list and its regression are not in any unit's owned paths

`.harness/extensions/boot/boot.mjs:26` hardcodes the proof mode list `['composition','sdk-consumer','installed-cli','collection','native']` and line 40 publishes `scope: 'configuration-and-native-session-projections'`. The harness regression asserts both — `proofs.at(-1)` is the `native` mode (`extension.test.mjs:49-56`) and the exact scope string (line 62) — and `harness checks` runs that regression plus `cargo test --workspace --all-targets --locked` and the architecture sensor (`.harness/extensions/checks/checks.mjs:80-86`).

Consequences: a new `git-notes` proof mode is not part of `harness boot` readiness unless `boot.mjs` changes; the published boot scope/limitations become dishonest once Git-notes ingestion ships behind an unchanged `configuration-and-native-session-projections` label; and changing them without updating `extension.test.mjs` fails the `harness-regression` gate. tk-0004's owned paths list `.harness/records/**` only. The repository precedent for a composition unit is broader — plan 010's tk-0009 owned `.harness/**`.

Related and already partly covered: the architecture sensor's package allowlist and `allowed()` edge table (`crates/testkit/src/bin/unisphere-arch-check.rs:20-78`, 104-120) must gain `unisphere-loader-git` and `unisphere-adapter-git-ai`, or the sensor fails with "unapproved workspace package". That file is inside the owned `crates/testkit/**` and the composition step already says "extend architecture sensor", so only the `.harness/extensions/**` ownership is missing. Note the sensor also scans every `unisphere-adapter-*` production tree for `std::fs|env|process|net|thread`, clocks and `include_bytes!` (lines 168-200, 289-305) — the pure `unisphere-adapter-git-ai` gets that enforcement for free, which is a genuine strength of the chosen crate name.

Smallest fix: add `.harness/extensions/**` to tk-0004's owned paths and state the boot scope/limitation strings the composed change must publish.

## F7 — NON-BLOCKING: explicit `--commit` selection still pays for whole-tree enumeration

The guide enumerates `ls-tree -r -z` over the pinned tip for all selections, bounded by `max_listing_bytes` 1 MiB with "oversized … listing … fail, never successful truncation". Measured on this repository's real notes ref: 81 notes produce 7,695 bytes of `ls-tree -r -z` output (~95 bytes/entry), so roughly 11,000 noted commits makes even a single explicit `--commit` read fail with a listing limit — the cheapest possible request failing on the largest repositories.

Smallest fix: for `GitNoteSelection::Commits`, resolve each note's fanout path directly at the pinned tip instead of enumerating the whole tree; keep full enumeration for `All`.

Also worth one line in the environment contract: `GIT_CONFIG_NOSYSTEM=1` plus `GIT_CONFIG_GLOBAL=/dev/null` discards the operator's `safe.directory` entries, so a legitimately foreign-owned repository will fail with dubious-ownership instead of a distinct, actionable error. Either add `-c safe.directory=<explicit repository>` for the selected repository only, or name that failure explicitly under `ac-0006` distinguishability. Related, lower priority: the repository's own sealed helper re-adds `SystemRoot` on Windows (`crates/testkit/src/sealed.rs:35-45`) because bare `env_clear()` breaks process startup there, and `/dev/null` is not a valid Windows path — state that the loader is Unix-scoped, consistent with `export_platforms: ["unix"]`, or handle both.

---

## Checked and found sound (no change requested)

- Git AI absence: no crate/library/executable/daemon/HTTP/private-cache dependency anywhere in the contract; standard read-only Git only. The pure-adapter crate name additionally inherits the existing source-purity sensor.
- No fabricated statistics or timing: `TelemetryRecord.timestamp_unix_nano` is `Option<u64>` and the writer omits `timeUnixNano` when absent (`crates/output-otlp/src/lib.rs:84-87`); the guide's "missing statistics never default to zero" is a deliberate, correct divergence from git-ai's own zero-filling shim (`authorship_log_serialization.rs:262-268`).
- Metadata/content boundary matches the real record shapes: `human_author`, `author`, `custom_attributes`, `messages`, `messages_url` are exactly the fields carrying identity/transcript data (`specs/…:229-270`; reconnaissance record keys), and inert-URL/no-dereference is correct.
- `refs/notes/ai` default and "no tracking-ref discovery/aggregation" are right: the canonical namespace is mandated (`specs/…:23-24`) and the fork tracking ref is separate (`src/git/refs.rs:14`), so `ac-0008` non-duplication holds by construction under explicit ref selection.
- `repository_id` as the canonical Git common directory resolves bare repositories and linked worktrees correctly and avoids inventing a global identity.
- Pinning the ref tip once and validating `read_note` membership against that pinned tree defeats the moving-ref race; `--no-optional-locks`, `protocol.allow=never`, disabled hooks/fsmonitor, no textconv/filters and promisor refusal are the right control set for a read-only Git surface.
- One bounded `write_batch` per selection: over-budget encoding fails explicitly as `OutputLimit` before the destination is touched (`crates/output-otlp/src/lib.rs:34-47`, `MAX_OUTPUT_BATCH_BYTES` at `crates/core/src/collection.rs:9`), never a silent truncation, which is the property `ac-0006` needs. **Corrected:** my R1 text additionally asserted that the stated 10,000-record / 16 MiB input ceiling fits inside 32 MiB encoded. That was an unnecessary sizing inference beyond the evidence and is withdrawn; the guarantee is the explicit failure, and no fit relationship between input and encoded-output budgets is claimed or required.
- Metadata prompts with no attestations (6 declared, 2 attested in the observed legacy blob) are preserved by the "identity per declared prompt/session/human" projection rather than dropped.

---

# R2 — focused delta re-review: APPROVE

Frozen checkpoint re-verified before reading. All four digests recomputed in the worktree and matching `assets/reviews/guide-r1-dispositions.json`:
`plan.dd.json` `e832077b…6ced8` (unchanged), `assets/impl-guide.dd.json` `0663554ff3439b83cd5992621e26b778928c71087941971b736f7d5a47303378` (v3), `assets/backpressure.dd.json` `d47def32…f9d2`, `assets/tasks/phase-1/tasks.dd.json` `fe69cc00…10fa`. Dispositions file itself: `9113c0e14cc41839a7f409c010cec15baa66f7f05780d2ddfa2b004c41062a80`. `implementation_started: false` is consistent with the tree: no product source changed, only plan artifacts.

Scope honoured: delta only, plus what the delta could invalidate in F1/F3/F4/F6. No product edits, no reruns of `checks`/`boot`/tests, no formal Builder receipt, no self-approval.

## Findings dispositions verified

- **F1 — closed.** `--git-executable ABSOLUTE_PATH` with relative values rejected; `run_git_notes<C: GitNotesApi>(args, context, make_collector: impl FnOnce(Option<PathBuf>) -> Result<C, GitNotesError>, …)` parses before constructing, so help and invalid syntax need neither Git nor a backend; the sealed positive proof passes the real absolute Git path while `PATH` stays empty, and missing-path vs empty-PATH are separate negatives. The closure is a statically typed seam, not a locator, and the app is named as the only executable/environment resolution owner — which keeps `std::env` out of the purity-scanned crates. This is a better answer than the env-var alternative I offered.
- **F2 — closed.** `SourceRepresentation::GitNotes` plus one registry row and an unreachable arm in `run_with_snapshot` (needed for exhaustiveness); `run_sessions`/`run_snapshot_sessions` and their parser shapes untouched. Descriptor pins `locations: []` with the stated rationale, so `adapter_catalog.rs:52-60`'s `base`/`storage_format` vocabulary is not widened and its `delayed_revision_reconciliation == false` invariant still holds. The fixture branch keeps the existing byte-fixture field as the synthetic note payload and drives `--repo` + explicit `--git-executable` instead of `--input`, which is exactly what `production_catalog_ids_match_exported_provenance` needs. Same-named limit flags across adapter-specific parsers are pre-existing house style, not a new wrinkle: `crates/cli/src/snapshots.rs:45-49` already defines its own `sessions export` flag set with different defaults from `sessions.rs`.
- **F3 — closed, and it now falls out cleanly.** Every record including the manifest carries `unisphere.profile.version` integer 1, `source.adapter=git-ai`, `source.path`=canonical selected repository, `source.format=git_notes`, `source.kind ∈ {note_metadata, declared_identity, line_attribution, notes_manifest}`, with `source.key`/`source.revision` defined for both note-derived records and the manifest. That satisfies the existing non-JSONL branch of `crates/app/src/adapters.rs:420-429` (`key` + `revision` + `format`, no `offset`) without touching the test's structure. Structural `source.key` locators (`$note`, `metadata/<map>/<key>`, `attestations/<file>/<entry>/<range>`) are honest non-byte-offset addresses. The manifest field set is now enumerated and scoped to repository/ref/selection rather than session history.
- **F4 — closed.** Syntactically valid unresolved keys emit the native key, prefix kind and range with `identity_resolution=unresolved` and omitted identity values; no skipping, no cross-note or cache lookup, no synthesis. Declared identities without attestations still emit. `UnsupportedTarget` for a selected present non-commit object and `ObjectRead` for a missing one are typed and distinguishable, and aborting rather than silently dropping under `All` is the defensible direction given `ac-0001`'s commit scope.
- **F5a — correctly rejected; my error, already withdrawn above.** v3 retains 16-hex and documented 7-hex keys verbatim with the `spec section1.2.3.1 line193` citation attached, which is precisely the legibility improvement worth having.
- **F5b/F5c — closed.** "normal unquoted and optionally quoted paths, positive single-line numbers (42), inclusive ranges (1-4) and comma-separated combinations sorted within each attestation" matches the specification at `specs/git_ai_standard_v3.0.0.md:66-68` and `:96-105`.
- **F6 — closed, and stronger than my proposal.** tk-0004 now owns `.harness/extensions/boot/{boot.mjs,extension.test.mjs,instructions.md,extension.ts}`, `.harness/extensions/checks/checks.test.mjs`, `.harness/engineering-harness.md` and `docs/development.md` — all seven paths verified to exist, and deliberately narrow (`checks.mjs` is correctly excluded, since the workspace gates already cover new crates). New `vd-0005` (`harness boot --json`, 3600000 ms) closes the gap where no listed check owned readiness. Best part: the readiness contract is "Git Notes failure prevents `ready=true` with child evidence retained", with the instruction to **remove** the wording-only scope assertion rather than repin it — that is the correct treatment of a test that pins a string instead of a behaviour.
- **F7 — closed.** Selected lookups use pinned nonrecursive literal-pathspec fanout queries bounded to OID-length/2 levels with no sibling enumeration, so an explicit `--commit` no longer pays for the whole tree; `All` keeps its own bound. On `safe.directory` the guide took the more conservative option than I suggested — refuse with a typed `UnsafeRepository` and actionable guidance rather than broadening trust with `-c safe.directory` — which is the right call for a tool reading someone else's repository. Unix-only is declared with `UnsupportedPlatform` and matches `export_platforms: [unix]`.
- **Known refinements — closed.** `GitObjectLoader` is distinct from the core `GitNoteLoader` trait; output exclusion now covers canonical worktree top-level, per-worktree Git directory and common Git directory, including nested `--repo` inputs and symlinked parent aliases, with the caller-owned destination-parent race documented rather than claimed away.
- **Output-expansion clarification — accepted.** The limits contract now states input size does not imply encoded size and defers to the writer's independent 32 MiB ceiling.

Also improved beyond the findings: `assets/backpressure.dd.json` replaced ten identical boilerplate proof strings with per-row proof describing what each row actually exercises, and the `roles` note records the R1 outcome without claiming approval.

## Non-blocking observations (no change requested, no re-review needed)

1. `docs/telemetry-profile.md:103-112` enumerates `unisphere.source.format` as `json_document | json_journal | sqlite_key_value`. Emitting `git_notes` makes that table incomplete until the doc is updated. The file is already owned by tk-0004 and "honest documentation" is already in its responsibility, so this is a reminder, not a gap in the guide.
2. `AGENTS.md:36` still describes boot as passing `configuration-and-claude-jsonl`, which is stale against today's `configuration-and-native-session-projections` and will be stale again against `configuration-native-sessions-and-git-notes`. The drift predates this plan and no acceptance criterion covers it; `AGENTS.md` is not in tk-0004's owned paths. Worth one line at closeout if the owner wants it, or leave to the prime.

## Verdict

**APPROVE** for product implementation to begin against guide v3 (`0663554f…3378`) and plan `e832077b…6ced8`. Every R1 finding is either addressed in the contract text or correctly rejected on primary-source evidence, and the two blockers are resolved by mechanisms I verified against the actual composition rather than by wording changes. No approval of implemented behaviour is implied: this approves the guide only, and the exact committed source, receipts and proof output still require the separate implementation review named in the guide's `review` section.
