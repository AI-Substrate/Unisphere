# Independent decomposition review — Plan 001 SDK/CLI foundation

**Reviewer:** `pij-angry-hekarro` (independent; not PM, not an implementation identity)
**Scope:** decomposition
**Subject SHA:** `1902f266518b11ad4e80cfaabc5e8f1551a50b5d`
**Inputs re-derived, not trusted from the packet:**

| Path | SHA-256 | Status |
|---|---|---|
| `docs/plans/001-sdk-cli-foundation/plan.dd.json` | `bc208a54522fe9d5d26c87d25270806ad72b6b0a53d9155ae4cff1b2871fe3cc` | matches packet; worktree blob == committed blob |
| `docs/plans/001-sdk-cli-foundation/assets/impl-guide.dd.json` | `b182ef04db573efa1be574e0f5b7e4dc51b087bd90a6663e653751e3e468552a` | matches packet; worktree blob == committed blob |

Both digests were recomputed from the worktree and independently from `git show HEAD:<path>`; they agree, so the reviewed bytes are the committed bytes. The only non-clean path at this SHA is untracked `assets/reviews/reviewer-launch-evidence.json` (see Evidence).

**Verdict: changes-requested.** Five medium findings are open. None invalidates the architecture — the five-crate split, the wave-0 baseline, the three independent lanes and the proof ladder are sound and are the right shape for the operator's parallel-construction requirement (RQ-017). The findings are contract holes and fence/proof omissions that would each surface as mid-flight rework or an unplanned baseline change after sealing, which is precisely when they are most expensive.

---

## 1. Do the three coder lanes actually compile and test independently after the baseline?

**Yes, with F2 and F3 fixed.** This is the strongest part of the guide.

The load-bearing decision is the fifth crate. `unisphere-app` exists so that `crates/cli/Cargo.toml` never has to name `unisphere-sdk`; the CLI lane depends on `unisphere-core` + `clap` + `serde_json` and is exercised through `FakeInspector`, so tk-0003 compiles and passes `cargo test -p unisphere-cli` in a clone containing only baseline + itself. Without that boundary the CLI manifest would need an unfinished sibling and the "no artificial finished-CLI prerequisite" requirement would be violated at the manifest level. The guide states this rationale explicitly (`architecture.contracts[13]`) and it is correct.

Checked and confirmed:

- Wave-1 units `tk-0002`, `tk-0003`, `tk-0005` each declare `depends_on: ["tk-0001"]` only, and each `reads` block names only tk-0001 paths. No same-wave sibling read or import exists anywhere in the guide.
- No empty SDK/CLI/app crate is created at baseline. `baseline.files` contains only real `crates/core/**` and `crates/testkit` library sources plus five config fixtures; `members = ["crates/*"]` admits each lane package when it actually arrives, so no member is declared for a directory that does not exist.
- `Cargo.lock` is deliberately excluded from `baseline.files` and PM-owned. This is the correct call: the CLI lane necessarily materialises `clap` into the lock the first time it runs `cargo test`, and freezing the lock would have made every lane's first command a fence violation.
- The `tk-0005` fence lands inside the same *package* as baseline testkit but on disjoint *paths* (`src/bin/**`, `fixtures/architecture/**`, `fixtures/consumer/**` vs `src/lib.rs`, `src/fakes.rs`, `src/sealed.rs`, `src/fixtures.rs`, `fixtures/config/**`). Cargo auto-discovers `src/bin/*.rs`, so tk-0005 genuinely needs no manifest edit, as the guide asserts. I verified the path sets are disjoint mechanically.
- The proof tools reach the SDK/CLI by spawning processes and taking binary/source paths from explicit context rather than by linking them, so tk-0005 compiles before any sibling exists.

The one real coupling that is *not* handled is F2: the architecture and consumer fixtures tk-0005 must ship are themselves Cargo packages, and they are placed inside a workspace member's directory.

## 2. Are the contracts complete enough to implement compatibly and safely?

**Nearly.** The contract block is unusually concrete for guide stage — exact struct fields and defaults, exact enum variants, exact error codes, the exact machine envelope including key order and the LF, exact exit codes, exact precedence including `Some([])` clearing, `MAX_CONFIG_BYTES` applied to *both* inline and file paths, the absolute-`File`-path rule, and the instruction to validate every supplied layer including values that are later overridden. Two safety decisions deserve to be called out as right: refusing to serialize `ConfigSource`/`InspectionRequest` into diagnostics because they carry input bytes, and putting `code`/`message`/`fix` behind fixed core accessors so SDK/CLI parity (ac-0004, ac-0006) is structural rather than a discipline both lanes have to maintain independently.

The hole is F1: the guide specifies how a `Failure` is *read* but never how one is *constructed*, and two different lanes must construct one. `UNI-ARGS-INVALID` / `invalid_arguments` can only originate in the CLI lane (clap rejects the argv), while `UNI-CONFIG-INVALID` / `UNI-CONFIG-READ` originate in the SDK lane. Both types live in core, which is baseline-owned and sealed before either lane starts.

Faithfulness to foundation-only scope is good: `contracts[0]` names the exclusions explicitly, `rk-0006` guards against source-root validation drifting into store discovery, and the workshop's standard-first OTLP direction is correctly quarantined as later work. The guide does not import any workshop schema into Plan001.

## 3. Are write fences non-overlapping and read owners correct?

**Fences: yes, verified mechanically** — no pair of units shares a path or a glob prefix. Read owners and `depends_on` edges are correct in both directions: tk-0004 reads all four other units and depends on all four; the wave-1 lanes read and depend on tk-0001 alone.

**One gap: F3.** `Cargo.lock` sits in `tk-0001.paths`, is absent from `baseline.files` (correct) — and is absent from `tk-0004.paths`, even though `tk-0004.interface` and composition step 6 both require the PM to refresh it while composing. The composed commit will therefore contain a lockfile write that no tk-0004 fence authorises. Same PM identity owns both units, so nothing will break in practice, but a fence check at `compose --import`/`--verify` is exactly the mechanism that is supposed to catch this class of thing, and it should not have to be argued away.

Manifest hotspot control is otherwise right: root `Cargo.toml` stays tk-0001-only, lanes use `<dep>.workspace = true`, and any genuinely new dependency is routed through the PM rather than negotiated between lanes.

## 4. Do all eleven ACs have an accountable owner and meaningful executable proof?

**Owners: yes, verified.** All eleven ACs appear exactly once in `capabilities`, every owner resolves to a declared unit, and every `#checks/vd-*` and `#acceptance_criteria/ac-*` reference in the guide resolves — zero dangling references. Seven ACs are owned by `tk-0004`, which is appropriate: those are the composed claims, and the guide is explicit that import is not proof.

**Proof quality: mostly real, not unit-only.** The ladder contains a genuine external Cargo consumer built in a temp directory against a path dependency (`vd-000a`), a genuine `cargo install --path` plus execution of the installed binary outside the checkout (`vd-000b`), a genuine differential of the real app binary against the real SDK over valid/malformed/missing/unreadable/oversized fixtures (`vd-0004`), a negative dependency-graph fixture that must fail (`vd-0005`/`vd-0006`), and a harness regression that injects a failing child check to prove the wrapper propagates failure instead of reporting readiness (`vd-000f`). That last one is the check most guides omit, and it is the one that makes ac-000a mean something.

Three proof gaps:

- **F4 (medium)** — ac-0003's "library calls do not read ambient HOME, process environment or global config implicitly" lists only `vd-0002`, an in-process `cargo test -p unisphere-sdk` suite. That suite cannot mount the hostile-ambient fixture `bp-0003` asks for: `sealed_command` builds a *subprocess* environment and tk-0002 ships no binary, and in edition 2024 `std::env::set_var` is unsafe and not parallel-safe, which `ac-0008` independently forbids. A `FakeReader` call-recorder proves no *reader* access, not no *ambient* access. The same shape applies to ac-0008's "no network calls": nothing in `vd-0002/0003/0004/000a` asserts it.
- **F7 (low)** — no proof ever executes the human render path of the real binary. `stdout_is_terminal` is injected so `vd-0003` covers both branches in-process, but the actual `IsTerminal` capture lives in `crates/app/src/main.rs` and `vd-000b` runs the installed binary with captured stdout, i.e. machine mode only.
- **F8 (low)** — "Node/OMP/Builder unavailable at product runtime" is asserted in `vd-000b`'s description, but `sealed_command` is contracted as "temporary HOME/config roots and scrubbed relevant variables" with no mention of PATH. Without PATH reduction the claim is argued, not executed — and tk-0005 is explicitly forbidden from changing testkit's lib, so the mechanism has to be in the baseline contract or it cannot be added later without reconciliation.

`vd-0008` (clippy) and `vd-0009` (fmt) are referenced by `tk-0004`/`composition` but by no AC. That is defensible — no AC claims static cleanliness — and I am not raising it as a finding.

## 5. Does the guide follow the actual Builder lifecycle and the requested models?

**Yes — and this is verified against the installed CLI, not assumed.** I read `harness builder --help` and the help for each named subcommand. Every command and flag the guide's composition steps rely on exists with the claimed grammar:

| Guide step | Installed grammar | Result |
|---|---|---|
| `harness builder contracts <plan> --seal --review <receipt>` | `contracts [--seal] [--review <path>]` | exists |
| `harness builder ready <plan> --unit <id>` | `ready [--unit <id>]` | exists |
| dispatch with clone isolation and `--parent` | `dispatch [--unit] [--workspace] [--parent] [--kind guide\|worktree\|clone] [--harness] [--model] [--effort]` | exists; `--kind clone` makes `isolation.mode: clone-per-coder` executable, and the `guide` default means the guide's own mode is honoured without an override |
| `harness builder ack` before release | `ack [--receipt <path>]` | exists |
| `harness builder compose <plan> --import <deliveries.json>` | `compose [--import <path>]` — "importing is not proof" | exists |
| `harness builder compose <plan> --verify <SHA>` | `compose [--verify <sha>]` | exists |
| `harness builder review <plan> --receipt <path>` | `review [--receipt <path>]` | exists |
| `harness builder advance` for departures | `advance` | exists |

The guide also states, correctly and in the installed CLI's own terms, that `guide --check` is structural only and not architectural judgement. Roles are explicit (`rl-0001` coder `github-copilot/gpt-6-astra` high; `rl-0002` reviewer `github-copilot/claude-opus-5` high) and both carry the honest note that these are requested configuration, not provider-served attestation — which matches what I can and cannot observe about my own runtime.

No future code, baseline SHA or AC completion is claimed anywhere. `rk-0005` and the `tk-0001` notes state plainly that everything listed is future until its unit executes, and composition step 2 forbids inventing a baseline SHA during guide authoring. The isolation note honestly records that the installed dispatcher refuses linked-worktree coder allocations and selects clones for that reason rather than silently substituting.

**F5** is the one lifecycle-adjacent problem, and it is environmental rather than procedural: the declared toolchain pin does not work on the observed host.

---

## Findings

| ID | Sev | Finding | Smallest correction |
|---|---|---|---|
| F1 | medium | `Failure` has no specified construction surface. `architecture.contracts[4]` gives accessors only (`code`/`message`/`fix`/`retryable`), but `tk-0003` must emit `InvalidArguments`/`UNI-ARGS-INVALID` from clap rejections and `tk-0002` must emit `InvalidConfiguration`/`ConfigurationRead`. The type is core-owned and sealed at wave 0, so neither lane can add constructors without a frozen-signature reconciliation mid-flight. | Name the public constructors in the baseline contract, e.g. `Failure::invalid_configuration(Location)`, `Failure::configuration_read(ReadFailure, Option<Location>)`, `Failure::invalid_arguments(Option<&str>)`, and state that code/message/fix text is core-owned. |
| F2 | medium | `tk-0005` owns `crates/testkit/fixtures/consumer/**` and `crates/testkit/fixtures/architecture/**`; both are necessarily Cargo packages (the arch fixture must declare a forbidden edge; the consumer fixture must declare a path dependency on the SDK). A live nested `Cargo.toml` inside a workspace member's directory is a known Cargo failure mode, and the fix — `[workspace] exclude` — lives in the root manifest, which is tk-0001-owned and frozen before tk-0005 starts. | Either add `exclude = ["crates/testkit/fixtures/**"]` to the baseline root-manifest contract now, or state in `tk-0005.interface` that fixture manifests ship as non-buildable templates (`Cargo.toml.template`) materialised into the temporary root by `unisphere-proof`/`unisphere-arch-check`. |
| F3 | medium | `tk-0004`'s write fence omits `Cargo.lock`, while `tk-0004.interface` and composition step 6 both require the PM to refresh it during composition. `Cargo.lock` appears only in `tk-0001.paths`, and correctly not in `baseline.files`. The composed commit therefore carries a write no tk-0004 fence authorises. | Add `Cargo.lock` to `tk-0004.paths`. Leave root `Cargo.toml` in tk-0001 alone so a new shared dependency stays an explicit baseline reconciliation. |
| F4 | medium | ac-0003's no-ambient-read claim and ac-0008's no-network claim have no executable negative proof. ac-0003 lists only `vd-0002`, an in-process suite that cannot host the hostile-HOME fixture `bp-0003` requires — `sealed_command` is subprocess-only, tk-0002 ships no binary, and `std::env::set_var` is unsafe and parallel-unsafe under edition 2024. | Add `vd-000a` to ac-0003's proof list and require `unisphere-proof sdk-consumer` to run the external consumer under sealed hostile `HOME`/`XDG_*`/`UNISPHERE_*` values, asserting the resolved `Configuration` is unchanged. State the no-network property as a dependency-surface argument (core/sdk pull only serde/serde_json) rather than an executed check, or drop it from the proven set. |
| F5 | medium | The toolchain pin is inert on the observed host. Observed: `rustc 1.95.0 (59807616e 2026-04-14) (Homebrew)`, `cargo 1.95.0`, **no `rustup` on PATH**, `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`, `clippy 0.1.98 (88d9e12ae1 2026-08-18)`. `rust-toolchain.toml` is a rustup mechanism: with no rustup it is silently ignored, and on a rustup-equipped coder clone it forces a 1.95.0 download with components. The installed clippy/rustfmt also report a different, newer toolchain than the installed rustc, so `vd-0008`/`vd-0009` would execute under components incoherent with the declared channel. ac-0009's "documented Rust toolchain" would be a file, not evidence. | Keep `rust-toolchain.toml` but stop treating it as the pin. Have `tk-0005`'s `checks` extension record and assert observed `rustc`/`cargo`/`clippy`/`rustfmt` versions, and document the observed baseline in `docs/development.md`. |
| F6 | low | `vd-0002`'s `tests/service_in_isolation.rs` technique requires `#[path]`-including `src/service.rs` into an integration-test crate, which only compiles if that module contains no `crate::`/`super::` references. The constraint is unstated and would surface as late rework in tk-0002. | One clause in `tk-0002.interface`: the isolated service module must be self-contained, referring to dependencies only through absolute `unisphere_core::`/`serde_json::` paths. |
| F7 | low | No check executes the real binary's human render path. `stdout_is_terminal` is injected so `vd-0003` covers both branches, but the actual `IsTerminal` capture lives in `crates/app/src/main.rs` and `vd-000b` runs the installed binary with piped stdout only. | Have `unisphere-proof installed-cli` invoke the installed binary once with explicit `--human` and assert the human projection and stream routing; no PTY needed. Or record the terminal-detection limit explicitly against ac-0005. |
| F8 | low | "Node/OMP/Builder unavailable at product runtime" has no named mechanism. `sealed_command` is contracted as temporary HOME/config roots plus "scrubbed relevant variables"; PATH is not mentioned, and tk-0005 may not modify testkit's lib to add it. | Name PATH reduction explicitly in the baseline `sealed_command` contract (tk-0001), before sealing. |
| F9 | low | An oversize `ConfigSource::Inline` payload has no assigned failure kind. `MAX_CONFIG_BYTES` is contracted to apply to inline bytes, but `ReadFailure::TooLarge` → `ConfigurationRead` describes a read that never happened, and `InvalidConfiguration` is not stated either. `vd-0004` is told to exercise oversized fixtures. | State which kind and code an oversize inline payload produces. |

## Evidence and limits of this review

- Read-only. No build, test, formatter, linter or project gate was run. The only executions were digest recomputation, `git` inspection (`rev-parse`, `status`, `show`, `worktree list`), structural cross-referencing of the two DD documents in-process, `--version` probes of `rustc`/`cargo`/`rustfmt`/`clippy`, and `--help` reads of `harness builder` and its subcommands. `command -v rustup` returned nothing.
- Native launch root is the main clone `/Users/jordanknight/substrate/unisphere/unishpere-main`; every reviewed artifact was read by absolute path from `/Users/jordanknight/substrate/unisphere/unisphere-sdk-cli-foundation`. The native root was not changed and is not claimed to be the subject worktree.
- Supporting context read at the subject SHA: `requirements-spine.md`, `original-ask.md`, `assets/backpressure.dd.json`, `assets/workshops/001-output-format.md`.
- `assets/reviews/reviewer-launch-evidence.json` is **untracked** at `1902f26`, so it is not covered by the subject SHA. Its spawn/state/ps facts nonetheless corroborate what I observed independently: session `01a07a09-0d66-7000-8240-75cc3de1bec5`, pane `%4003`, pid `56117`, folder as above, and argv `bun …/omp --auto-approve --model github-copilot/claude-opus-5 --thinking high`. I re-derived those from `pij-rs list --json` and `ps -p 56117` rather than adopting them from the file.
- Runtime honesty: harness `omp`, model selector `github-copilot/claude-opus-5` and effort `high` are observable only as requested configuration — in process argv (`--thinking high`) and in the pij-rs seat record (`effort: high`). **Provider-side model identity and reasoning effort are not verified and are not claimed.**
- No product code, guide, plan, flow, governance file, canonical `assets/team` receipt or peer file was modified. Nothing was committed or pushed. No coder was launched. No AC, task or acceptance state was marked complete. This review approves nothing beyond its own verdict.
