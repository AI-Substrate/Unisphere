# Unisphere

Common-format telemetry collector for agent harnesses, delivered as a Rust SDK and a thin CLI. Product implementation has not begun; no canonical application build/test/run lane exists. Main-branch system setup is operator-authorized; product work belongs in isolated Builder plan workspaces. Coordinate shared-file ownership with active peers.

## Governance — new primes start here

Governance is the standing **orphan `prime-governance` branch**, not `main` and not a product-plan branch. Its permanent worktree on this machine is `/Users/jordanknight/substrate/unisphere/unisphere-governance`; the government root is `.harness/government/` inside that worktree. Discover the actual path with `git worktree list --porcelain` by matching `branch refs/heads/prime-governance` rather than deriving it from the current plan directory.

1. Read that worktree's `AGENTS.md`, then `.harness/government/orient-local.md`, `spine.md`, and `baton-book.md` by absolute path; inspect the portfolio through `harness flow show --path <absolute-government-root>/prime-flow.json`.
2. Inspect `pij list --prime --json` and reconcile the repository identity with the spine and the operator; a missing local directory or a narrow peer listing does not prove that no prime exists. This seed appoints no prime, and no agent may designate itself over another seat.
3. If the branch exists but has no worktree, restore it into a new explicit directory with `git worktree add --lock --reason 'Permanent Unisphere governance' <path> prime-governance`; inspect local/remote refs first and never force, reset, or overwrite a conflicting checkout.
4. Only if governance is genuinely absent and the operator authorizes bootstrap, follow `/pij prime` and its `references/prime/rituals/bootstrap.md` sections 2–4: create an orphan `prime-governance` branch in a permanent worktree, seed the per-repo orientation, spine, baton book and CLI-owned portfolio, then record the designated writer. Use `git worktree add --orphan -b prime-governance --lock --reason 'Permanent Unisphere governance' <new-path>` on Git versions supporting it; never use `harness builder new` to create government.

The designated prime is the only government writer; PMs and coders read it. Never merge governance into product branches, put code there, or retire its locked worktree with a plan. Commits still use `harness commit`; a standing governance branch is not authorization to push.

Keep product plans, guides, tasks, execution/review evidence and code together on their Builder plan branches, then land them through the approved PR workflow. Keep engineering-harness extensions, onboarding reports, retrospectives and local skills with the product repository: do not move all of `.harness/` into government. At each plan closeout the prime reconciles the portfolio/rulings with exact delivery evidence; branch isolation alone does not keep government current.

## Builder and code composition

Use `/builder` for research → product plan → implementation guide → tasks → implementation/review → closeout/ship. Use the installed `harness builder` commands for managed plan allocation, readiness, dispatch and evidence; do not substitute the legacy `harness team` grammar. Read the live command help and capability checks before allocation. Repo-local `node_modules/.bin/ddocs` is the document-authoring tool, not a Rust runtime dependency.

The architectural direction is **hexagonal architecture (ports and adapters)** with a **functional core / imperative shell**, constructor injection and an explicit composition root: CLI → SDK/application services → core contracts; concrete source adapters implement those contracts and depend inward. No dependency from core/services back to CLI or concrete adapters, no hidden service locator, and no mandatory daemon, HTTP server or ML stack. The implementation guide owns the final crate layout, service interfaces, dependency checks and shared adapter-contract tests; these directions are not an implemented-code claim.

Builder can create plan worktrees, but the inspected installed dispatcher currently supports OMP coders only and refuses linked-worktree coder allocations. Recheck the installed capability before dispatch and select clones explicitly when required; never silently change the requested isolation or review model.

<!-- BEGIN harness:onboarding -->
## Engineering harness

Read `.harness/engineering-harness.md` before non-trivial work. At session start:

1. `harness --version` — the CLI is an ambient global tool, not a repository dependency. If missing: `npm install -g @ai-substrate/engineering-harness` (Node >=22).
2. `harness instructions` — read the agent briefing; `harness help --json` discovers the command map.
3. `harness doctor --json` — inspect extension loading, convention complaints, and machine-attribution warnings separately.
4. `harness instructions boot` then `harness boot --json` — attempt readiness before changing product code.

`checks` and `boot` currently return **unconfigured (exit 2)**. This is missing product proof, not success. Do not claim the collector runs or add meaningless checks to make onboarding green. Implement the canonical product lane first, then wire it into these extensions.

Project-local pi skills live under `.pi/skills/`. `/eng-harness-flow` is the front door: use `--hook pre-flight` at work start, `--hook pre-coding` once scope is agreed, `--hook post-coding` at a work-unit handoff, and `--hook post-flight` at task closeout. If skills are not loaded, read `.pi/skills/eng-harness-flow/SKILL.md` and follow it inline. The router remains on adoption until real product readiness is available.

Capture friction immediately:
`harness observe "<what happened>" --kind difficulty --severity degrading --agent <session-slug>`.
At closeout, read `harness observe --list --agent <session-slug> --json`, create a durable record with `harness record retro`, fill the returned path, then clear **only your bucket** with `harness observe --clear --agent <session-slug>`. Never clear another peer's observations. Offer one concrete command, fixture, or sensor to encode the highest-value lesson.

Commit durable `.harness/` substrate, reports, records, and local skills. `.harness/temp/` is scratch; never commit its contents except the protective `.gitignore`. Preserve global Git trace2 configuration and collector metadata during repo setup.
<!-- END harness:onboarding -->

<!-- BEGIN harness:commit-guidance -->
## Committing in this repo

Use `harness commit "<message>" -- <paths>` rather than a chained
`git add … && git commit …`.

A `harness commit` is **verified or named**: it probes the collector ingress,
commits, and then tells you WHICH outcome you got. It never blocks and never
rolls back. The outcomes are:

- **confirmed** — when the collector ingress socket is reachable: harness commits with no trace2 override, waits (bounded) for the `refs/notes/ai` note, and tells you whether it landed. A landed note is the healthy shape, and a miss is reported to you rather than hidden — with the next step named in the command's own output. Nothing was buffered on this path, so there is nothing to drain.
- **buffered and named** — when git's configured trace2 target is a plain FILE, or when the ingress is blocked, absent or unconfigured: the commit is made with its trace2 events going to a buffer file instead of the collector, so attribution is DEFERRED, not lost — and it isn't proven yet either. `harness commit` names the buffer it used; when the configured target is a plain FILE it must be pointed back at the socket first, because while it names a file there is no ingress to replay into. Drain it with `harness doctor telemetry-nudge` from an UNSANDBOXED shell. Recovery is POSIX-ONLY: the drain replays into an af_unix socket, so on a Windows host `harness doctor telemetry-nudge` refuses on platform grounds and drains nothing — the buffered events stay on disk, untouched, until they are drained from a host whose collector ingress is an af_unix socket.
- **NOT VERIFIED on this platform** — when trace2 points at a Windows NAMED PIPE (\\.\pipe\…): the commit is made with no trace2 override (git talks to the pipe as usual), nothing was buffered, nothing was written beside the pipe — and nothing is claimed about attribution, because nothing was measured. Check for yourself with `git notes --ref=ai show HEAD`. Do NOT run `harness doctor telemetry-nudge` — there is no buffer to drain and no replay path for the named-pipe transport, and it will refuse.

A chained or compound `git commit` can **silently lose attribution** — agent
command sandboxes block git-ai's socket, git quietly disables trace2, and the
commit's authorship may later be recorded as human.

Neither shape guarantees delivery. What `harness commit` guarantees is that the
outcome is never silent. Read `harness instructions commit` for the detail.
<!-- END harness:commit-guidance -->
