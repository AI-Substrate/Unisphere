# Development and collection proof

Unisphere 0.1.0 implements explicit configuration inspection, bounded Unix JSONL/native snapshot loading, pure native adapters and OTLP output through SDK/CLI. It does not discover private stores implicitly, start a daemon, or claim lossless/full-session telemetry.

## Toolchain and checkout

Use Rust 1.95.0, edition 2024, with clippy and rustfmt. Install/select it explicitly with rustup if needed:

```sh
rustup toolchain install 1.95.0 --profile minimal --component clippy --component rustfmt
cargo +1.95.0 build --workspace --locked
cargo +1.95.0 test --workspace --all-targets --locked
cargo +1.95.0 test --workspace --doc --locked
```

`rust-toolchain.toml` requests a version from rustup; it does not override an unrelated Homebrew executable earlier on PATH. The quality gate observes the actual Rust tools before running checks. Approved releases/commits:

| Tool | Release | Commit identity |
|---|---|---|
| rustc | 1.95.0 | `59807616e1fa2540724bfbac14d7976d7e4a3860` |
| Cargo | 1.95.0 | `f2d3ce0bd7f24a49f8f72d9000448f8838c4e850` |
| clippy | 0.1.95 | `59807616e1` |
| rustfmt | 1.9.0 / 1.9.0-stable | `59807616e1` |

Equivalent distributions are accepted by release and emitted commit identity, not a machine-specific path. A formatter emitting only `rustfmt 1.9.0` may pass when its release and the other emitted identities match, but its commit remains null/not-emitted with an explicit provenance warning; no formatter hash match is claimed. For stronger optional observation, run `rustup run 1.95.0 rustfmt --version` if that equivalent toolchain is already installed. Missing executables, failed probes, release mismatches or mismatched emitted hashes return `E_TOOLCHAIN_PREREQUISITE`. Gates do not install tools or mutate global defaults.

Cargo dependency fetches require registry access on an uncached build. Product runtime requires neither the registry nor Node, DD, OMP, Builder, or an async runtime. Linux and macOS are the configured CI targets; a workflow definition is not evidence that a particular run passed. Permission-denied smoke requires an unprivileged POSIX account. Windows behavior is not claimed by this lane.

## Run the installed CLI

Choose an installation root you own:

```sh
cargo +1.95.0 install --locked --path crates/app --root ./local-install
./local-install/bin/unisphere --help
./local-install/bin/unisphere --version
./local-install/bin/unisphere config check --source-root ./sessions --json
./local-install/bin/unisphere config check --config ./missing.json --json
./local-install/bin/unisphere config check --source-root ./sessions --human
```

Success exits 0, configuration/I/O failures exit 1, and invalid arguments exit 2. Captured output defaults to one versioned JSON object plus LF on stdout with no diagnostic contamination; explicit human successes use stdout and human failures use stderr. The missing-file example intentionally fails with `UNI-CONFIG-READ` and remediation. Roots are uninterpreted strings: no expansion, normalization, existence probe, or session read occurs. Configuration precedence is defaults, then the explicitly supplied document, then explicit overrides; `--clear-source-roots` clears the list and conflicts with `--source-root`.

See [SDK usage](sdk.md) and [CLI reference](cli.md) for public contracts and configuration examples. This development lane is independently authored from the frozen core/testkit contracts; it does not import another project's implementation or require unpublished binaries.

## One quality gate, one foundation smoke lane

The engineering harness is optional development tooling, installed globally rather than as a product dependency. Use Node >=22 and `@ai-substrate/engineering-harness` 0.14.0 or a compatible later CLI:

```sh
npm install -g @ai-substrate/engineering-harness@0.14.0
harness checks --json
harness boot --json
```

With rustup, a command-local `RUSTUP_TOOLCHAIN=1.95.0` selects that toolchain only when the executable resolves through rustup. Ensure PATH selects the intended distribution; inspect the emitted provenance instead of assuming the override worked.

`checks` records actual tool versions/provenance, then runs formatting, clippy, workspace tests, rustdoc, dependency/source-purity checks and wrapper regressions. `boot` calls checks once and runs the five real proof modes below using the observed Cargo path; only all-success returns `ready:true`, scoped to `configuration-and-native-session-projections`. No service starts and no full-fidelity or network-denial claim is made.

## Standalone proof commands

Run from the checkout root after all product crates are composed:

```sh
cargo run --locked -p unisphere-testkit --bin unisphere-arch-check
cargo test --locked -p unisphere-testkit --bins
cargo run --locked -p unisphere-testkit --bin unisphere-proof -- composition
cargo run --locked -p unisphere-testkit --bin unisphere-proof -- sdk-consumer
cargo run --locked -p unisphere-testkit --bin unisphere-proof -- installed-cli
cargo run --locked -p unisphere-testkit --bin unisphere-proof -- collection
cargo run --locked -p unisphere-testkit --bin unisphere-proof -- native
node --test .harness/extensions/checks/checks.test.mjs .harness/extensions/boot/extension.test.mjs
```

`unisphere-proof <command> --repo /explicit/checkout` also accepts explicit repository context. It fails if the required SDK or app target is absent; placeholders, help-only output, and the baseline core/testkit suite cannot satisfy composition.

- `composition`: builds a real external consumer and the app; compares complete SDK/CLI success/error envelopes for defaults, explicit roots, overrides/clearing, all shared invalid documents, missing/unreadable/directory/oversized files, and invalid documents hidden by an override. It also exercises hostile-environment SDK self-checks.
- `sdk-consumer`: materializes `Cargo.toml.template` into a temporary standalone Cargo workspace, calculating the SDK path at runtime. The ordinary external caller exercises both facade and injected-reader APIs. Distinct sealed baseline/HOME/XDG/UNISPHERE subprocesses must return identical explicit configuration and safe error results.
- `installed-cli`: performs a real `cargo install --path crates/app --root <temporary-root>`, then invokes the installed binary outside the checkout. Exercises help/version, default machine-mode success/failures, explicit human success/failures, stream routing, and conflicting arguments.
- `unisphere-arch-check`: inspects Cargo metadata declarations, including optional, renamed, target-specific and normal/dev/build edges. Only present approved crates are required, so core/testkit can run independently. SDK may directly use existing `serde` for visitors; production dependencies on testkit and reversed dependencies remain forbidden. Negative fixture graph data must fail; `--metadata FILE` checks an explicitly supplied metadata fixture.
- `collection`: builds an external consumer using the real loader/adapter/writer, compares SDK/CLI OTLP records across bounded batches and metadata/content policy, checks hostile environment isolation, partial tails, malformed input and output-file collision protection, then installs and runs the real CLI outside the checkout.
- `native`: builds a real external SDK consumer and temporary installed CLI, compares every registered native JSONL/document/journal/SQLite dialect in both content policies, checks the catalog, exercises changed/deleted document and SQLite projections plus delayed/partial journals, and proves a real partial destination write returns no checkpoint. It reuses the existing collection decoder with source-representation-specific provenance requirements.

The source-purity sensor scans production core and every approved adapter source tree, rejecting explicit effectful constructs using negative fixtures. It excludes testkit and conventional trailing test modules; zero core or adapter source scans fail. Aliases/macros/indirect effects still require independent source review; see [adapter guidance](adapters.md).

Proof builds retain explicit compiler access but isolate HOME, config, Cargo cache, target and install roots under a fresh temporary directory. The compiler's observed sysroot binds child compilation to the selected compiler; no private path is committed. Built products run through the shared `sealed_command` helper with cleared environment and empty PATH. Hostile variables are set on individual child commands, never in global test-process state. Temporary manifests are not live nested workspace packages. Scratch directories are automatically removed on completion/failure, while child diagnostics are reported before exit.

Automated behavior establishes no observed ambient configuration influence, not a syscall trace. Independent review must separately inspect core/SDK for ambient reads, network/client calls, process launches, FFI, unsafe escape and forbidden dependencies. Actual TTY detection requires separate terminal evidence; captured/explicit-mode tests do not claim it.

## Ownership and proof reports

Within Builder work, coders author scoped source and behavioral tests; the PM runs formatting, builds, tests and the assembled commands against the composed commit. Do not commit another lane's files, generated `Cargo.lock` changes, dispatch seeds or `.harness/temp` evidence. Use `harness commit` with explicit owned paths. Report which commands actually ran, their exits and evidence; authored scenarios are not passing scenarios.
