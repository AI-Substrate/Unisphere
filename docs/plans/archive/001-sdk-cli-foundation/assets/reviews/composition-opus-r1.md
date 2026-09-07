# Composition review r1 — independent source and behaviour

- Reviewer: `pij-xenacious-yarpen` (omp, github-copilot/claude-opus-5, effort high), native root `/Users/jordanknight/substrate/unisphere/unishpere-main`
- Subject: `560dbd2bf89b3ba030c2c8259c91e5b5f9417ecc` · baseline `1ec7213` · integration `83bb8e44`
- Plan `bc208a54…fe9d5d26c87d25270806ad72b6b0a53d9155ae4cff1b2871fe3cc` · Guide v7 `9a2977c8…899d` — both recomputed from committed blobs and equal to the packet
- Recorded: 2026-09-07T08:21:10Z
- Verdict: **approved** for source and behaviour — no material source finding. Zero open findings. The first-class compose receipt remains an explicit prerequisite.

## Headline: the E477 delta is formatting, and I can prove it

`composition-ownership-refusal.json` records `harness builder compose --verify 0b119041…` refusing with
`E477 Undeclared PM integration changes` over eleven paths. The PM's characterisation — formatter
output plus AGENTS.md fact corrections plus four proof-binary lint fixes, not smuggled source edits —
is the load-bearing claim of this round, so I tested it rather than accepting it.

First, the imports are clean. Diffing each unit's delivered commit against its imported commit over
that unit's owned tree gives **no difference at all**: `cdcb1d69` ≡ `9edfea2` on `crates/sdk`,
`49654b0a` ≡ `fd04322` on `crates/cli`, `d5d9a25` ≡ `83bb8e4` on `crates/testkit`. So everything in
`83bb8e4..560dbd2` over those paths is PM integration work — 1,191 lines across the eleven files, far
more churn than "some formatting" sounds like.

Line counts cannot settle this, because rustfmt reflow rewrites line boundaries. So I normalised each
file at both commits by stripping all whitespace and then rustfmt's added trailing commas, and
compared the residue:

| Path | Non-whitespace residue | Result |
|---|---|---|
| `crates/cli/src/args.rs` | 2746 → 2746 | formatting only |
| `crates/cli/src/lib.rs` | identical on whitespace strip alone | formatting only |
| `crates/cli/src/output.rs` | 3366 → 3366 | formatting only |
| `crates/sdk/src/fs.rs` | 2218 → 2218 | formatting only |
| `crates/sdk/src/service.rs` | identical on whitespace strip alone | formatting only |
| `crates/cli/tests/frontend.rs` | 16034 → 16034 | formatting only (+392/−74 lines) |
| `crates/sdk/tests/public_api.rs` | 5310 → 5310 | formatting only |
| `crates/sdk/tests/service_in_isolation.rs` | 9788 → 9788 | formatting only (+185/−63 lines) |
| `crates/testkit/src/bin/unisphere-arch-check.rs` | 5266 → 5274 | residue diffed |
| `crates/testkit/src/bin/unisphere-proof.rs` | 16272 → 16273 | residue diffed |

For the two files with residue I diffed the normalised streams character by character. Every
difference is rustfmt converting `=> expr,` into `=> { expr }` on match arms — eleven such arms in
`unisphere-arch-check.rs`, one in `unisphere-proof.rs`. No identifier, literal, condition or control
edge changed.

The four lint fixes are isolated in `560dbd2` alone, nine insertions against eleven deletions in one
file, and each is a provable identity: `!x.as_str().is_some_and(|s| !s.is_empty())` →
`x.as_str().is_none_or(|s| s.is_empty())` twice, a nested `if let` collapsed into a let-chain, and
`from_mode(0)` → `from_mode(0o0)`. No suppression, no `#[allow]`, no assertion weakened.

**So E477 is a declaration/fence gap, not hidden source edits.** The PM's claim is correct, and it is
now correct on evidence rather than assertion.

## The chain from my r6 approval holds

`team/baseline-v7.dd.json` binds `source_sha 1ec7213` and guide `9a2977c8` — the commit and guide I
approved at r6 — and its `review` field carries digest `e3c91d26…`, exactly my r6 receipt. Both r6
artifacts landed byte-identical to what I emitted (`fd0d743b…`, `e3c91d26…`), discharging the closure
I named last round. All three units record `baseline_sha 1ec7213`. `composition.dd.json` cites
baseline digest `c9fabdbd…`, which equals the committed `baseline-v7.dd.json` bytes.

And the frozen set held through composition: **all 18 sealed files are byte-identical at
`560dbd2`**, zero mismatches. Three coders and a PM integration touched none of the frozen contract.

## bp-0008 — source-surface judgement, performed explicitly

I read every production file and scanned the non-test regions for ambient configuration and
environment reads, network and client construction, process launches, background work, FFI and unsafe
escapes. The complete result:

- **core** (`lib.rs`, `config.rs`, `ports.rs`, `errors.rs`) — nothing. No `std::env`, `std::fs`,
  `std::net`, `std::process`, no thread or async, no `unsafe`, no `build.rs`. Sole dependency
  `serde`. `#![forbid(unsafe_code)]`.
- **sdk** — exactly one I/O call in the entire crate: `File::open(path)` in `fs.rs`, reached only
  from `ConfigSource::File` after an absolute-path check, bounded to `max_bytes + 1`, with metadata
  taken from the open handle rather than the path. Zero environment reads, zero network, zero process
  launches, no threads, no async, no FFI. `#![forbid(unsafe_code)]`.
- **cli** — zero. All context (`cwd`, `stdout_is_terminal`, `version`) arrives through `CliContext`;
  writers are caller-owned. `#![forbid(unsafe_code)]`.
- **app** — the imperative shell, and the only place ambient state is read: `env::current_dir`,
  `env::args_os`, compile-time `env!("CARGO_PKG_VERSION")`. That is precisely where a composition
  root should read it.

Dependency surface corroborates: the shipped closure is `serde`, `serde_json`, `clap` and clap's
rendering crates. `tempfile` and `unisphere-testkit` are dev-only and `testkit` is `publish = false`.
No `build.rs` exists anywhere in the workspace. `unisphere-arch-check` enforces this kind-aware
against `cargo metadata --locked` on *declared* edges, so an optional or target-specific dependency
cannot slip past on a machine that never builds it, and internal edges must use a local path so a
registry substitute cannot impersonate a contract.

**Judgement: bp-0008 is discharged at the grade the guide approves** — independent core/SDK
source-surface inspection plus sealed hostile-environment behaviour. No syscall trace and no executed
network-denial run is required, and I do not treat their absence as a gap. I am not escalating.

The hostile-environment half is real rather than decorative: `sealed_command` does `env_clear`, sets
`PATH=""`, and redirects HOME/XDG/TMP into a temporary root, and `hostile_consumer` then runs the
external SDK consumer four times — baseline, HOME-poisoned, XDG-poisoned, and
`UNISPHERE_CONFIG`/`UNISPHERE_CONFIG_PATH`/`UNISPHERE_SOURCE_ROOTS`/`UNISPHERE_OUTPUT`-poisoned —
with `{"source_roots":["ambient-must-not-win"]}` planted at three plausible discovery paths, and
requires **byte-identical stdout** across all four.

## Proof tooling drives production, not fixtures

`unisphere-proof` builds a genuine external Cargo project from
`fixtures/consumer/Cargo.toml.template` with a runtime-resolved SDK path, builds `unisphere-app` with
`--locked`, and performs a real `cargo install --locked --path crates/app --root <temp>` before
invoking the installed binary from outside the checkout. `require_file` refuses every lane when the
target is absent, and `absent_targets_fail_before_any_build` asserts all three lanes fail with
"required target absent" while leaving the scratch directory uncreated — so a placeholder or the
baseline core/testkit suite cannot satisfy composition. `machine()` rejects non-JSON, extra newlines,
stderr contamination, wrong ok/error discrimination, non-actionable errors, and any output containing
`SENSITIVE-CONFIG-MARKER`. That leak assertion is not vacuous: the marker really is embedded in
`malformed.json`, `UNKNOWN_KEY` and `INVALID_UTF8`, so hostile bytes genuinely flow through the parser
on every run. Child failures retain status, stdout and stderr; `child_failure_preserves_status_and_both_streams`
proves it. Compiler access is deliberately retained for the build lane while HOME, XDG, `CARGO_HOME`,
`CARGO_TARGET_DIR` and the install root are all redirected under a fresh temporary directory, and the
sysroot is resolved from the observed compiler rather than assumed.

## Acceptance-criteria coverage

Coverage below means *this reviewer verified the mechanism in committed source*; where a result is
cited it is PM-observed and labelled so.

| AC | Mechanism verified in source | Coverage |
|---|---|---|
| ac-0001 external in-process SDK | real external Cargo project, facade + injected reader, no CLI spawn, no runtime | covered |
| ac-0002 documented config, deterministic, typed rejections | custom visitors reject unknown/duplicate/null/non-object/non-string/blank/oversize; field indexes in `Location` | covered |
| ac-0003 precedence identical, no ambient reads | one `resolve` shared by both surfaces; `parity_case` compares complete envelopes; hostile-env equality | covered |
| ac-0004 installed CLI agrees with SDK | real `cargo install`, help/version/success/invalid/missing parity | covered |
| ac-0005 versioned envelope, mode precedence | `output.rs` writes one object plus LF; `mode()` exact-flag scan beats terminal detection; `ColorChoice::Never` | covered |
| ac-0006 typed actionable failures, no leakage | `&'static str` message/fix; parser prose confined to numeric line/column; marker assertions | covered |
| ac-0007 isolation + forbidden-edge fixture fails | `service_in_isolation.rs`; four negative architecture fixtures each asserted to fail with the edge kind | covered |
| ac-0008 deterministic, parallel-safe, isolated, no network | private temp roots, per-command hostile vars never global, parallel-safety tests, source-surface judgement above | covered |
| ac-0009 fresh checkout builds, no Node/DD at runtime | workspace manifests, `rust-toolchain.toml`, no `build.rs`, no repo-private paths in committed source | covered |
| ac-000a one real lane, failing check propagates | `checks`/`boot` extensions; 11 Node regressions incl. "failing product check … stops later gates" and "a real harness child failure is not laundered into readiness" | covered |
| ac-000b docs reproduce success and failure; licence/provenance | README/`docs/sdk.md`/`docs/cli.md`/`docs/development.md` read against source; envelope examples match `output.rs` byte for byte; licence table matches `Cargo.lock` exactly | covered, see recommendation 1 |

All eleven remain `unchecked` in `plan.dd.json`; marking them is Builder's act on the first-class
receipt, not mine.

Documentation accuracy I checked rather than assumed: the `invalid_arguments` envelope printed in
`docs/cli.md` matches `output.rs` field order and `errors.rs` strings exactly; the success envelope
matches; the THIRD_PARTY_NOTICES table (clap 4.6.6, serde 1.0.229, serde_json 1.0.151, tempfile
3.27.0) matches `Cargo.lock` exactly; `LICENSE` exists. Scope disclaimers are honest throughout —
README states the no-network grade is source and dependency inspection rather than a runtime trace,
`docs/development.md` states a workflow definition is not an observed CI result, and `boot` returns
its three limitations in the envelope itself rather than in prose only.

## Findings

**No material source finding.** Nothing in the composed source changes the semantics the frozen core
contract and the r6-approved guide describe, and nothing I read leaks input values, reads ambient
configuration, or fakes composed behaviour.

Three recommendations, none blocking and none a finding:

1. Only the two crate-level doc examples in `sdk/src/lib.rs` are compiled by the rustdoc gate. The
   examples in `README.md`, `docs/sdk.md` and `docs/cli.md` are correct today — I checked each against
   source — but nothing prevents drift. Smallest fix: `#![doc = include_str!("../../../docs/sdk.md")]`
   or an equivalent doctest include, so ac-000b's examples are compiled rather than proofread.
2. `checks` resolves `cargo` through PATH and records `toolchain.cargo.provenance.invoked` precisely
   because `rust-toolchain.toml` does not enforce PATH; `boot` then parses that envelope but launches
   the three proof lanes with a bare `cargo`. Same PATH makes them the same binary in practice.
   Smallest fix is one line: take the invoked path from the parsed envelope.
3. `crates/testkit/src/bin/unisphere-proof.rs`, `unisphere-arch-check.rs` and
   `fixtures/consumer/main.rs` are committed mode `100755`. Cosmetic; `git update-index --chmod=-x`.

## Named prerequisite

`team/composition.dd.json` currently carries `files: []`, `checks: []` and **no `artifact_sha`**. It
is a real record of the import — baseline digest, three unit commits, packet digests, integration
`83bb8e44` — and nothing more. First-class `harness builder compose --verify` has not succeeded; it
refused E477 and the amendment is owned upstream. So this approval covers source and behaviour only;
it is not a compose verification and must not be presented as one. When the amendment lands,
`artifact_sha` must bind `560dbd2` (or its successor) rather than `integration_sha 83bb8e44`, which
predates `crates/app` entirely and therefore attests nothing about the reviewed candidate.

## Method and limits

Read-only, against committed bytes obtained with `git show <sha>:<path>`: `rev-parse`, `status`,
`log`, `diff --name-status/--stat/--numstat`, SHA256 recomputation, normalised source comparison, and
dependency-closure analysis from the manifests. Per the packet I executed **no** build, test, linter,
formatter, `ddocs` or `harness` verb, and committed nothing. Every check result cited here — the
quality gates, the three assembled smoke lanes, the PTY smoke, and the boot envelope — is a
**PM observation** read from evidence, not my own execution. What I verified independently is that
the source those results describe is the source at `560dbd2`, and that the evidence's own digests and
conclusions reproduce from committed bytes.

Timing I can confirm from the record: the candidate committed at 08:09:56Z, and `composed-boot-full.json`
(uncommitted PM evidence, `subject_sha 560dbd2`) reports checks at 08:12:07Z and boot ok / `ready: true`
at 08:12:31Z with all six gates and all three proofs at exit 0 — the first evidence in this plan
captured strictly *after* its candidate commit. `composed-quality-corrected.json` at 08:07:43Z
preceded the commit and describes the same working tree. In that run rustfmt emitted a commit hash and
`provenance_gaps` was empty, so the accepted F8 null-commit path was not exercised; the local proof
used a fully observed toolchain tuple, as the packet states.

The working tree was dirty at close with PM-owned in-flight files (`execution-log`, `tasks`, and five
untracked assets including the boot capture). My review binds committed bytes at `560dbd2` and is
unaffected. Writes were confined to the two authorised paths, both new.
