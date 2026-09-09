# Contributing to Unisphere

Unisphere standardises supported agent-session telemetry through a Rust SDK and
CLI. Contributions should improve native coverage or integration without hiding
source limitations or coupling pure mapping to storage access.

## Development setup

Use Rust **1.95.0** with `clippy` and `rustfmt`; the workspace uses edition 2024.
[just](https://just.systems/man/en/installation.html) is optional convenience
for installation. Ubuntu and macOS CI exercise the current runtime; do not infer
Windows execution or live-vendor coverage from that result.

```sh
git clone https://github.com/AI-Substrate/Unisphere.git
cd Unisphere
cargo build --locked --workspace
cargo run --locked -p unisphere-app -- adapters list --json
```

`just install` rebuilds/reinstalls from this checkout into `~/.local` by default.
Use a separate prefix when validating an installation, rather than replacing your
normal binary:

```sh
just install "/tmp/unisphere-contributor-prefix"
/tmp/unisphere-contributor-prefix/bin/unisphere --version --json
```

The engineering harness is **development tooling**, not a CLI/SDK runtime
requirement. Reuse it if installed. If it is missing, follow the conditional
[agent setup guidance](AGENTS.md#engineering-harness); do not treat a missing tool
as permission to invent a successful check. CI's published checks/boot runtime pin
is separate from the Builder-capable tooling used for managed plans. See
[development.md](docs/development.md) for exact toolchain/provenance details.

## Repository and architecture map

| Area | Responsibility |
| --- | --- |
| `crates/core` | Pure collection/configuration contracts, descriptors and revision types. |
| `crates/sdk` | Application orchestration over constructor-injected ports. |
| `crates/loader-jsonl`, `crates/loader-snapshot` | Explicit bounded filesystem/read-only SQLite access. |
| `crates/adapter-*` | Pure native-data mapping and format-specific fixtures. |
| `crates/output-otlp` | Bounded OTLP JSONL encoding to a caller-owned writer. |
| `crates/cli` | Frontend commands over core contracts and supplied context. |
| `crates/app` | Executable and explicit adapter/runner registration. |
| `crates/testkit` | Fakes, shared conformance, architecture checks and executable integration proof. |

Dependencies point inward. The SDK must not acquire concrete adapter or CLI
imports. Mappers receive data and return records: **no filesystem, environment,
network, process or clock access**. Keep storage, mapping and destination policy
separate; no hidden service locator or mandatory background service.

## Add or extend an adapter

1. Establish the native schema from authoritative source and bounded structural
   evidence. Record supported versions/dialects and unknowns; do not copy another
   project's aggregation assumptions as source truth.
2. Implement the applicable `SessionAdapter` or `SnapshotAdapter` contract in the
   owned crate. Keep physical-record identity distinct from logical messages and
   snapshots distinct from byte cursors. Do not invent timestamps, model facts or
   usage totals when native data is absent or has a different scope.
3. Provide the static descriptor with accurate location hints and capabilities.
   Hints are not installed-app detection, and journal replay is not persisted
   revision reconciliation.
4. Add synthetic native fixtures and meaningful behavioral regressions. Reuse
   shared conformance where applicable, and exercise malformed/unknown data,
   content policy, partition/replay behavior and native semantic boundaries.
5. Add one real descriptor-plus-runner entry in `crates/app/src/adapters.rs`.
   Extend its representation-appropriate fixture and the external/native proof
   cases so the catalog, actual export and emitted adapter ID remain consistent.
6. Declare only needed inward dependencies and update the architecture policy if
   genuinely required. Document field mappings and classified fidelity gaps.

See [adapter guidance](docs/adapters.md), the [telemetry profile](docs/telemetry-profile.md)
and existing adapter documents rather than introducing a parallel convention.

## Change a loader or output writer

- Keep source selection explicit; no implicit HOME scan or reference traversal.
- Enforce record/aggregate bounds during reads, not after unbounded materialisation.
  Raw-byte limits are not measured peak-memory guarantees.
- Preserve the read-only SQLite transaction and final-leaf safety boundaries;
  native keys and revision digests must not masquerade as LF byte offsets.
- Reduce mutation journals under their native operation semantics before mapping
  messages; never export each patch as a chat event.
- Publish a checkpoint only after output acceptance. A short write or flush error
  may leave bytes but must not become a successful checkpoint.
- Preserve empty replacement projections and honest unknown finality. Do not
  imply raw retention, history, deduplication or exactly-once ingestion without
  implementing and proving that contract.

Details: [JSONL loading](docs/session-loader.md), [snapshot loading](docs/snapshot-loader.md),
[SDK orchestration](docs/sdk.md) and [output encoding](docs/telemetry-profile.md).

## Tests and fixture privacy

Run the proof appropriate to the changed contract. Common direct commands are:

```sh
cargo test --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run --locked -p unisphere-testkit --bin unisphere-arch-check
cargo run --locked -p unisphere-testkit --bin unisphere-proof -- native
```

With the development harness available, `harness checks --json` runs quality,
architecture and wrapper regression gates; `harness boot --json` adds real
external-SDK/CLI/installed-binary proof. Read the actual exit, status and scope:
compiling, an authored test or a green unrelated lane is not proof of your change.

Prefer tests that fail on plausible behavior, boundary or error regressions.
Include explicit native identity and usage expectations, unknown/invalid parts,
partial JSONL/journal input, changed/deleted revisions, content exclusion and
output failure when relevant. A new adapter must be exercised through the actual
SDK/CLI registration, not only as an isolated mapper.

Use synthetic or deliberately sanitised fixtures. **Do not commit private
prompts, credentials, tool payloads or artifacts** without explicit sanitisation
and provenance review. Source paths and identifiers may themselves be sensitive.
Retain structural evidence and native source citations without exporting private
stores. Do not mutate process-global environment in parallel tests; supply context
or use isolated child environments.

## Contribution workflow and done criteria

Follow the existing [repository/agent rules](AGENTS.md) for isolated Builder
workspaces, shared-file ownership, governance and attribution. Human-facing
contributions should stay focused; don't copy internal planning ceremonies into
public usage examples or create a second policy here.

- Coordinate ownership before touching shared contracts, registries or lockfiles.
- Update affected docs, descriptors and capability claims together.
- Prove the changed behavior with actual commands or an executed example; retain
  failures and their fixes rather than replacing them with an unsupported pass.
- Use `harness commit "message" -- <owned paths>` when committing in this repo;
  read whether attribution landed or was explicitly deferred.
- Include scope, tested versions/platforms, evidence and remaining limitations in
  the PR. Let CI run its existing gates; never suppress failures to obtain green.
- Preserve existing user work, licences and original provenance. Do not force-push
  or retire another workspace as incidental cleanup.

See [development proof](docs/development.md) for the maintained command details
and [fidelity.md](docs/fidelity.md) for how coverage limits are classified.

## Report a bug safely

Include the Unisphere version, adapter ID, source representation, platform, exact
command, expected/observed behavior and safe diagnostics. Prefer a minimal
synthetic reproduction or field/type projection over a session dump. For usage
or ordering bugs, state the native field's scope and the evidence for its meaning.
Do not paste credentials, private prompts, output artifacts or identifying paths
into an issue merely to make it reproducible.
