# Plan009 adapter catalog — independent composition review (r1)

- Scope: composition. Subject `17829db8a427d1a6e69681044d58d7cb7c837263`, "feat(cli): expose registered adapter metadata without source discovery", parent `09c0605f` — the exact commit approved in `design-opus-r2.md`.
- Plan `docs/plans/009-adapter-catalog/plan.dd.json` sha256 `b5cfc5fb5d3d590589607010f963b522acd34b2c77154a08aabbebc0d24d68d8`.
- Guide `docs/plans/009-adapter-catalog/assets/impl-guide.dd.json` sha256 `a77126067be182fc04fb5a4fa2a1c0f35b43b0bbf43776dd2cf27f15f64fa790`.
- Composition receipt `assets/team/composition.dd.json` sha256 `f2b7f95f7c3acdbd02e89363c6a2e4ccc9488fb78b040774065aaa42b842245f`.
- Reviewer: `pij-huge-nigel`, omp, github-copilot/claude-opus-5, effort high.
- Verdict: **approved**. All six ACs are met by the committed candidate, the five R2 obligations D1–D5 and E1 are all discharged in code and docs, and no dependency, architecture policy or frozen contract moved. Two low, accepted proof-durability findings; nothing blocks the ship.

I read committed bytes at the subject rather than the working tree, recomputed every pinned digest, and read the implementation myself before reading the PM's evidence. I ran no build, test, linter or formatter, did not re-audit unchanged Plan005 code, and wrote only this report and its receipt.

## What the candidate actually is

Nine source and doc files, no dependency change: `git diff --stat 09c0605f..17829db8 -- '*Cargo.toml' Cargo.lock crates/testkit` is empty, so no crate gained a dependency and the architecture allowlist was not loosened to admit this feature. The frozen baseline holds — `crates/core/src/collection.rs` and `crates/core/src/ports.rs` hash exactly to their sealed values, and the baseline receipt itself matches the digest the composition record cites. The seal was taken with `--review …review-decomposition-rv-plan009-design-opus-r2.dd.json`, so the design gate is bound into the baseline rather than asserted beside it.

`AdapterDescriptor`, `LocationHint` and `AdapterCapabilities` landed in `crates/core/src/catalog.rs` as `Copy` structs of `&'static str` and `&'static [T]`. Every field is a compile-time constant. This is the frontend-neutral pure DTO the design called for, and its purity is structural: there is no owned `String`, no path type, no lazily-resolved field, so there is nothing in the type that *could* consult the environment. `crates/cli/src/lib.rs` re-exports the three types, which is how `crates/app` reaches them without a direct core dependency it is not allowed to declare — a pass-through, not a layering inversion, and consistent with how the app already reaches core types through `unisphere_sdk`.

## AC-by-AC

**ac-0001 — one v1 envelope, registered production adapters only.** `output::json` writes `{"ok":true,"command":"adapters.list","v":1,"data":{"adapters":` then serialises the borrowed slice. The recorded vd-0002 stdout is that exact envelope with a single `claude-code` entry carrying id, application and a concise description. The integration helper `descriptor()` asserts `ids == ["claude-code"]` on every one of the three real-process cases, so "production only" is checked three times against the shipped binary rather than once.

**ac-0002 — hints that never assert installation.** The hint is `home` + `.claude/projects` + `*/*.jsonl` + `jsonl`, all asserted field-by-field in `catalog_json_is_static_under_hostile_environment`. The non-assertion property is proven by the harder of the two available tests: `catalog_does_not_resolve_inaccessible_store_roots` writes a *file* where a home directory would be, asserts `fs::read_dir(blocked.join(".claude/projects")).is_err()` so the traversal genuinely cannot succeed for any user including a privileged runner, then requires the command to still exit 0 with a non-absolute hint path. A catalog that resolved anything would fail there. This is a real negative test, not a restatement of the positive one.

**ac-0003 — honest capability separation.** The DTO carries `sdk_caller_owned_cursor: true` and `cursor_source_assumption: "append_only"` beside `cli_persisted_resume`, `delayed_revision_reconciliation` and `lossless_archive`, all false, and the integration test pins all seven wire values. `docs/cli.md` explains the distinction in the field table — the cursor row says it is "not a guarantee of safe resume after arbitrary source mutation", and the assumption row names in-place rewrite, regrowth and truncation above a checkpoint as not fully detected. That matches `docs/fidelity.md` rather than softening it.

**ac-0004 — no loading, scan, expansion or execution.** Verified two ways. By reading: `run_adapters` takes only argv, `&CliContext`, the borrowed descriptor slice and two writers, and reaches no loader, inspector or environment; and in `main.rs` the `adapters`/`sessions` branch returns *before* `Inspector::new(StdConfigReader)` is constructed, so on the catalog path the config reader is never built at all. Purity by construction order is stronger than purity by discipline, and it is the detail I most wanted to find here. By execution: `catalog_json_is_static_under_hostile_environment` uses `env_clear()`, sets `HOME`, `CLAUDE_CONFIG_DIR`, `XDG_CONFIG_HOME` and `UNISPHERE_CONFIG` to two different trees — one of which contains a real `.claude/projects/synthetic-project/session.jsonl` holding `SYNTHETIC-PRIVATE-MARKER` — and asserts `changed.stdout == initial.stdout` byte-for-byte plus absence of the marker. Byte-identity across two populated, differently-shaped homes is the correct discriminator; a merely non-crashing catalog would not pass it.

**ac-0005 — one registration drives metadata and dispatch.** `AdapterRegistration { descriptor, run }` holds both, `dispatch` selects with `entry.descriptor.id == name`, and the catalog projection is `registry.each_ref().map(|entry| &entry.descriptor)` — a stack array of borrows, no allocation, and structurally incapable of disagreeing with the dispatch table because it *is* the dispatch table. Fixture exclusion from the production catalog is structural, not asserted: the fixture registration lives inside `#[cfg(all(test, unix))] mod tests` and cannot exist in the shipped binary. The existing one-registration test still proves both halves through a single entry — catalog shows `["fixture-text"]`, export emits `unisphere.source.adapter == "fixture-text"` and content passes through — and it survived the change unmodified in substance.

**ac-0006 — help, invalid arguments and output failure keep the conventions.** `Failure::invalid_arguments(None)` carries no argument content, and `catalog_argument_errors_keep_the_catalog_label_and_hide_input` proves the whole shape at once against the real binary: exit 2, empty stderr, `ok:false`, `command:"adapters.list"`, `error.kind:"invalid_arguments"`, and `SYNTHETIC-PRIVATE-ARGUMENT` absent from stdout. Help is delegated to clap's `DisplayHelp` and emitted through the existing `Response::Help` arm, so it keeps the `command:"help"` label — visible in the recorded help smoke.

## The five R2 obligations

**D1 is genuinely discharged.** `production_catalog_ids_match_exported_provenance` iterates `&ADAPTERS` — the production constant itself, not a copy — invokes `(registration.run)` for each entry, parses the emitted OTLP document and asserts the `unisphere.source.adapter` attribute equals `registration.descriptor.id`. Both degenerate forms the guide forbade are absent: there is no second identity list, and the compared pair is produced by the real export path rather than typed twice. Registering a second adapter extends this test's coverage automatically. This is the finding I graded medium in R1 and it is now closed by construction.

**D2** — `limitations` does not exist in `crates/core/src/catalog.rs`. One representation of the negatives, as contracted.

**D3** — the rename and the new assumption field are in the DTO, the wire envelope, the human rendering and the docs table, with no surface still carrying `sdk_cursor_resume`.

**D4** — `crates/app/tests/adapter_catalog.rs` exists with the three real-process cases, and vd-0004 is recorded exit 0. The check is non-vacuous as designed: it names a test target that must exist.

**D5** — `json_failure` is parameterised by command label, so `Response::Failure` keeps `config.check` and `Response::CatalogFailure` emits `adapters.list` with no leakage between them; the exit mapping treats both failure variants identically at 2/1; human failures route to stderr while human success goes to stdout; the write-failure branch emits the bare diagnostic and returns 1 without a second envelope. The human success rendering opens with `Registered adapters; location hints are not detected installations:` — the interactive misreading guard, present verbatim.

**E1 is fixed, in the file and shape I named.** `docs/cli.md` now states: "Catalog JSON failures go to stdout; session-command errors remain on stderr. Human catalog failures go to stderr. This deliberate stream distinction preserves the existing command contracts." The divergence is now documented rather than discovered, which was the whole ask.

The external output-failure proof is real and is the right kind of proof: a separate crate outside the workspace depending on `unisphere-cli` by path, driving `run_adapters` with a writer that fails on `write` in one pass and on `flush` in the other, asserting exit 1 both times and that `SYNTHETIC-PRIVATE-IO` never reaches stderr. That exercises the contract as an embedding consumer sees it, not as an insider test.

## C1 — the human path has no automated regression (low, accepted)

All three tests in `adapter_catalog.rs` pass `--json`. The human surface — the not-detected-installations label, the hint and capability lines, and the human-failure routing to stderr with empty stdout — is evidenced only by one-time captures: `catalog-human-smoke.json`, `catalog-human-error-smoke.json` and a real-PTY observation in `catalog-terminal-observation.json`. `crates/cli` has no unit tests at all, so nothing in the four declared checks fails if that label is edited away.

The behaviour is correct today; I read the code and the captures agree with it. What is missing is durability, and it lands on the one line whose entire job is to stop a human from reading a plausible path as a discovery. `docs/cli.md` does say human layouts are not machine-stable contracts, and I accept that for *layout* — but the non-assertion claim is a product property, not a layout detail. The encoding is cheap: a fourth case in the file that already exists, using the existing `catalog()` helper, asserting the human stdout contains the label and that `--human` with a bad argument yields empty stdout, exit 2 and a stderr message free of the hostile token. Accepted, not blocking, and worth doing before the next adapter lands.

## C2 — the check ledger records three of four declared checks (low, accepted)

`assets/team/composition.dd.json` records vd-0002, vd-0003 and vd-0004, each exit 0. vd-0001 (`cargo test -p unisphere-core --lib`, "Inherited pure core contract behavior") has no row, so the durable artifact does not carry the 4-of-4 claim on its face.

I checked whether the property is nonetheless proven rather than assuming either way. `.harness/extensions/checks/checks.mjs` runs `cargo test --workspace --all-targets --locked` as its `tests` gate, which is a strict superset of `-p unisphere-core --lib`, and vd-0003 is recorded exit 0 — so core's inherited unit tests did execute and did pass, and `crates/core/src/lib.rs` gaining `mod catalog;` broke nothing. This is a bookkeeping hole, not a proof hole. Recording the vd-0001 row, or annotating the receipt that vd-0003 subsumes it, closes it. Accepted.

## Working-tree state and evidence provenance

`assets/catalog-io-smoke.json` was uncommitted-modified when I began. I reviewed the committed bytes and then read the delta: it adds `recorded_at`, `argv`, `cwd` and `temporary_consumer_removed`, and changes nothing about the consumer source or its result. Provenance metadata only, so the external consumer proof I assessed is the same proof either way. `candidate-commit.json`, `composition-verification.json`, `composition-verify-before-init.json`, `solo-composition-init.json` and `assets/team/composition.dd.{json,md}` are untracked at the subject; I verified the composition receipt at the digest the packet pinned, and note that it is not yet committed alongside the candidate it certifies.

Every source and doc file I read hashes exactly to the digest recorded for it in the composition receipt's 373-file inventory, so the record describes the commit I reviewed. The four `baseline-owner` warnings are unmapped-ownership advisories on the two frozen contract files and do not bear on this candidate.

## Scope of this approval

This approves the composition: the candidate implements the approved design, the six ACs hold against the real binary, and the declared checks that were recorded are green. It is not a statement about adapters that do not exist, about non-Unix platforms, or about resume safety beyond the append-only assumption the catalog now declares. The two accepted findings are durability work, best done when the second adapter arrives and the provenance loop starts earning its generality.
