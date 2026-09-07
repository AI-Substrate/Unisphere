# Plan001 baseline review r2 — focused independent re-review

- **Subject SHA:** `633b9bf3c533188bde44e49a9d88f56a9b572bfd` (`fix(plan001): close baseline fixture and toolchain review gaps`)
- **Prior subject:** `18ee5165011eb1a7dcfd013b30c75fba2f55f7ee` (r1) — r1 report and receipt preserved unchanged
- **Scope:** decomposition (source-bound baseline review before Builder seal)
- **Reviewer:** `pij-xenacious-yarpen` — OMP, `github-copilot/claude-opus-5`, effort `high`
- **Reviewer native root:** `/Users/jordanknight/substrate/unisphere/unishpere-main`
- **Subject workspace inspected read-only:** `/Users/jordanknight/substrate/unisphere/unisphere-sdk-cli-foundation`
- **Plan:** `docs/plans/001-sdk-cli-foundation/plan.dd.json` — sha256 `bc208a54522fe9d5d26c87d25270806ad72b6b0a53d9155ae4cff1b2871fe3cc` (unchanged from r1)
- **Guide v4:** `docs/plans/001-sdk-cli-foundation/assets/impl-guide.dd.json` — sha256 `370eaee98d6122eb2581b158ca606236b3318db66d7b694ac16273746c4a3d9e`
- **Backpressure:** sha256 `5e700e9aa22288422c94ea943c7407a0ddf7ba13f4dad50a5d113697718fc6fc` (unchanged from r1)
- **Tasks:** sha256 `daf37136c524f95af057028e41af1eac92e78402fdff8a69d1b679e5989f508c`

## Verdict

**changes-requested.** One material finding is open: **F-0007**, a proof-attribution defect in the canonical record. The three assertions `dw-0001`/`dw-0002`/`dw-0003` are marked `checked` at this SHA, but their sole `proven_by` link resolves to `lg-0001`, whose own text names the *superseded* commit `18ee5165` and an 11-test run (`3 core, 8 testkit`, clippy `--lib` only). The revision added five shared fixtures, a generator and two tests; `dw-0002` explicitly asserts that **all** shared config fixtures are usable, and lg-0001's run exercised none of the new ones.

The correcting evidence already exists in this commit and is digest-bound to the exact reviewed bytes — `assets/baseline-r2-final-checks.json` records 3 + 10 tests, clippy widened to `--lib --tests`, and `cargo fmt --all --check`, all exit 0, over `fixtures_sha256 aa915f10…` which I independently confirmed equals the committed `crates/testkit/src/fixtures.rs`. It is simply not referenced by any canonical document: `git grep baseline-r2-final-checks` at this SHA returns nothing under `docs/`.

So this is not a substance failure. **Everything the r1 review asked to be fixed is genuinely fixed, and I verified each fix against bytes rather than against the disposition file.** The blocker is that sealing now would freeze a record in which a checked assertion cites a run that did not exercise the asserted state. The remedy is inside the PM fence and outside mine: record a new execution-log entry citing `baseline-r2-final-checks.json` at the current commit and repoint the three `proven_by` addresses. I hold no other objection, and I expect to approve on that basis alone.

Two r1 findings are now `fixed`, four are `accepted`, one new `low` is `accepted`, one new `medium` is `open`.

## What I executed and what I did not

Executed (read-only): `git rev-parse`/`status`/`log`/`diff`/`ls-tree`/`show`/`grep`, `shasum -a 256`, in-process structural leaf comparison of guide v3 against guide v4, regex counting of `#[test]` in committed blobs, and version-only toolchain probes (`-vV`) of both installed distributions.

Not executed, and therefore not claimed: `cargo test`, `cargo clippy`, `cargo fmt`, `cargo build`, `ddocs validate`, `harness builder guide`, any harness verb, any syscall or network trace. Where I rely on execution I rely on the PM's committed receipts and say which one.

## Revision shape

The revision is one commit touching 14 files, of which exactly **one is source**: `crates/testkit/src/fixtures.rs`. `crates/core/**`, `crates/testkit/src/{lib,fakes,sealed}.rs`, `Cargo.toml`, `Cargo.lock` and `rust-toolchain.toml` are byte-identical to the r1-reviewed bytes, so every r1 judgement about the core contract surface, the fakes, `sealed_command`, the workspace policy and the lockfile carries forward unchanged rather than being re-asserted on faith.

`plan.dd.json` is untouched (digest identical to r1): the product promise was not edited to fit the evidence.

Guide v3 → v4 compared structurally leaf by leaf, not by eye: **exactly 5 changed leaves**, each read in full.

- `meta.version` 3 → 4 and `meta.updated`
- `architecture.contracts[10]` — baseline fake/fixture contract
- `architecture.contracts[13]` — toolchain contract
- `checks/vd-0004.description` — parity input list

No other guide leaf moved. Contract count is 15 before and after, so nothing was dropped to make room.

## Disposition of r1 findings, verified against bytes

### F-0001 — guide toolchain prose was factually stale → **fixed**

`contracts[13]` is rewritten and now states the correction explicitly rather than quietly: *"The earlier PATH-only probe missed an installed rustup toolchain; current actual evidence is assets/toolchain-observation.json."* The false "no rustup, and newer clippy/rustfmt provenance" clause is gone. Naming the cause of the earlier error is better practice than silently replacing the sentence.

I re-probed both distributions on this host and the newly documented tuple is correct in every element:

| element | guide v4 tuple | Homebrew `-vV` | rustup 1.95.0 `-vV` |
|---|---|---|---|
| rustc | 1.95.0 `59807616e1fa2540724bfbac14d7976d7e4a3860` | release 1.95.0, commit-hash `59807616e1fa2540724bfbac14d7976d7e4a3860` | identical |
| cargo | 1.95.0 `f2d3ce0bd7f24a49f8f72d9000448f8838c4e850` | release 1.95.0, commit-hash `f2d3ce0bd7f24a49f8f72d9000448f8838c4e850` | identical |
| clippy | 0.1.95 commit `59807616e1` | `clippy-driver` reports rustc commit `59807616e1fa…` | identical |
| rustfmt | 1.9.0-stable commit `59807616e1` | `rustfmt 1.9.0` (no commit emitted) | `rustfmt 1.9.0-stable (59807616e1 2026-04-14)` |

Three of four elements verify against both distributions. The fourth is the subject of F-0008 below.

### F-0002 — match by release+commit, not install path → **fixed**

`contracts[13]` now reads: *"Homebrew and rustup distributions may satisfy the same tuple: compare releases and commit identities, not installation paths, distribution labels or exact whole version strings; record resolved binary provenance separately."* That is exactly the correction requested, and the host proves why it is needed: the two distributions differ in label (`(Homebrew)` suffix), in path, and in bundled LLVM (22.1.3 vs 22.1.2) while carrying identical upstream rustc and cargo commit identities. A whole-string or path comparison would reject a correct toolchain; the documented rule does not.

### F-0005 — shared fixture surface narrower than the documented rules → **fixed**

This is fixed in source, in the contract, in the check description and in the tests, and the four move together rather than one paper over the others.

Source (`crates/testkit/src/fixtures.rs`), five new shared constants plus a generator:

- `NON_OBJECT = b"[]"`
- `UNKNOWN_KEY = br#"{"unknown":"SENSITIVE-CONFIG-MARKER"}"#`
- `DUPLICATE_KEY = br#"{"source_roots":[],"source_roots":["duplicate"]}"#`
- `NON_STRING_ROOT = br#"{"source_roots":[42]}"#`
- `INVALID_UTF8 = b"{\"source_roots\":[\"SENSITIVE-CONFIG-MARKER\xff\"]}"`
- `oversized_document() -> Vec<u8>`

All five constants are appended to `INVALID_DOCUMENTS` with `FailureKind::InvalidConfiguration`, so a lane iterating the shared table picks them up automatically instead of needing to know they exist.

Each is the right shape for the rule it pins, and the set is discriminating rather than decorative:

- `NON_STRING_ROOT` (`[42]`) is genuinely distinct from the pre-existing `WRONG_TYPE` (`source_roots: 42`) — element type versus field type, two different rejection sites.
- `UNKNOWN_KEY` and `INVALID_UTF8` both carry `SENSITIVE-CONFIG-MARKER`, which is what finally makes `rk-0003` true. In r1 that risk row claimed shared sensitive-marker malformed **and invalid-UTF8** fixtures when no invalid-UTF8 fixture existed; the row is unchanged in v4 and did not need changing, because the source moved to meet it. That is the better of the two repairs I offered.
- `INVALID_UTF8` places the `\xff` *inside* a string literal after 41 valid bytes, so it is a UTF-8 failure rather than a structural one — it exercises the decoder path, not the parser path.
- `oversized_document()` is `MAX_CONFIG_BYTES + 1` bytes of `{}` followed by spaces: valid JSON parsing to an empty object, exactly one byte over the limit, generated on demand. This is materially better than what I asked for — it satisfies "otherwise-valid, oversized" without committing a megabyte blob, so the oversize path is rejected on size and not incidentally on syntax.

Contract `contracts[10]` now enumerates the same list and adds the parity rule *"All lanes consume the same negative fixtures for applicable SDK/CLI parity cases."* `vd-0004`'s description is widened from "valid/malformed/missing/unreadable/oversized" to the full twelve-input list, so the guide's assembled-parity check names the inputs the fixtures now provide.

Tests: two new, and both defend behavior rather than restate the constant.

- `duplicate_fixture_retains_both_keys_in_the_input_stream` asserts `text.matches("\"source_roots\":").count() == 2` **and** that the document still parses as an object. Its comment states the reason — a `serde_json::Value` collapses duplicate keys — so the test is deliberately checking the byte stream, which is the only place the duplicate survives. That is the correct instinct; a `Value`-based assertion here would silently prove nothing.
- `oversized_fixture_is_valid_json_but_exceeds_the_limit` asserts both legs: exact length `MAX_CONFIG_BYTES + 1`, and that it deserializes to `json!({})`. Length alone would not have proven "otherwise valid".

The extended `invalid_documents_…` test adds decoder/structure assertions for `INVALID_UTF8`, `NON_OBJECT`, `UNKNOWN_KEY` and `NON_STRING_ROOT`.

Implementability check for the downstream lane, since a fixture set is only useful if it is dischargeable: four of the five (`NON_OBJECT`, `UNKNOWN_KEY`, `DUPLICATE_KEY`, `NON_STRING_ROOT`) are rejected by a single `#[derive(Deserialize)]` with `#[serde(deny_unknown_fields)]` — serde reports `invalid type: sequence`, `unknown field`, `duplicate field` and `invalid type: integer` respectively — and `INVALID_UTF8` is rejected by `serde_json::from_slice` before any visitor runs. So tk-0002 discharges the whole new negative set with the derive the guide already implies, plus its existing post-parse blank-root rule. No bespoke validator is required and no lane is handed an unsatisfiable fixture.

### F-0003, F-0004, F-0006 — **accepted**, dispositions recorded

`assets/baseline-review-dispositions.json` records each with a named guardrail: F-0003 a tk-0003 dispatch supplement reinforcing that the pre-declared SDK dependency must not be inherited; F-0004 scoped `harness commit` paths, a coder packet excluding `Cargo.lock`, and Builder import fence enforcement; F-0006 host path provenance retained locally with publication behind separate approval and path review. These match the residual-risk dispositions I proposed in r1 and I have no further objection. They are commitments about future dispatch, not claims about sealed bytes, and I do not treat them as discharged.

## Evidence quality

`assets/baseline-r2-final-checks.json` is the strongest evidence artifact in this plan so far, on three counts.

1. **It is digest-bound to the reviewed bytes.** Its `basis` names `fixtures_sha256 aa915f1065888bf7e5061b37607430722e6c4a58e14d006f4943f2b420887e6c` and `guide_sha256 370eaee9…`. I recomputed both from the committed blobs and they match. This closes the usual "which source did the green run actually cover" hole by construction, and I recommend it become the house pattern for every future check record.
2. **The gate was widened, not merely re-run.** Clippy moved from `--lib` to `--lib --tests`, and `cargo fmt --all --check` was added. The new tests are therefore linted, which is where the new code actually lives.
3. **The failing intermediate run was retained, not discarded.** `assets/baseline-r2-checks.json` preserves an earlier state whose `cargo test` stderr carried `warning: calls to std::str::from_utf8 with an invalid literal always return an error` at `fixtures.rs:85`, from an assertion that has since been replaced. Keeping a superseded, warning-carrying run beside the clean one is deliberate honesty; it is also what let me confirm that the final run's clean stderr reflects a real code change rather than a narrowed command.

Independent corroboration without executing anything: I counted `#[test]` attributes in the committed blobs — core `config.rs` 1, `errors.rs` 2; testkit `fakes.rs` 4, `fixtures.rs` 4, `sealed.rs` 2 = **13**, matching the recorded 3 + 10, and every recorded test name resolves to a committed function.

On the PM's "current DD validation zero issues after commit": the retained run shows `ddocs validate` **degraded** with `error 0, warn 3`, all three `address-target-untracked` (`E432`) pointing at `assets/execution-log.dd.json` from the three `tk-0001` `proven_by` addresses. I did not re-run `ddocs`, but the claim is verifiable from the commit without doing so: the warning class is literally "target is not tracked", and `git ls-tree` confirms `execution-log.dd.json` **is** tracked at `633b9bf`. The warnings were an artifact of validating before the commit and are self-resolving. The PM's statement is consistent with the committed state.

## Focus items retained from r1

Every r1 conclusion below rests on bytes that are unchanged at this SHA (verified by name-only diff: `crates/testkit/src/fixtures.rs` is the only non-doc file touched), so these are carried forward, not re-litigated.

1. **Core types, ports and safe failure API** — unchanged; contract-faithful and sufficient for all three lanes.
2. **Fakes and `sealed_command`** — unchanged; fixtures strengthened as above. `sealed_command` still constructs and never spawns: there is no `spawn`/`output`/`status` call anywhere in the crate.
3. **Workspace compiles baseline alone** — `Cargo.toml`/`Cargo.lock` unchanged; lock still resolves 26 packages with no `unisphere-sdk`/`unisphere-cli`/`clap`.
4. **O1 / `Cargo.lock`** — unchanged and still correctly absent from `baseline.files`. Runtime seal probing remains PM responsibility; I claim nothing about it.
5. **Scope retained** — 5 units, 20 assertions, 11 ACs, 11 backpressure rows, all counts confirmed at this SHA.
6. **Safety surface** — the only source change adds byte constants, a `Vec<u8>` generator and tests. It introduces no `fs`, `env`, `process`, `net`, FFI or `unsafe`; `#![forbid(unsafe_code)]` still stands in both crates. The r1 limit still applies: this is a source-and-dependency-surface judgement over the baseline only, **not** an executed network-denial test, and `bp-0008`/`dw-0011` remain unproven.
7. **Toolchain probes are local-only** — re-confirmed, and now with the tuple independently verified against both installed distributions.

## Findings

| ID | Severity | Disposition | Summary |
|---|---|---|---|
| F-0001 | medium | fixed | Guide v4 toolchain prose corrected; tuple verified against both installed distributions |
| F-0002 | low | fixed | Guide v4 mandates release+commit comparison, forbids path/label/whole-version pinning |
| F-0003 | low | accepted | Pre-declared SDK/CLI workspace deps; tk-0003 dispatch supplement recorded |
| F-0004 | low | accepted | Lockfile delivery risk; scoped-commit and import-fence guardrails recorded |
| F-0005 | medium | fixed | Five shared negative constants + `oversized_document()`; contract, `vd-0004` and tests all moved together |
| F-0006 | low | accepted | Host `PATH` in evidence assets; publication behind separate approval |
| F-0007 | medium | **open** | `dw-0001/2/3` are `checked` citing only `lg-0001`, which names the superseded commit and an 11-test run; the digest-bound 13-test proof is committed but canonically unreferenced |
| F-0008 | low | accepted | Homebrew `rustfmt` emits no commit hash, so tk-0005's tuple check must degrade per-binary or it can never pass on that distribution |

### F-0007 — checked assertions cite superseded proof (medium, open)

At `633b9bf` the three `tk-0001` assertions carry `"state": "checked"` with a single `"proven_by": "../../execution-log.dd.json#entries/lg-0001"`. `lg-0001`'s text reads: *"Baseline commit 18ee5165011eb1a7dcfd013b30c75fba2f55f7ee: cargo test … exited 0 (3 core, 8 testkit); scoped cargo clippy --lib -D warnings exited 0. … Command/output receipts: assets/baseline-checks.json."*

The mismatch is concrete, not stylistic:

- The cited commit is the **previous** subject. `fixtures.rs` changed after it.
- The cited counts are **11** tests; the current source contains **13**.
- The cited clippy is `--lib`; the current gate is `--lib --tests`, and the new code is in tests.
- Sharpest instance: `dw-0002` asserts *"bounded fake reads and **all shared config fixtures** are usable independently."* The shared fixture set grew by five constants and one generator in this very commit. The run cited as proving that assertion executed **none** of them.

`dw-0003` is materially unaffected (`sealed.rs` is byte-identical), and `dw-0001`'s core leg is unaffected (`crates/core/**` byte-identical), but all three share the one stale pointer.

Not a substance failure, and I want that on the record plainly: `assets/baseline-r2-final-checks.json` **does** prove all three assertions for the current bytes, it **is** committed at this SHA, and it is digest-bound to the exact `fixtures.rs` under review. The defect is that no canonical document references it — `git grep -n baseline-r2-final-checks 633b9bf -- docs` returns nothing — so the canonical answer to "what proves `dw-0002`?" is a run of superseded bytes.

Why this blocks rather than rides along: sealing freezes the record. A future auditor reading `dw-0002 = checked, proven_by lg-0001` finds an entry that names a different commit and eight testkit tests, and has no path from the assertion to the evidence that actually covers it. That is exactly the proof-attribution failure this review stage exists to catch, and it is cheaper to fix now than to explain later.

Requested action (PM-owned; `execution-log.dd.json` and `tasks.dd.json` are both outside my write fence): add an execution-log entry for `633b9bf` citing `assets/baseline-r2-final-checks.json` with the 3 + 10 counts, the widened clippy and the `fmt --check` leg, and repoint the three `proven_by` addresses at it — or at both entries if the history is worth keeping. No source change is required and no other finding stands between this baseline and approval.

### F-0008 — Homebrew `rustfmt` emits no commit identity (low, accepted)

`contracts[13]` names `rustfmt 1.9.0-stable commit 59807616e1` as a tuple element and instructs comparison by "releases and commit identities". Observed on this host:

- rustup: `rustfmt 1.9.0-stable (59807616e1 2026-04-14)` — release and commit both present.
- Homebrew: `rustfmt 1.9.0` — **no commit hash, no `-stable` suffix**.

So the documented tuple element is unobtainable from the Homebrew binary's own output, which sits in tension with the same paragraph's promise that *"Homebrew and rustup distributions may satisfy the same tuple."* A tk-0005 check demanding commit identity for all four binaries would permanently fail on Homebrew rustfmt; one silently skipping the element would weaken the gate without saying so.

Accepted rather than open: this constrains a future unit's implementation, not the sealed baseline bytes, and the guide already requires resolved provenance to be recorded separately. Safeguard for tk-0005: compare commit identity **where the binary emits one**, require release match for all four, and record per-binary which elements were verifiable — so a Homebrew rustfmt is a named partial verification rather than either a silent pass or an impossible failure. I am not asking for a guide edit before seal.

## Observations (not findings)

- `oversized_document()` is a function, so it cannot live in the `INVALID_DOCUMENTS` const table. A lane iterating that table gets the eleven document cases but not the oversize case; `vd-0004`'s description names oversized inputs explicitly, so the gap is covered by the check rather than by the data structure. Worth a sentence in the tk-0002/tk-0003 packets so nobody assumes table iteration is exhaustive.
- `INVALID_UTF8` is categorized `InvalidConfiguration` for both the inline and file paths, which is coherent: the reader returns bytes successfully and the decode failure surfaces at parse time. No `ReadFailure` variant is needed for it.
- The `duplicate_fixture_…` test's comment explains *why* it inspects the byte stream instead of a parsed `Value`. That reasoning is exactly right and is the kind of comment that survives contact with a future maintainer.
- `context-brief.md` and the tasks `summary` dropped their hard-coded "guide v3" references in favour of "current guide" plus an explicit "latest independent approval and seal are required before dispatch". Good: the version pin was a staleness trap, and the replacement states the actual gate.
- The LLVM divergence between distributions (22.1.3 Homebrew vs 22.1.2 rustup) is further evidence for v4's "not exact whole version strings" rule, and neither affects the tuple.

## Boundaries of this review

Judged: the committed baseline contract surface at `633b9bf`, the five changed guide leaves, the revised fixture module, and the committed check receipts.

Not judged and not claimed: any product acceptance criterion (all eleven remain `unchecked`); `vd-0002`…`vd-000f`; the unwritten SDK/CLI/app source; no-network behavior as an executed test; runtime collector seal probing; and the composition review (`dw-0011`), which remains a separate obligation against a different SHA. `dw-0004` — this review and seal — is correctly still `unchecked` at this SHA.
