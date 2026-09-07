---
record_kind: "retro"
harness_version: "0.14.0"
branch: "main"
repo: "https://github.com/AI-Substrate/Unisphere.git"
created_at: "2026-09-07T02:32:19.978Z"
agent: "pij-female-varl"
plan_id: "001-sdk-cli-foundation"
schema_version: "1.2"
retro_id: "2026-09-07T02:34:24Z-pij-female-varl-f2e1082706d7"
started_at: "2026-09-07T02:18:26.156Z"
ended_at: "2026-09-07T02:34:24Z"
summary: "Seeded governance and inherited agent routing, dogfooded first-class Builder worktree allocation, and preserved requirements-only scope. Local DD tooling was unavailable from configured/public registries; used explicit ignored links to the harness-bundled CLI. Reported populated example-guide defect to the operator-designated harness prime, who confirmed backlog row 37."
entries:
  - id: DL-001
    kind: difficulty
    description: "Repo-local ddocs was missing; npm proxy returned 404 and direct registry failed ENOTCONN."
    target: tooling
    severity: degrading
    workaround: "Linked the installed harness's bundled @ai-substrate/dd 0.1.0 into ignored node_modules in main and the allocated worktree; verified --help and actual ddocs get/validate."
    suggested_encoding: "Provide an explicit first-class Builder document-tooling bootstrap/preflight that distinguishes bundled internal writer from repo-local authoring tooling."
    fp: "f2e1082706d7"
    disposition: kept
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-07T02:18:26.156Z"
  - id: INS-001
    kind: insight
    description: "Builder new copied a populated converter teaching guide into a requirements-only workspace; ready correctly refused dispatch against the empty product contract."
    target: tooling
    disposition: kept
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-07T02:25:07.248Z"
---

# Retro — Governance and requirements bootstrap

- Main governance-routing commit: `aa5e75cad1d8397a7ec392e2cb7773c00f673a93`; orphan governance seed: `2cd257166b786b3308447459ff3be3cce06e2849`. Both harness commit calls reported notes landed; this is attribution-delivery evidence, not proof of every attributed line.
- First-class Builder retry with absolute workspace succeeded: `/Users/jordanknight/substrate/unisphere/unisphere-sdk-cli-foundation`, branch `builder/001-sdk-cli-foundation`, allocation `al-001-4437fdce-9332-4eed-9398-7651d0ab20e2`.
- Cold-reader proof discovered the locked governance root from AGENTS, read its required files and portfolio, and confirmed product flow at research. `plan.dd.json#meta` remained draft. `builder ready` returned E471/not-ready, not a false green.
- `pij-varied-alpaca` confirmed the teaching-guide seed is a real defect (backlog row 37) and advised leaving it unapproved. Requirements are captured in `requirements-spine.md`; no Rust code, approved plan or coder dispatch was created.
- Legacy `pij state --json` refused its schema projection; native `pij-rs state --json` works and declares unsupported fields. Harness prime classified this as an intentional wrapper boundary, not a Builder defect.
- Perplexity research MCP timeout and reduced-context fallback are recorded with the standards research; direct long-timeout research and the Flowspace3 consumer interview are tracked separately, not claimed complete by this setup retrospective.
- Existing `.serena` files and all other peers' observation buckets were left untouched. No pushes, main/governance merges, global trace2 changes or real producer-store mutations occurred.
