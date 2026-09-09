---
record_kind: "harness-change"
harness_version: "0.14.0"
branch: "main"
repo: "https://github.com/AI-Substrate/Unisphere.git"
created_at: "2026-09-07T00:34:48.948Z"
agent: "pij-right-kotallo"
plan_id: null
schema_version: "1.0"
resolves: ".harness/records/retro/2026-09-07/001-harness-onboarding.md"
change_type: "new-command"
target: "Repo-local checks/boot entry points, governance, agent routing, and project-local harness skills"
---

# Initial harness adoption

Established the canonical `.harness/` operating surface without adding a dependency to the product or inventing a product implementation. `checks` names the missing canonical quality lane; `boot` composes checks and cannot claim readiness from absent or merely passing checks.

Installed exactly `eng-harness-flow` and `eng-harness-0-harnessability-assessment` for project-local pi, sourced from `AI-Substrate/harness-engineering`. To reproduce the intended scope: `harness skills install --target pi --source AI-Substrate/harness-engineering --skill eng-harness-flow eng-harness-0-harnessability-assessment`.

Added `AGENTS.md` startup/lifecycle routing and CLI-owned commit guidance. Observation scratch self-protects through its nested `.gitignore`; retrospective draining is scoped to the calling session.

Fixed the generated scratch ignore rule to keep `.harness/temp/.gitignore` itself trackable (`!.gitignore`) while ignoring buffers and collector metadata. This lets fresh clones inherit the protection without force-staging runtime scratch.

## Evidence

- `initial-adoption-evidence.json`: actual doctor, checks, boot, and help envelopes and process exit codes.
- Doctor: 2 loaded extensions, 0 failures/conflicts, no convention complaints. Overall degraded only on machine capture/collector warnings.
- Checks and boot: unconfigured, exit 2. No product test/build/service was run.
- `node --test .harness/extensions/boot/extension.test.mjs`: 5 passing harness-only contract tests.
- `.harness/reports/harnessability/latest.json`: independently authored pre-product assessment, separate from harness plumbing health.

Product maturity remains L0. The adoption flight plan retains a blocked product-readiness bridge. Global trace2 settings, existing collector scratch, and unrelated `.serena/` files are untouched.
