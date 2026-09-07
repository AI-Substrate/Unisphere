# `harness checks` — agent briefing

## What this verb computes

Run `harness checks --json` for the foundation quality gate. It first records actual rustc/Cargo/clippy/rustfmt release and emitted commit output plus executable provenance, then compares the approved tuple documented in `docs/development.md`. Missing tools, failed probes or mismatched identities yield `E_TOOLCHAIN_PREREQUISITE` before any quality gate. A matching formatter release with no emitted commit hash is accepted with an explicit provenance warning and optional stronger-observation command; its commit and commit_matches stay null, never fabricated as a match. Equivalent installation paths/distributions are accepted by observed identity.

After successful tool observation, it runs Cargo formatting, clippy with denied warnings, workspace behavioral tests, rustdoc examples, the declaration-based architecture checker, and harness wrapper regressions. Each gate is bounded. Failed children preserve command, arguments, exit code, stdout and stderr; later gates do not run after a failure.

## Your role

Select an installed coherent Rust 1.95.0 toolchain per command; do not mutate global defaults or assume `rust-toolchain.toml` overrides PATH. Read toolchain evidence and remediation before diagnosing product code. Do not claim authored tests passed without executing this lane against the applicable source. During Builder fan-out the PM owns validation/formatting, not concurrent coders.

## Proof boundary

This gate proves only the commands it reports. It does not establish installed SDK/CLI behavior: `harness boot` adds real assembled proof. No gate claims telemetry readers, collection, database readiness, a syscall-level network-denial test, or actual TTY detection. Core/SDK source-surface review remains separately required for no-network/ambient-effect claims. Node and the ambient harness CLI are development tools, not product runtime dependencies.
