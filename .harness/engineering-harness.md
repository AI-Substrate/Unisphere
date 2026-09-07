# Engineering harness

> **AGENTS START HERE → `harness instructions`** — the CLI's baked agent
> briefing (envelope contract, role split, discovery loop). Then
> `harness instructions <verb>` per verb.

## Boot command
`harness boot --json` composes `harness checks --json` with a 120-second deadline. It starts no services. Product readiness is **unconfigured (exit 2)**: this repository has not implemented the telemetry collector or its canonical run/smoke lane. A successful checks process alone still yields `degraded` with `ready: false` until real readiness is wired.

## Checks command
`harness checks --json` is the quality-gate entry point. No product build, lint, or test command exists yet; it returns **unconfigured (exit 2)** with an explicit next action. When implementation establishes that command, wrap it here once; boot composes the gate rather than duplicating it.

## Health check
No product health endpoint or runnable collector exists. `harness doctor --json` checks the harness itself; loaded extensions are not product health.

## Interact method
Harness: terminal CLI with JSON envelopes and `--help`. Product: no supported telemetry ingestion interaction exists yet. Do not use real external telemetry or credentials to manufacture a readiness claim.

## Observe method
Read CLI `status`, `data`, `error`, `next_action`, and exit code. Capture friction with `harness observe "<what happened>" --kind difficulty --severity degrading --agent <session-slug>`. Read `harness observe --help` for core capture options; core verbs do not all have per-verb instruction pages.

## Deterministic signal inventory
| Signal | Command | Proof boundary |
|---|---|---|
| Extension loading and conventions | `harness doctor --json` | Harness configuration, not the product |
| Product quality gate | `harness checks --json` | Unconfigured; no tests/build run |
| Product readiness | `harness boot --json` | Unconfigured; no service starts |
| Capture and retrospective inventory | `harness observe --list --json`; `harness retro insights --json` | Recorded process evidence, not telemetry collector behavior |
| Harness composition regression | `node --test .harness/extensions/boot/extension.test.mjs` | Isolated missing/unconfigured/failure/success-warning verdicts; never product proof |

## Evidence paths
- `.harness/reports/harnessability/latest.json` and `latest.md`: assessment and proof gaps.
- `.harness/records/retro/`: committed onboarding and session retrospectives.
- `.harness/records/harness-change/`: encoded harness changes, not per-boot logs.
- `.harness/temp/`: gitignored session scratch; keep its nested `.gitignore` tracked, never commit buffers or collector metadata.
- `.harness/adopt.flow.json`: CLI-owned adoption position. The product-readiness bridge remains blocked until the real lane exists.

## Injection map
Plain main-branch system setup; no implemented application flow or CI pipeline exists yet. `AGENTS.md` carries the lifecycle cues; project-local pi skills are under `.pi/skills/`.

| Hook | Fires from | What fires it |
|---|---|---|
| `pre-flight` | `AGENTS.md`, session start | `/eng-harness-flow --hook pre-flight`; re-entry retains adoption until product proof exists |
| `pre-coding` | `AGENTS.md`, agreed scope before implementation | `/eng-harness-flow --hook pre-coding` |
| `coding` | `AGENTS.md`, friction during work | `harness observe "<what happened>" --kind difficulty --agent <session-slug>` |
| `post-coding` | `AGENTS.md`, work-unit handoff | `/eng-harness-flow --hook post-coding`; drain only the caller's buffer |
| `post-flight` | `AGENTS.md`, complete task closeout | `/eng-harness-flow --hook post-flight`; offer one concrete encoding |

## Back-pressure gaps
- Product source, canonical validation, startup/health, and fixture-backed telemetry input-to-output proof are absent. The first product work must establish these before a green readiness verdict is possible.
- Runtime behavior, schema normalization, malformed-input handling, persistence, and external-effect isolation cannot yet be proved.
- Machine attribution may report `cli-only-trace2` and capture-liveness unavailable. Do not remove global Git trace2 configuration during repo onboarding; those diagnostics are distinct from extension conventions and product readiness.

## Current maturity snapshot
**L0 — product boot and interaction are unavailable.** The repo-local harness provides discoverable commands, explicit missing-proof verdicts, skills, and a durable improvement loop; these do not raise product runtime maturity.
