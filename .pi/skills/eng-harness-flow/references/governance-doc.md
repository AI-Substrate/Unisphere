# The engineering-harness governance doc — contents & write conditions

This is the **one canonical description** of the engineering-harness governance doc: where it lives, what it contains, the `harness-change` record ledger, and *when* each is written. The verb modules that touch the doc (`boot`, `adopt`, `backpressure`, `retro`) and the `eng-harness-0-harnessability-assessment` peer **link here** rather than restating these rules, so there is a single source of truth.

> **Separation of concerns (the load-bearing idea):**
> - **governance doc = contract** — what the harness *is*, plus the *current* maturity snapshot.
> - **`harness-change` records = changelog** — what *improved*, sparse (one record per encoded improvement).
> - **boot = runtime read** — it *reports*, it never *writes*.

---

## Where it lives (G1 · path + read order)

The canonical governance doc is **`.harness/engineering-harness.md`** — kept alongside the other harness artifacts in `.harness/` (which already holds `extensions/`, is tracked, and is not gitignored).

Every reader reads **exactly one** location: `.harness/engineering-harness.md`. The legacy `docs/project-rules/*` fallback chain is retired (plan 014) — a repo whose doc lives anywhere else should `git mv` it to the canonical path.

---

## What the governance doc contains (the BIO contract)

The doc is a thin **Boot / Interact / Observe contract** plus a signal inventory and the current maturity snapshot. It records:

| Section | What it holds |
|---|---|
| **AGENTS START HERE breadcrumb** | the doc's opening line points agents at the CLI's self-briefing channel: `AGENTS START HERE → npx harness instructions` (then `harness instructions <verb>` per verb). The briefing system is the *live* role contract; the governance doc carries the pointer so a zero-context reader finds it in one hop (plan 014 AC-12). |
| **Boot command** | the exact command that boots the system to a healthy, observable state (`<60s` target); the engineering-harness substrate the agent-facing loop runs on. **Composes `harness checks`** once services are ready. |
| **Checks command** | the mandated quality gate — lint, unit tests, typecheck, and similar deterministic correctness checks (`harness checks`). Agents run it before considering work *done*; teams gate commits/push on it; `boot` composes it. Extensible: gates are added here as the team grows. |
| **Health check** | the command/endpoint that proves the system is up (read by boot's Stage 1). |
| **Interact method** | how an agent sends input to the running system (boot's Stage 2). |
| **Observe method** | how an agent captures evidence — logs, screenshots, traces, snapshots (boot's Stage 3). |
| **Deterministic signal inventory** | the sensors that let a human/agent prove behaviour without inference: runtime inspectability, smoke paths, architecture/static checks, security/dependency/schema checks. |
| **Evidence paths** | where artifacts land (log/trace/screenshot/output locations) so they're discoverable. |
| **Injection map** | where the repo's *extant* dev/SDD flow calls `/eng-harness-flow` — one row per lifecycle hook (`--hook pre-flight`, `pre-coding`, `coding`, `post-coding`, `post-flight`; the six `--event` seams — `session-start`/`pre-implement`→`pre-flight`, `post-spec`→`pre-coding`, `task-pause`→`coding`, `phase-end`→`post-coding`, `plan-complete`→`post-flight` — are the permanent alias): where it fires from and what fires it. The host flow is swappable (`the-flow`, another SDD pipeline, plain PR work); the hook vocabulary is the constant. This section is the durable signal behind the router's S3 rung — without it a cold agent has no structural reason to call the harness. |
| **Back-pressure gaps** | behaviours that still rely on inference or human eyeballing — named honestly as improvement candidates, never as scores. |
| **Current maturity snapshot** | the **single, current** L0–L4 level the harness is *actually* at (see [`maturity-assessment.md`](./maturity-assessment.md)). The doc holds only the *current* snapshot — the trajectory lives in the `harness-change` record ledger. |

The doc is a **contract, not a log**: it says what the harness *is right now*, not what happened each session.

---

## G3 · The change ledger = `harness-change` records (a changelog, not a log)

The harness changelog is the **`harness-change` record ledger** (`.harness/records/harness-change/`, written via `harness record harness-change`): **one record per improvement *encoded into the harness*** — the *Improve* beat of the loop. It is explicitly **not** a per-session or per-boot log.

- A record is written **only** when a retro / magic-wand actually *ships a harness change*: a new command, a new sensor, a faster boot, a maturity-level move.
- **Most loop runs add zero records.** Booting, observing, even draining a retro buffer do **not** write a `harness-change` record unless an improvement is encoded.
- It is the **trajectory**; the governance doc holds only the *current* snapshot. A maturity level that climbs (or a boot time that shrinks) across records is the compounding value made visible.

Consumers (e.g. the `retro` verb `--harvest`) read the **current** maturity snapshot from `.harness/engineering-harness.md` and treat the `harness-change` record ledger as the changelog; any field with no live source (last validation, boot ms, verdict) is reported `null` rather than fabricated.

---

## G4 · Boot is read-only

The `boot` verb only **reads** maturity from the governance doc — it does **not** write governance or history. There is no per-validate `## History` append and no per-session write anywhere. Boot *reports* the level that is actually working; it never edits the contract.

---

## G5 · Write conditions — who writes the doc, and when

| Event | What changes | Who |
|---|---|---|
| **Inception** (once) | the doc is *created* with the BIO headings, the signal inventory skeleton, evidence paths, and the seed maturity snapshot | **the `harness init` writer** (a CLI command — shipped, FX001). `npx harness init` stamps the skeleton and seeds maturity **L0**, leaving every other BIO field a `TODO`/empty placeholder (the Injection map an empty table); it is **idempotent + never-clobber**. (Setup *drives* setup by calling it; it does not itself generate the governance doc.) |
| **Improve beat** (on a capability change) | the **body** (boot cmd / signals / evidence paths / back-pressure gaps) **and** the **current maturity snapshot** are edited to match new reality; a `harness-change` record is written (`harness record harness-change`) | the Improve beat — when the harness gains or changes a capability |
| **Inject decision** (adoption S3, or when the host flow changes) | the `## Injection map` section is added/updated in an *existing* doc — never created standalone (`harness init` stamps an empty Injection map table; S3 adds its rows) | the `adopt` verb Step 3, with the user's go-ahead |
| **Every other loop run** | **nothing** — boot reads, observe writes its buffer, retro writes `.retro.md`; the governance doc is untouched | — |

The doc is therefore written at **inception once**, and its body + snapshot change **only at the Improve beat**. It is never rewritten just to record that a session happened.

> **Seeded, not populated (honesty about the skeleton).** `harness init` *creates* the doc, but seeds it empty — maturity **L0**, every other BIO field a `TODO`. So governance *exists* right after install, yet boot must report from what the doc actually *holds*, not from its mere presence: an unpopulated doc has no boot command, so boot reports `UNAVAILABLE` until the body is filled at the **Improve beat** (G5). If the doc is genuinely absent (e.g. `init` was never run, or a CLI old enough to predate `init`), readers degrade to `UNAVAILABLE` rather than erroring.
