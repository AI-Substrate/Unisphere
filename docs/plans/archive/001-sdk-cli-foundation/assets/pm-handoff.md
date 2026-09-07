# Plan001 implementation-guide handoff

**PM:** `pij-right-kotallo` (operator-designated).
**Scope:** Reviewed guide handoff, not coder implementation release.
**Workspace:** `/Users/jordanknight/substrate/unisphere/unisphere-sdk-cli-foundation`.
**Branch:** `builder/001-sdk-cli-foundation`.

## Approved inputs

- Product plan: `plan.dd.json`, SHA-256 `bc208a54522fe9d5d26c87d25270806ad72b6b0a53d9155ae4cff1b2871fe3cc`.
- Guide v3: `assets/impl-guide.dd.json`, SHA-256 `95da8bf6c3d65077ae18085a379c9c4f780571a7f2ef62d5ac1a65644ae6cdb0`.
- Reviewed committed subject: `d325a8f60a3c97739b7c3b16e612110b5db84530`.
- Independent reviewer: `pij-angry-hekarro`, native OMP `github-copilot/claude-opus-5`, high; native-file/root/argv/registry canary observed, no provider-served identity attestation.
- Approved review: `assets/reviews/decomposition-opus-review-r2.md`, SHA-256 `e7d3f37fec13225a35529dbd81421895059c64222c352d1c304e457062524a1b`.
- Accepted canonical receipt: `assets/team/review-decomposition-review-decomposition-d325a8f-pij-angry-hekarro-r2.dd.json`, SHA-256 `5549935811d2ba56244a7e285d4bb440b496cd0fb983cf94d12008a6a9ac98fc`.

## Construction and runtime

PM baseline `tk-0001` precedes three independent wave-1 coder units: `tk-0002` SDK/configuration service, `tk-0003` CLI frontend and `tk-0005` proof/development tooling. PM `tk-0004` composes their verified deliveries into the real app. All coder lanes depend only on the baseline, not a completed sibling or CLI. The five crate responsibilities and exact paths/interfaces/checks are in the guide, not redefined here.

Coders use **OMP `github-copilot/gpt-6-astra`, high**; reviewers use **OMP `github-copilot/claude-opus-5`, high**. These exact selectors and high effort were listed by the installed model catalog and resolved by `harness builder settings`. Recheck native capability at launch; use explicit full clones for OMP coders because linked-worktree roots are refused. Fresh clone document tooling is a prerequisite, not a product runtime dependency.

## Current readiness and next gates

Main observed guide structural validity (zero issues), clean DD validation/view rendering, and accepted the genuine r2 review through `harness builder review`. `harness builder ready` still returns **E471/not-ready**, identifying missing baseline `Cargo.toml`; no product source or baseline receipt was fabricated. The empty phase-task scaffold still needs real task authoring.

Resume the canonical Builder flow in the plan workspace, reconcile the existing tracker/chore receipts through supported commands, author tasks from this guide, implement and commit the PM baseline, obtain fresh source-bound decomposition evidence as required, then seal and check each unit's readiness. Native Builder dispatch/ack/release confirmation and compose/import/verify/review are mandatory dogfood paths; do not manually bypass them. This handoff is not permission to mark future work done or release coders before those gates.

Use explicit absolute file paths and command cwd when the PM's native session remains rooted in the main clone; changing shell cwd does not rebind native file tools. Coder/reviewer launch root and packet canaries must report their actual root, never an assumed plan path.

## Two accepted observations to carry forward

1. **O1 — lockfile ownership:** probe the explicit composition-owned generated `Cargo.lock` authorization during baseline sealing; if the runtime checker disagrees with the reviewed guide, report it to `pij-varied-alpaca`, not a fence bypass.
2. **O2 — no-network proof:** explicitly include `bp-0008`'s independent source-surface inspection in the composition reviewer's packet; PM accountability does not make the PM an independent reviewer.

The guide also records the host's mixed Rust/compiler-component observation. `rust-toolchain.toml` alone is not enforcement; establish and record a coherent approved toolchain before claiming full product gates, without automatic global changes.

Report all Builder workflow experience to **`pij-varied-alpaca`** with actual command/cwd/exit/evidence. Preserve scratch reader experiments before any workspace retirement; they remain outside Plan001 shipping scope. No push, PR, merge, teardown or machine-global mutation is authorized by this handoff.
