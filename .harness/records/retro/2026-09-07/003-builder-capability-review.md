---
record_kind: "retro"
harness_version: "0.14.0"
branch: "main"
repo: "https://github.com/AI-Substrate/Unisphere.git"
created_at: "2026-09-07T02:03:10.780Z"
agent: "pij-female-varl"
plan_id: null
schema_version: "1.2"
retro_id: "2026-09-07T02:03:24Z-pij-female-varl-89bea3e2e4d3"
started_at: "2026-09-07T02:02:59.242Z"
ended_at: "2026-09-07T02:03:24Z"
summary: "Inspected Builder skill contracts, installed CLI help and allocation/dispatch source without starting a plan; found a missing local DD authoring prerequisite and distinguished implemented worktree allocation from the installed clone-only OMP coder dispatch restriction."
entries:
  - id: DL-001
    kind: difficulty
    description: "Unisphere has no node_modules/.bin/ddocs: its --help probe exits 127 although harness builder commands are installed."
    target: tooling
    severity: degrading
    workaround: "Completed the capability report through installed harness help and source inspection; did not claim readiness or install tooling."
    suggested_encoding: "A Builder preflight should distinguish its bundled internal DD writer from the repo-local authoring CLI and name the missing installation before allocation."
    fp: "89bea3e2e4d3"
    disposition: kept
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-07T02:02:59.242Z"
---

# Retro — Builder capability review

Evidence: `harness builder --help`, `new --help`, `dispatch --help`, `guide --help`, `advance --help`, `harness flow --help`, and `harness docs harness-builder` succeeded; `node_modules/.bin/ddocs --help` exited 127.

Installed source: `services/builder/workspace-service.ts:238-250` implements Git worktree/clone allocation; `services/builder/dispatch-service.ts:913-940` restricts dispatch to OMP coders and refuses linked worktrees. These are source-inspected capabilities and restrictions, not an executed allocation or peer-launch smoke test.

No product code, plans, worktrees or pij peers were created by this investigation; one read-only scout assisted source inspection, and no commit or installation was performed.
