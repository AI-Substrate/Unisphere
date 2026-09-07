# Unisphere

Common-format telemetry collector for agent harnesses. Product implementation has not begun; no canonical application build/test/run lane exists. Work on `main` during system setup, as directed by the operator, and coordinate shared-file ownership with active peers.

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
