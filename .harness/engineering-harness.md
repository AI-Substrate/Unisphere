# Engineering harness

> Start with `harness instructions`; read `harness instructions <verb>` for each operation. The CLI is global development tooling, not a product dependency.

## Boot command

`harness boot --json` runs the quality gate once, then the real foundation composition, external SDK consumer and installed CLI smoke commands. Only a valid ok checks envelope and three successful proofs return `ready: true`, explicitly scoped to `configuration-sdk-cli-foundation`. It starts no services and never claims native telemetry collection or reader support. A missing/unconfigured/failing child cannot imply readiness. Details: [development guide](../docs/development.md).

## Checks command

`harness checks --json` first records actual Rust tool versions, emitted commit identities and resolved provenance. The approved Rust 1.95.0 tuple is compared by release and emitted commit, not private paths or distribution labels. A matching rustfmt release that omits commit metadata may pass with an explicit provenance warning, null hash/match and optional stronger-observation command; no missing hash is claimed as matched. Missing binaries, failed probes and version/emitted-hash mismatches fail. `rust-toolchain.toml` alone is not enforcement. Then run formatting, clippy, workspace behavior, rustdoc, declaration-based architecture and harness verdict regressions. Failures retain child status/stdout/stderr and stop later gates. No checks/boot recursion.

## Health and interaction

There is no product daemon, health endpoint or native collector in this foundation. The supported interaction is `unisphere config check`, with explicit configuration and predictable machine/human output. `harness doctor --json` reports harness loading/conventions and machine attribution separately from product proof.

## Deterministic signal inventory

| Signal | Command | Proof boundary |
|---|---|---|
| Harness wiring | `harness doctor --json` | Extensions/conventions, not product behavior |
| Product quality | `harness checks --json` | Actual tool identity and reported Rust/harness gates |
| Foundation readiness | `harness boot --json` | Quality plus all real proof commands; no collection claim |
| Dependency direction | `cargo run --locked -p unisphere-testkit --bin unisphere-arch-check` | Declared normal/dev/build edges, including optional/target/renamed edges; negative graph fixtures |
| Independent proof tools | `cargo test --locked -p unisphere-testkit --bins` | Controlled tool fixtures without hidden SDK/CLI implementation dependency |
| Composition parity | `cargo run --locked -p unisphere-testkit --bin unisphere-proof -- composition` | Real SDK/app success and safe failure parity |
| External SDK | `cargo run --locked -p unisphere-testkit --bin unisphere-proof -- sdk-consumer` | Public facade/injected reader and sealed hostile-environment behavior |
| Installed CLI | `cargo run --locked -p unisphere-testkit --bin unisphere-proof -- installed-cli` | Real temporary installation, outside-checkout runtime, machine/human stream routing |
| Harness propagation | `node --test .harness/extensions/checks/checks.test.mjs .harness/extensions/boot/extension.test.mjs` | Missing/mixed tool identity and failed child propagation; not collector proof |

## Isolation and remaining proof limits

Proof tools create fresh temporary package/HOME/config/cache/target/install roots; Cargo retains explicit compiler access while product subprocesses have an empty PATH and cleared environment. No machine-private SDK path is committed. Fixtures use `Cargo.toml.template`, never nested live Cargo packages. Hostile environment changes are child-local. An absent target fails. Permission proof requires an unprivileged POSIX user; configured Linux/macOS CI is not itself observed platform evidence.

No observed ambient influence is an automated behavior claim. No-network/ambient-read/FFI/unsafe/process guarantees additionally require independent source-surface review of core/SDK and dependency inspection. This lane is not an executed network-denial test, real TTY test, native-reader proof, or telemetry collection readiness.

## Observe and evidence

Capture friction immediately with `harness observe "<what happened>" --kind difficulty --severity degrading --agent <session-slug>`. Read CLI status, data, error, next_action and exit code together.

- `.harness/reports/`: assessment and proof-gap reports.
- `.harness/records/retro/`: durable session lessons, not fabricated runtime evidence.
- `.harness/records/harness-change/`: encoded harness improvements.
- `.harness/temp/`: ignored scratch, observation buckets and local evidence; never commit except its protective `.gitignore`.
- Builder plan execution/review receipts: PM-owned exact-source proof records, distinct from a worker's authored tests.

At handoff read only your observation bucket, retain a durable retrospective through the authorized writer, and clear only your bucket after preservation. Never clear another peer's evidence. Preserve global Git trace2 and collector metadata; machine attribution warnings are not product readiness failures.

## Work seams

`AGENTS.md` routes `/eng-harness-flow --hook pre-flight` at session start, `pre-coding` after scope agreement, `post-coding` at handoff and `post-flight` at closeout. Local pi skills live under `.pi/skills/`. Product code, plan/guide/tasks and proof records belong on Builder product branches; governance has a separate single writer. During fan-out, the PM owns all formatting and validation across deliveries. The adoption bridge advances only from actual composed proof, not from the existence of these wrappers.
