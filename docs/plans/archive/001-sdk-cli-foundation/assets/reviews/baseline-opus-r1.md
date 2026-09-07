# Plan001 baseline review r1 — independent source-bound decomposition review

- **Subject SHA:** `18ee5165011eb1a7dcfd013b30c75fba2f55f7ee` (`feat(core): establish explicit configuration contracts and isolated testkit`, Jordan Knight, 2026-09-07T15:00:22+10:00)
- **Scope:** decomposition (source-bound baseline review before Builder seal)
- **Reviewer:** `pij-xenacious-yarpen` — OMP, `github-copilot/claude-opus-5`, effort `high`
- **Reviewer native root:** `/Users/jordanknight/substrate/unisphere/unishpere-main` (main clone; `.git` is a directory, git-dir == git-common-dir, primary entry in `git worktree list --porcelain`)
- **Subject workspace inspected read-only:** `/Users/jordanknight/substrate/unisphere/unisphere-sdk-cli-foundation`
- **Plan:** `docs/plans/001-sdk-cli-foundation/plan.dd.json` — sha256 `bc208a54522fe9d5d26c87d25270806ad72b6b0a53d9155ae4cff1b2871fe3cc` (verified equal at working tree and at subject SHA)
- **Guide:** `docs/plans/001-sdk-cli-foundation/assets/impl-guide.dd.json` — sha256 `95da8bf6c3d65077ae18085a379c9c4f780571a7f2ef62d5ac1a65644ae6cdb0` (verified equal at working tree and at subject SHA)

## Verdict

**approved.** No high-severity finding. Nothing found blocks sealing the committed baseline. Six findings are recorded below (two `open` and intended as PM actions **before lane dispatch**, four `accepted` residual risks). Baseline approval claims no product acceptance criterion: all eleven ACs and all twenty task assertions remain `unchecked` at this SHA, and this review executed no build, formatter, linter or test.

## What I executed and what I did not

Executed (read-only): `git rev-parse`/`worktree list`/`show`/`ls-tree`/`log`, `shasum -a 256`, byte-level reads of committed blobs, host toolchain **version probes** (`rustc -V`, `clippy-driver -V`, `rustfmt -V`, `cargo -V`, directory listings under `~/.rustup`, `~/.cargo/bin`), process/registry self-observation.

Not executed, and therefore not claimed: `cargo test`, `cargo clippy`, `cargo fmt`, `cargo build`, any harness verb, any syscall/network trace. Where I rely on execution, I rely on the PM's committed evidence (`assets/baseline-checks.json`, `assets/baseline-check-initial.json`) and say so explicitly.

## Commit shape

The commit contains exactly 20 files: the 17 entries of `impl-guide.dd.json#baseline/files`, plus `Cargo.lock` and the two evidence assets. Set difference computed against the guide's frozen list:

- baseline files missing from the commit: **none**
- extra files in the commit: `Cargo.lock`, `assets/baseline-check-initial.json`, `assets/baseline-checks.json`

No stray or out-of-fence file landed.

## Review focus findings

### 1. Core types, ports and safe failure API vs guide; sufficiency for independent lanes — **satisfied**

Every contracted symbol exists with the contracted shape.

- `Configuration { source_roots: Vec<String> }` with `Default` → `[]`; `ConfigOverrides { source_roots: Option<Vec<String>> }` defaults `None`; `ConfigSource = Defaults | Inline(Vec<u8>) | File(PathBuf)` with `Defaults` as `#[default]`; `InspectionRequest { source, overrides }`; `InspectionReport { configuration }` (`crates/core/src/config.rs`).
- Serialization fence is exactly as designed: `Serialize` on `Configuration`, `InspectionReport`, `FailureKind`, `Location` only. `ConfigSource` and `InspectionRequest` are deliberately **not** `Serialize`, so input bytes cannot reach a diagnostic envelope (`rk-0003`).
- `MAX_CONFIG_BYTES = 1_048_576` is public (`config.rs:6`), so both the inline-oversize (`invalid_configuration`) and file-oversize (`ReadFailure::TooLarge`) rules are implementable by the SDK lane without a second constant.
- `Failure` has private fields with the three contracted constructors and the full accessor set `kind/location/read_failure/code/message/fix/retryable`, plus `Display` and `Error` (`crates/core/src/errors.rs`). Copy is core-owned `&'static str`; `retryable()` is unconditionally `false` in scope. Codes are exactly `UNI-CONFIG-INVALID`, `UNI-CONFIG-READ`, `UNI-ARGS-INVALID`.
- `FailureKind` serializes `snake_case`, which is bit-for-bit what the CLI machine envelope requires (`"kind":"invalid_configuration|configuration_read|invalid_arguments"`). Coder B needs no adapter enum.
- Ports (`crates/core/src/ports.rs`) are the contracted synchronous `ConfigReader::read(&self, &Path, usize) -> Result<Vec<u8>, ReadFailure>` and `InspectionApi::inspect(&self, &InspectionRequest) -> Result<InspectionReport, Failure>`, both `Send + Sync`. No async, no registry, no global.
- No SDK/CLI behavior is stubbed into core. There is no parser, no filesystem adapter, no argument type. That is correct for tk-0001.

Per-lane sufficiency without sibling code: tk-0002 has core DTOs/ports/`MAX_CONFIG_BYTES`/`FakeReader`/fixtures and inherits `serde_json`; tk-0003 has `FakeInspector`, the serializable failure surface and inherits `clap`; tk-0005 can build `unisphere-arch-check` from `std::process` + `serde_json` against `cargo metadata` and `unisphere-proof` from `std::process`/`std::fs` + `sealed_command`, all within testkit's existing dependencies as its fence requires.

### 2. Fakes, fixtures and `sealed_command` implement real contracts; tests defend behavior — **satisfied**

- `FakeReader` (`crates/testkit/src/fakes.rs`) enforces absolute fixture keys with an assertion (defended by a `#[should_panic]` test), honours `max_bytes` by returning `ReadFailure::TooLarge` **before** handing back bytes, returns cloned values (the test mutates the returned buffer and re-reads to prove isolation), maps unknown paths to `NotFound`, and records every `ReadCall{path,max_bytes}` including the rejected ones (`calls.len() == 5`).
- `FakeInspector` records requests for both `Ok` and `Err` configurations and is exercised under `std::thread::scope` with 8 concurrent `inspect` calls asserting 8 recorded requests — a genuine parallel-safety defence (`ac-0008`/`bp-0008` at fake level), not a smoke assertion.
- `sealed_command` (`crates/testkit/src/sealed.rs`) rejects relative binary or root **before** creating any directory (test asserts the target directory does not exist afterwards), creates only `home/config/cache/data/state/tmp` under the caller's `temp_root`, `env_clear()`s, sets `PATH=""`, points HOME/XDG_*/TMPDIR at the temporary tree, and returns the `Command` **unspawned** — proven by constructing against a deliberately nonexistent executable. The Windows branch re-adds only `SystemRoot` (OS loader requirement) plus temp-rooted `USERPROFILE`/`APPDATA`/`LOCALAPPDATA`. This matches the contract "builds (does not run) a child".
- Fixtures are real documents with the intended traps: `roots.json` deliberately carries a leading/trailing-space root, a literal `~/literal`, and a duplicated root, which pins the "preserve order, duplicates, no trimming/expansion" rule; `blank-root.json` uses `" \t\n "`; `wrong-type.json` uses `42`; `malformed.json` embeds `SENSITIVE-CONFIG-MARKER` inside truncated JSON so leakage tests have a searchable token. `INVALID_DOCUMENTS`/`READ_FAILURES` bind each fixture to its expected `FailureKind`.
- Test count corroboration: I counted the committed test functions — core 3 (`config` 1, `errors` 2) and testkit 8 (`fakes` 4, `fixtures` 2, `sealed` 2) = **11**, exactly matching the 3+8 recorded in `baseline-checks.json`. The PM's "11 baseline tests" claim is consistent with the committed source without re-running anything.

### 3. Baseline compiles alone; root dependencies and testkit affordances permit each coder — **satisfied (on recorded evidence)**

- `assets/baseline-checks.json` records `cargo test -p unisphere-core -p unisphere-testkit --lib`, cwd = workspace root, exit 0, 3 + 8 passed. That argv is byte-identical to `impl-guide.dd.json#checks/vd-0001`.
- The committed `Cargo.lock` (26 packages) contains `unisphere-core` and `unisphere-testkit` only, with no `unisphere-sdk`, `unisphere-cli`, `unisphere-app` — and no `clap`. That is independent proof that uninherited `[workspace.dependencies]` entries are not resolved, i.e. the workspace genuinely locks and builds with the sibling crates absent (see F-0003).
- Root manifest is `members = ["crates/*"]` with `resolver = "3"`, shared `edition = 2024`, `rust-version = "1.95"`, and workspace declarations for `serde`, `serde_json`, `clap`, `tempfile` — everything the three lanes inherit.
- `crates/testkit/Cargo.toml` is `publish = false` and depends on `unisphere-core` + `serde_json` + `tempfile`, exactly the declared graph. Note for tk-0005's allowlist: `testkit → tempfile` is a **normal** edge (not dev), so the kind-aware graph check must permit it while still forbidding any shipped crate → testkit edge.

### 4. O1 — `Cargo.lock` ownership — **resolved, no disagreement to escalate**

`Cargo.lock` is committed at this SHA and is correctly **absent** from `impl-guide.dd.json#baseline/files`. That is precisely the arrangement the guide authorizes: single-owner PM-generated state fenced to tk-0004, materialized during baseline prep, outside the frozen tk-0001 contract. Its content corroborates the claim rather than merely asserting it — no product crate, no networking/async/database dependency leaked into the graph. Runtime seal probing remains PM responsibility; I make no claim about it. See F-0004 for the one residual handling risk.

### 5. Five units, twenty assertions, eleven-AC scope retained — **satisfied**

- `assets/tasks/phase-1/tasks.dd.json` carries exactly five units (tk-0001…tk-0005) and exactly twenty `done_when` assertions (`dw-0001`…`dw-0014` in hex: 9 + 6 + 5).
- `plan.dd.json#acceptance_criteria` carries exactly eleven criteria (ac-0001…ac-000b), each with a distinct `bp-0001…bp-000b` pressure row; `backpressure.dd.json#rows` has the matching eleven.
- Coverage: every AC is claimed by at least one unit's `satisfies` (ac-0002/0003 → tk-0002; ac-0005 → tk-0003; ac-0007/000a → tk-0005; ac-0001/0004/0006/0008/0009/000b → tk-0004). No orphan AC, no assertion pointing outside the eleven.
- **No future acceptance is claimed by this approval.** At the subject SHA every AC state and every `done_when` state is `unchecked`, including `dw-0001`/`dw-0002`/`dw-0003` which the PM has since checked in uncommitted canonical progress bookkeeping, and `dw-0004` (this review + seal) which remains unchecked. I reviewed committed subject bytes only and take no position on uncommitted PM bookkeeping.

### 6. Baseline safety / ambient / network / unsafe surface — **inspected; clean at baseline. This is a source-surface judgement, not a syscall trace.**

- `#![forbid(unsafe_code)]` at the crate root of **both** `unisphere-core` and `unisphere-testkit`.
- `unisphere-core` has zero `std::fs`, `std::env`, `std::process`, `std::net`, FFI, `extern "C"`, `static mut`, `OnceLock` or thread-spawn usage. Its entire import surface is `std::path`, `std::fmt`, `std::error` and `serde::Serialize`. There is no ambient configuration read to hide: core cannot read anything.
- `unisphere-testkit` touches the outside world in exactly three audited places, all in `sealed.rs`: `std::fs::create_dir_all` restricted to paths joined under the caller-supplied absolute `temp_root`; `std::process::Command` **construction only** (no `spawn`/`output`/`status` anywhere in the crate); and a Windows-only `std::env::var_os("SystemRoot")` read. `fakes.rs` and `fixtures.rs` are pure — fixtures are `include_bytes!` of committed files, resolved at compile time.
- Dependency graph (committed lock, 26 packages): `serde`/`serde_core`/`serde_derive`/`serde_json`/`itoa`/`memchr`/`zmij`/`syn`/`quote`/`proc-macro2`/`unicode-ident` and the `tempfile` chain (`rustix`, `libc`, `linux-raw-sys`, `errno`, `fastrand`, `getrandom`, `once_cell`, `cfg-if`, `bitflags`, `r-efi`, `windows-sys`, `windows-link`). No HTTP client, no async runtime, no database, no ML crate.
- **Explicit limit:** this is a restricted dependency-and-source-surface inspection of the *baseline only*. It is not an executed network-denial test, and it says nothing about the not-yet-written SDK. `ac-0008`/`bp-0008` remain unproven and the separate composition-review obligation (`dw-0011`) is untouched by this approval.
- Diagnostic-secrecy surface is genuinely defended, not asserted: private `Failure` fields, fixed `&'static str` copy that cannot be replaced by caller data, and a test proving `Display` omits a `PRIVATE-MARKER` location value.

### 7. Toolchain observations are real local-only probes — **satisfied for the probes; the guide's prose describing the host is stale (F-0001)**

`assets/toolchain-observation.json` records four probes by **absolute binary path** with argv, cwd, exit code and full `-Vv` output: rustc 1.95.0 (`59807616e`, 2026-04-14), cargo 1.95.0 (`f2d3ce0bd`), clippy 0.1.95 (`59807616e1`), rustfmt 1.9.0-stable (`59807616e1`) — a coherent suite by commit hash, with `"scope": "command-local selection only; no installation/global mutation"` and selection expressed as a command-local `PATH` + `RUSTUP_TOOLCHAIN`. This is a real probe record, not a `rust-toolchain.toml`-only pin claim. I found no evidence of global mutation and none is implied by the recorded selection; I did not and cannot audit historical global state.

Independent host observation from my native root contradicts the guide's *narrative*, not the probes — see F-0001/F-0002.

## Findings

| ID | Severity | Disposition | Summary |
|---|---|---|---|
| F-0001 | medium | open | Guide toolchain prose ("no rustup", "newer clippy/rustfmt provenance") is factually stale; reconcile before tk-0005 release |
| F-0002 | low | accepted | Two distribution-distinct but version-identical 1.95.0 suites on host; `checks` must match by version+commit-hash, not install path |
| F-0003 | low | accepted | Root `[workspace.dependencies]` pre-declares `unisphere-sdk`/`unisphere-cli` paths that do not exist; inert, but lowers the cost of a tk-0003 contract violation |
| F-0004 | low | accepted | Committed, non-ignored `Cargo.lock` will dirty in every lane; only prose prevents a coder from delivering it |
| F-0005 | medium | open | Shared fixture surface is narrower than the documented validation rules and is fenced against all three coders |
| F-0006 | low | accepted | Evidence assets embed the operator's absolute `PATH` and workspace paths in a repo bound for a public remote |

### F-0001 — guide toolchain prose contradicts the recorded observation (medium, open)

`impl-guide.dd.json#architecture/contracts` ("Toolchain contract") states: *"This host was observed with Homebrew rustc/cargo1.95.0, **no rustup**, and newer clippy/rustfmt provenance."* Independently observed from my native root:

- `~/.cargo/bin/rustup` exists (11 MB binary, mtime 2026-08-26), and `~/.rustup/toolchains/` contains six toolchains: `1.57.0`, `1.85.0`, `1.95.0`, `1.98.0`, `nightly-2026-08-08`, `stable`, all `-aarch64-apple-darwin`. The `1.95.0` toolchain's `bin/` holds `cargo cargo-clippy cargo-fmt clippy-driver rustc rustdoc rustfmt`. The baseline checks were run from that directory. "No rustup" is false.
- Homebrew's suite is `clippy 0.1.95` and `rustfmt 1.9.0` — the **same** versions as the rustup toolchain, not "newer".

Impact: `dw-0014` requires tk-0005's `checks` to "enforce the documented approved coherent toolchain". The documented description is wrong, so a coder implementing to the guide text would encode the wrong expectation. No committed baseline file depends on this prose, so it does not block sealing.

Requested action (PM, guide-owned — I am fenced out of guide edits): reconcile the Toolchain contract text against `assets/toolchain-observation.json`, and state the approved tuple as version + commit-hash, before tk-0005 is released.

Corroboration for the guide's *other* toolchain claim: `rust-toolchain.toml` really is only a request here. The default `rustc`/`cargo` on this host resolve to `/opt/homebrew/bin` (`rustc 1.95.0 (59807616e 2026-04-14) (Homebrew)`), and `rustup` is not on the default `PATH`, so a Homebrew-default shell ignores `rust-toolchain.toml` entirely. The guide is right to refuse to treat that file as enforcement.

### F-0002 — match the toolchain by version+commit-hash, not by install path (low, accepted)

Both available 1.95.0 suites carry identical upstream identity (`rustc 59807616e`, `cargo f2d3ce0bd`); only the distribution differs (`(Homebrew)` suffix, different absolute path). If tk-0005's `checks` pins the recorded absolute rustup path or an exact `-Vv` string, the gate will fail on an otherwise-correct Homebrew-default shell for no product reason. Recommend matching on release + commit-hash and recording the resolved path as evidence.

### F-0003 — root manifest pre-declares nonexistent path dependencies (low, accepted)

`Cargo.toml` declares `unisphere-sdk = { path = "crates/sdk" }` and `unisphere-cli = { path = "crates/cli" }` while neither directory exists. Proven inert: the committed lock resolved 26 packages and contains neither (nor `clap`), and `vd-0001` passed. Residual risk is contractual, not mechanical — it makes `unisphere-sdk.workspace = true` a one-line way for coder B to break "CLI frontend must not import unisphere-sdk", and `vd-0006` (the graph check that would catch it) does not exist until tk-0005 lands. Recommend restating the prohibition in the tk-0003 dispatch packet.

### F-0004 — lockfile will dirty in every lane (low, accepted)

`Cargo.lock` is committed and not ignored by `.gitignore`. Any lane adding a crate (tk-0003 pulls `clap`) rewrites it locally on first build. The "coders never deliver lockfile changes" rule is prose-only; a `git add -A` defeats it. Recommend dispatch packets require path-scoped commits limited to the unit's fence, and that import verification rejects a delivery touching `Cargo.lock`.

### F-0005 — shared fixture surface is narrower than the documented validation rules (medium, open)

`crates/testkit/src/fixtures.rs` covers empty, valid-roots, wrong-type, blank-root, malformed, plus `NotFound`/`PermissionDenied` categories. The guide's configuration semantics additionally require rejecting **non-object documents, unknown keys, duplicate known keys, non-string root elements, non-UTF8 bytes** and **oversized (> `MAX_CONFIG_BYTES`) input** — and `dw-0005` names exactly those. None has a shared fixture. `rk-0003`'s stated check claims "vd-0002/vd-0003/vd-0004 include sensitive-marker malformed/**invalid-UTF8** fixtures"; only the malformed fixture carries the marker, and no invalid-UTF8 fixture exists.

This matters because the fixture module is fenced against every coder: tk-0002/tk-0003 hold `crates/testkit/**` as read-only `reads`, and tk-0005 is explicitly told not to change the baseline testkit lib. So no lane can add these shared fixtures, and `vd-0004`'s "real app and real SDK on the same inputs" comparison loses the non-UTF8/oversize cases unless they are re-created independently in two places.

Not blocking: each lane can define lane-local negative inputs inside its own crate, and non-UTF8/oversize inputs are awkward as committed JSON files (an oversize fixture is better generated at runtime than committed as a 1 MiB blob).

Requested decision (PM, before dispatch): either (a) extend `fixtures.rs` under tk-0001 ownership with an invalid-UTF8 byte constant and an oversize generator helper, or (b) explicitly authorize lane-local negative fixtures for these cases in the dispatch packets and correct `rk-0003`'s wording so it stops claiming a shared invalid-UTF8 fixture that does not exist. (a) is the smaller change and keeps `vd-0004`'s same-input comparison honest.

### F-0006 — evidence assets embed host-private paths (low, accepted)

`assets/toolchain-observation.json` embeds the operator's full `PATH` (including `~/github/tools/scripts`, `~/Library/pnpm`, `/pkg/env/global/bin`, Ghostty app paths) and both evidence files embed the absolute workspace path. That is legitimate provenance for an execution record and is explicitly *not* what `ac-0009`/tk-0005 forbid (private absolute paths baked into **product** code or committed fixtures — the committed fixtures are clean). Flagging only because `workspace.package.repository` points at a public GitHub remote and push authorization is a separate gate: decide before the first push whether host `PATH` belongs in history.

## Observations (not findings)

- `Failure::message()`/`fix()`/`code()` return `&'static str` where the guide says `&str`. Strictly stronger, compatible with the contract.
- `Failure` is intentionally not `Serialize`; the CLI must assemble its error envelope from the accessors. That is the right shape for "code/message/fix copy is core-owned and fixed" and it prevents an accidental structural leak of `location` internals. Coder B should be told to build the envelope field-by-field rather than reaching for a derive.
- `FakeInspector` returns one configured `Result` for every call. Sufficient for per-invocation CLI tests; a lane needing two different outcomes instantiates two inspectors.
- `baseline-check-initial.json` preserves the first (index-updating, 24-package locking) run and `baseline-checks.json` the settled run. Keeping both is good evidence hygiene: it shows lock materialization happened during baseline prep exactly as the tk-0004 authorization describes.

## Boundaries of this approval

Approved: the committed baseline contract surface (core types/ports/failures, testkit fakes/fixtures/sealed helper, root workspace policy, generated lockfile handling) is real, internally consistent, faithful to guide v3, independently buildable, and sufficient for tk-0002/tk-0003/tk-0005 to proceed in parallel without sibling code.

Not approved and not claimed: any product acceptance criterion; `vd-0002`…`vd-000f`; the future SDK/CLI/app source; no-network behavior as an executed test; runtime collector seal probing; and the composition review (`dw-0011`), which remains a separate obligation against a different SHA.
