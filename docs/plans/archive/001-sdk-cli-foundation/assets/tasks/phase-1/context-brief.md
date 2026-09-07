# Phase 1 implementation context

Product intent: `../../../plan.dd.json`. Architecture, exact APIs, write/read fences and argv checks: current `../../impl-guide.dd.json`. Selected proof: `../../backpressure.dd.json`. Authorization: `../../implementation-authorization.json`. Task/assertion state lives only in `tasks.dd.json`.

## Construction order

- PM `tk-0001`: real core DTOs/ports/safe errors, reusable testkit fakes/config fixtures/sealed child helper, root workspace policy. No SDK/CLI stubs. Independent source-bound review and Builder seal before dispatch.
- Baseline-only wave: `tk-0002` SDK/configuration service, `tk-0003` CLI frontend, `tk-0005` proof/development tooling. Separate full clones; no sibling implementation reads/imports. Exact requested runtime: OMP `github-copilot/gpt-6-astra`, high.
- PM `tk-0004`: Builder imports, real app composition root, root usage/notices, generated lockfile and assembled proof. Independent OMP `github-copilot/claude-opus-5`, high review targets the exact verified artifact.

## Proof and boundaries

Every assertion names its pressure row and guide check. Coders author behavioral tests but skip validation/formatters; PM executes isolated lane checks and final gates. Baseline-only tests never imply external consumer, CLI installation or composed acceptance. Shared signatures are guide-owned; changes require reviewed reconciliation before dependent release.

`Cargo.lock` has one owner: composition PM, explicitly authorized to generate it during baseline preparation; it is not a frozen baseline file or coder delivery. Probe this exception when sealing; report tool disagreement to `pij-varied-alpaca`, never bypass fences.

Rust 1.95.0/edition 2024 is requested. Record actual compiler/Cargo/clippy/rustfmt provenance and select an already-installed coherent tuple per command; do not change global tooling. An inert toolchain file cannot prove quality readiness.

All product inputs are explicit. Configuration roots remain uninterpreted strings. Fakes and temporary subprocesses replace private stores; no process-global environment mutation in tests. Product runtime has no Node/DD/Builder requirement. `bp-0008` additionally requires independent no-network/ambient/process/FFI/unsafe source inspection; dependency checks and hostile-environment smoke are not syscall-denial proof.

Foundation only: native readers, telemetry schema/profile and scratch experiments do not ship. No push, PR, main merge, teardown, global mutation or real private telemetry access. Preserve exact failing command/cwd/exit/output and report Builder experience. Before any release require current readiness, sealed source, native packet/canary ack, and exact release observation/confirmation.
