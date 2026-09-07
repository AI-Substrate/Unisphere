---
name: eng-harness-flow
description: |
  Front door for the engineering-harness loop. Use it at work seams so agents
  pursue two goals at once: complete the requested task, AND improve the
  deterministic engineering environment future agents and teammates inherit.
  Before work: boot and check backpressure. During work: capture friction and
  missing proof. After work: drain/harvest observations and cross-plan insights. At closeout: offer to
  encode the best lesson as a command, check, fixture, sensor, diagnostic,
  template, default, or clearer error. Advisory means the user is never blocked —
  it does NOT mean the agent may silently skip the loop. Mechanically a stateless
  router: it re-derives where work sits from repo signals and routes to the one
  correct harness skill (never call children directly); honours the adoption gate
  and `--event`/`--json`/`--hooks`; never gates, scores, or blocks.
---
# eng-harness-flow

## Why this skill exists

Agents optimise for the requested task — fix the bug, add the feature, reach "done". That is necessary but incomplete. This skill gives the agent a **standing second objective**: while doing the work, improve the engineering environment that future agents, humans, and teammates will use.

**Two jobs, every session:**
- **Primary** — complete the requested work.
- **Standing secondary** — reduce future inference cost and increase deterministic proof. When the repo makes you guess, retry, hand-check, remember tribal setup, read logs to find a verdict, or infer whether "done" is really done, that is harness feedback. Capture it; at closeout, offer one concrete way to move the repeated part out of tokens and into deterministic substrate — a command, check, fixture, sensor, diagnostic, template, default, or clearer error.

Code review checks the **product**. The harness loop reviews the **process that produced** the code.

**Required reading — the concept, once.** Before the first beat, read the complete engineering-harness reference: `harness docs the-harness-distilled` (bundled offline in any repo with the CLI; source `harness-foundations/the-harness-distilled.md`). It is the single-hit orientation — what a harness is, the four-layer stack, backpressure, encode-don't-document, and the operating loop this router drives. Everything below assumes it.

**Token and model posture.** Spend tokens where they can change the outcome; delegate bounded chores to cheaper tiers, analysis/review to capable tiers, and keep judgement with the lead unless context overrides. See builder `references/00-routing.md` § Model-to-task fit & delegation and `harness-foundations/rules-of-why.md` Rules 5–6.

**The loop — four beats:**
1. **Before work** — prove the environment and check backpressure (`--hook pre-flight`, then `--hook pre-coding`).
2. **During work** — capture friction the moment it bites: `harness observe "<what was hard or unproven>" --kind <kind>`. Fire on: a retry or backtrack · a >30s wait on a tool · a search that returned nothing where you expected matches · a failure you had to *guess* to interpret · runtime behaviour you could only *infer* (no smoke path / log / trace / health evidence) · an architecture, dependency, security, or schema constraint you *eyeballed* that a check could have proved · hidden or tribal setup · catching yourself thinking "if only there were a…".
3. **After a phase/session** — drain the observations into a retro (`--hook post-coding`).
4. **At closeout** — harvest, name the single highest-leverage improvement in plain language, and offer to encode it (`--hook post-flight`). **Do not end silently.**

**Meaning of advisory — the user is never blocked; the agent is never silent.** "Advisory" constrains the **user's** freedom (they can decline any step), not the **agent's** duty to *surface* it. Run or offer the harness step at the seam; if the user waves it past, continue. The router itself never gates, scores, or blocks — but silently dropping the boot, the survey, the drain, or the closeout encode offer is the failure this loop exists to prevent.

---

The **single front door** to the harness loop — the harness-loop analogue of `the-flow` (which guides the SDD pipeline). `the-flow` walks a *linear* journey (spec → plan → tasks → code → review → merge); this router routes a *cycle* re-entered wherever the work is: **adopt → boot → backpressure → observe → retro → improve**, drawn in [`references/getting-started.md`](./references/getting-started.md). On every call it re-derives *where on the loop you are* from deterministic repo signals + an optional caller hint, and hands back the **one right harness action** — never call the children directly.

**Progressive disclosure is the contract: load exactly one verb module for the current step — never read all of them up front.**

## The state contract — stateless routing, CLI-driven flow position (read this first)

`eng-harness-flow` is a **stateless dispatcher** for *routing*: `(repo signals, conversation, optional hint) → next harness action`. Its detection — *which* flow is live, *which* rung is missing, *where* the work sits — is **re-derived every call** from deterministic repo signals, never remembered (re-entry after `/compact` needs nothing reloaded). It **never gates, scores, or blocks** (every route is a suggestion; a hint is never a command; declining the harness is conversational, not a `.disabled` file) — but that is the **user's** licence to decline, never the agent's licence to skip the loop silently (§ Why this skill exists → *Meaning of advisory*); **never runs `/compact`** (it can only recommend it); and **never invents a verdict** (`harness doctor` answers whether the harness is healthy, not the router's opinion).

**What it *does* persist — and the only thing it does (plan 032):** the **position of the flow it is driving**, as a **first-class, CLI-driven flight plan**, through the **real `harness flow` verb family** (never hand-edited JSON). This is the deliberate, *scoped* supersession of the old "writes nothing" stance — the dogfood + the exemplar. The scope is **flow position only**:

- ✅ The router drives **two mutually-exclusive flows** as flight plans (🧰 adopt + ⚙️ loop — § below); position lives in `nav`, driven by `harness flow nav`.
- ✅ When the ⚙️ loop runs **alongside an active the-flow**, it injects its four fire-hook steps as **chores** into `the-flow.json` so that flow's rail tracks them (and they stop getting missed); standalone, it authors its own `.harness/loop.flow.json`.
- ❌ Routing/detection/verdicts stay signal-derived; verb modules stay **harness-blind**; the five lifecycle hooks + the `--json`/`--hooks` envelope contract are **frozen**.

> **The unifying rule (unchanged for routing):** routing state that *could* drift lives in deterministic substrate a child verb owns (reports, buffers, `.retro.md`, the governance doc) — **never remembered by the router**. *Flow position* is different: it is persisted **in the flight plan, by the CLI** (the single writer), exactly as the-flow does — observable substrate, not router memory.

The mechanics of driving a flow — nav model, spine-vs-excursion, the verb flags + gotchas, the build-order rule, and the AC-07 chore shape — live in [`references/flight-plan-ops.md`](./references/flight-plan-ops.md) (**load it once, before the first flight-plan mutation of a session; run the capability precheck there first**). The full routing engine — detection signals A–J, the two-zone adoption gate (the Graph), the two-flow selection predicate, the engineering dispatch, the precondition/conflict matrix, and the byte-stable public contract — lives in [`references/00-routing.md`](./references/00-routing.md). The human-mode voice (rail, narration, why-table, tone) lives in [`references/coach.md`](./references/coach.md).

## The two first-class flows (🧰 adopt · ⚙️ loop)

Unlike `the-flow` (one linear journey), `eng-harness-flow` drives **two distinct, mutually-exclusive flows** — the adoption gate decides which is live, and they **never co-run**:

| Flow | Shape | Spine (CLI node ids) | Terminal | Lives in |
|---|---|---|---|---|
| 🧰 **adopt** (`harness-adopt`) | finite, once per repo | `install → governance → build-boot → bridge` (`scout`/`inject` = `branch_of` excursions) | `bridge` (a `decision`; the adopt→loop gate) | its own adopt flight plan during onboarding |
| ⚙️ **loop** (`harness-loop`) | cycling, every session | `boot → backpressure → observe → drain-gate → retro-drain → retro-harvest → improve` | `improve` (`next:[]`; the cycle is a **nav reset**, DAG stays acyclic) | **chores in `the-flow.json`** when a the-flow is active; else its own `.harness/loop.flow.json` |

- **Selection predicate**: the adoption gate **S0 (install) + S2 (governance) + S4 (boot)** all hold → the **loop** is live; otherwise **adopt** is live. Exactly one (`00-routing.md` § The two first-class flows).
- **Coexistence**: when the loop runs alongside an active the-flow, the four **fire** hooks (`pre-flight`/`pre-coding`/`post-coding`/`post-flight`) are injected as **chores** (`run /eng-harness-flow --hook <hook>`) onto `the-flow.json` — idempotent, dedup-keyed on the `--hook` token, lifecycle `todo → done|skipped`. Only `improve` gets no chore — `observe` (`coding`) now rides as a **per-phase chore** (the inversion of the old "coding gets no chore" line; see § Flight-plan harness seams below). The exact shape + the R-1 seam-node reconciliation are in [`references/flight-plan-ops.md`](./references/flight-plan-ops.md).

## Flight-plan harness seams — creation, lifecycle & execution discipline

<!-- doctrine-parity:039 v2 — this block is mirrored byte-identically in the-flow `references/harness-seams.md`; edit BOTH copies and keep them identical (the parity guard diffs them). -->

This is the **canonical** statement of how harness seams are laid onto an SDD flight plan, how they live, and how an agent satisfies them; the-flow's `harness-seams.md` mirrors this block verbatim. *(Supersedes the v1 reconcile-on-read mechanism and the the-flow `f9a86f1` prose-cadence step — no spawn-on-read, no `provenance.reconcile_hook`, no remembered prose trigger.)*

**Creation is two parts — no gate** *(D1 reversed the old three-part "skeleton + conditional chore apply + expander" model: the full seed is now baked **unconditionally**, with **no create-time gate and no expansion gate**)*:

1. **The full pre-authored template seed.** A fixed, hand-authored JSON template (`references/flight-plan.template.json`, BYO via `harness flow create --template`) ships the **complete 11-node starter** — the `research → plan → phase-1 → review-1 → post-flight → ship` spine **plus the 5 harness chores** (phase-1's own **boot + observe + drain**, plus the two globals **backpressure** anchored off `plan` and the **post-flight harvest** anchored off `post-flight`) **plus pre-authored per-node `instructions[]`**. The CLI instantiates it **verbatim** in **one `harness flow create --template` call** and stamps root identity (provenance / created_at / events / nav). This is **unconditional — there is no router-installed / repo-provisioned gate**: the chores seed at `status: known` (designed, definite work — never speculative) and are **mandatory for the agent, declinable only by the human** (`importance` grades advisory strength for the human, never agent licence), and an un-harnessed repo carries them **un-run, not absent** — provisioning gates only whether a chore is ever *run*, **never** whether its node *exists*. A **Simple 1-phase flow is therefore complete at `create`** — the spine **and** its five baked chores, no follow-up apply.
2. **A plan-complete additive expander.** When the plan locks N>1 phases, **one additive `apply` batch** splices `phase-2..N` after `phase-1`, **each new phase carrying its own spine `review-N` (`zone: flight`, interleaved `phase-N → review-N → next`) plus its boot+observe+drain trio** spliced from **this same doctrine**. Expansion is **purely additive — nothing relocates, no `mv`** — **byte-stable idempotent**, and **likewise un-gated** (a no-harness repo still gets the full per-phase chores, un-run). It is re-fired at every structural entry (plan-complete + adopt + resume-mismatch + manual sync) and **no-ops on a complete spine**. Expansion is triggered **structurally by the `plan` node completing** — no remembered prose trigger, and **no `expand` nodeType** (the schema is not extended for it).

**The per-phase additive chore model.** **Every phase** carries its own three chores — **boot** (`harness-boot`, `pre-flight`), **observe** (`observe`, `kind: command`, `importance: recommended`, command `harness observe "<what>" --kind <kind>`), and **drain** (`harness-retro` `(drain)`, `post-coding`) — plus **two global** chores: **backpressure** (`pre-coding`, anchored off `plan`) and the **post-flight harvest** (`harness-retro` `(harvest)`, hook `post-flight`, anchored off the `post-flight` close-out stage — a pre-existing flow that anchored it off `ship` keeps that anchor: dedup on the `--hook` token, never re-parent). `observe` **gets a chore** — the inversion of the old "the `coding` seam gets no chore" rule: the continuous mid-phase capture seam now has a structural anchor, surfacing in `due_chores` the whole time `nav.now` sits on its phase, with its lifecycle terminal at that phase's **drain**. Only **`improve`** gets no chore. **`(drain)` vs `(harvest)` is one `harness-retro` type**, disambiguated by hook — `post-coding` drains that phase's observe buffer; `post-flight` is the terminal long-horizon harvest — plus the label. **No new retro nodeType.**

**D5 — never resurrect a terminal node.** No mutation (`upsert`/`set`/`mv`/`remove`, single or batched) silently flips a `done`/`skipped` node back to `todo`; the guard is **batch-wide** — a `remove`-then-re-`add`/`upsert` of the same terminal id within one batch cannot launder it.

**Chore/seam execution discipline (the anti-fake rule).** A runnable harness chore/seam is satisfied **only by actually invoking the `/eng-harness-flow` skill** through the host's skill mechanism — **the Skill tool in Claude Code**, the equivalent slash-command invocation elsewhere — with the node's **exact `--hook`**. **Never** narrate a plausible router envelope, reimplement the check inline, or flip the node to `done` without a real invocation; the envelope is narrated **verbatim from that real call** (never fabricate an insight). For `observe`, the equivalent is **actually running `harness observe "<what>" …`** — a real capture, never a narrated one. **Execution of a seam *attempt* is mandatory for the agent — declining belongs to the human alone.** Every chore is resolved by a **real attempt**, whatever the environment: router **installed and routing** → the real invocation above; router **missing** (Layer-1 miss) → record a **detection receipt** — `harness flow comment --path <flow-path> --node <chore-id> --kind validation --source agent --text "decision:unavailable reason:<…> time:<…>"` — **then** `harness flow status --path <flow-path> --node <chore-id> --to done`; router installed but the envelope says `noop`/`UNAVAILABLE` → the same two calls with that **real envelope** as the comment text. All three are **completed attempts, never skips** — a chore never sits outstanding forever blocking `nav` in an un-harnessed repo. `skipped` is reserved for the human's discretionary decline, which is **two real CLI calls in the same turn, ordered**: `harness flow comment --path <flow-path> --node <chore-id> --kind decision --source user --text "<the human's verbatim words>"`, **then** `harness flow status --path <flow-path> --node <chore-id> --to skipped` — the receipt lands before the due entry clears; a status flip alone records nothing. Never a narrated skip, never a fake `done`, and never the agent's own judgment that a chore is "optional" (no wording — "advisory", "recommended", `importance` — is ever agent licence to skip). **Receipts — comment first, status second, always:** every terminalization is the append-only validation comment above (decision/verdict/reason + time from the real envelope or detection, and for `pre-coding` also `basis_sha256:<full SHA-256 of the plan file surveyed>`) followed by the status flip — never `set-node --note` for a receipt (comments are append-only, auditable history; a note is overwritable) — and a `done` harness chore with no receipt comment is treated as unsatisfied. **Never advance past a due or unreceipted chore:** `nav set --now` may depart a node only when `due_chores` is empty **and** — since terminal nodes leave that read — a read-only inspection of the node's anchored chores in `the-flow.json` shows every terminal chore carrying its receipt comment. Prior or manual evidence never substitutes for the seam attempt (a bare `harness boot` does not satisfy a `pre-flight` chore). For `observe`, terminalize at phase end **only after** at least one real capture — its validation comment carrying the real capture's record pointer — or a human decline; **then** the phase's drain runs. **Print-then-offer is untouched:** a routed action with side effects runs only on the human's explicit acceptance — **silence is neither acceptance nor decline** (the chore stays open until a real attempt outcome or a human decline resolves it). **Recurrence — latest-plan basis:** on each guided entry recompute the plan file's SHA-256; a mismatch with (or absence of) the terminal `backpressure` chore's recorded `basis_sha256` re-opens the seam with a **deterministic node id** — `backpressure-<first 12 hex of that SHA-256>` (full hash in the receipt; D5 — never resurrect the terminal one): same bytes re-use the same node and its valid receipt (a byte-stable no-op), changed bytes create exactly one new node, fired against the latest plan. Toward the **user** the posture is unchanged: chores never gate, never score, never block the human.

<!-- /doctrine-parity:039 v2 -->

## Registry

**This table is the master** verb↔module binding; the Graph (ordering + detection) is the master in [`references/00-routing.md`](./references/00-routing.md). Each lifecycle hook / adoption rung resolves to exactly one verb, and each verb to exactly one module — except `coding`/observe, which has **no module**: it is the silent `harness observe` CLI verb.

| Trigger (lifecycle hook / adoption rung) | Verb | Module / target | Produces |
|---|---|---|---|
| `pre-flight` | boot | `references/stages/boot.md` | boot verdict (HEALTHY / SLOW / UNHEALTHY / UNAVAILABLE) |
| `pre-coding` | backpressure | `references/stages/backpressure.md` | `backpressure-coverage.md` |
| `coding` | observe | — **CLI verb**: `harness observe "<what>" --kind <kind>` (silent; no module) | one observe-buffer entry |
| `post-coding` | retro (drain) | `references/stages/retro.md` | drained `.retro.md` |
| `post-flight` | retro (harvest) + improve | `references/stages/retro.md` | harvested view + encoded improvement |
| adoption gate · on-ramp / inject | adopt | `references/stages/adopt.md` | installed + injected harness (delegates: assess, add-extension) |
| adoption gate · build boot | add-extension | `references/stages/add-extension.md` | a loadable extension / basic boot |
| adoption gate · scout | assess | `eng-harness-0-harnessability-assessment` *(public peer skill — not a module)* | harnessability report |

Module missing at its path → say so and stop. Never improvise a verb from memory.

## Command grammar

Works with **no arguments** (full auto-detect); a parent driving its own flow can pin position. The flags' full semantics — plus the byte-stable public contract (`--hook` / `--event` / `--hooks` / `--json`) — are defined once in [`references/00-routing.md`](./references/00-routing.md); this is the surface summary:

- `--hook <name>` — the PRIMARY invocation; names one of the five neutral lifecycle hooks (`pre-flight | pre-coding | coding | post-coding | post-flight`).
- `--event <seam>` — a permanent, zero-break **alias** for `--hook` (the six host seams `session-start | post-spec | pre-implement | task-pause | phase-end | plan-complete` map onto the five hooks).
- `at=<stage>` — friendly stage hint (`auto` / `adopt` / `boot` / `backpressure` / `observe` / `retro-drain` / `retro-harvest` / `improve`).
- `--plan-dir` / `--spec` / `--phase` — pin the plan / spec / phase so detection is never ambiguous.
- `--prompt-optional <bool>` — parent owns skip-suppression for optional offers (default true).
- `--json` — the machine-readable routing envelope (+ the resolved `hook`).
- `--hooks [--json]` — the discovery manifest `{ manifest_version, hooks[5] }`.
- `--help` — the synopsis below (print-and-stop).

**A hint is never a command.** The router validates each hint's precondition (the adoption gate + the conflict matrix in `00-routing.md`) and **redirects** when a hint contradicts the signals — it never blindly runs the named stage. `--repo` is reserved for v2 (the router operates on `cwd`).

## Progressive disclosure

Load **exactly one** verb module (`references/stages/<verb>.md`) when a step is taken — never read all of them up front. A module may lazily pull `references/00-routing.md` § Shared conventions when it cites one (the sanctioned exception); reading modules for verbs you are not executing is not. The verb modules are **orchestration-blind**: they carry no sibling names, no flow position, no lifecycle-hook self-reference, and no routing — that knowledge lives only here and in `00-routing.md`. (A module may still `harness observe` a friction it hits at the moment it bites — execution-log note as the fallback when harness-less — since capturing friction is not orchestration.) Human-mode coaching (the rail, the narration beats, the why-table) lives only in [`references/coach.md`](./references/coach.md), never duplicated into the dispatch or the modules.

## `--help` — synopsis (print-and-stop)

`--help` prints this static synopsis and **stops** — no signal detection, no state, no routing, nothing fires:

```text
eng-harness-flow — stateless router to the harness loop (one front door).

USAGE
  /eng-harness-flow [--hook <name> | --event <seam>] [--plan-dir <p>] [--spec <p>]
                    [--phase <id>] [--prompt-optional <bool>] [--json] [--hooks] [--help]

LIFECYCLE HOOKS  (--hook, the primary invocation)
  pre-flight    before work starts       -> boot validation
  pre-coding    spec settled, pre-build   -> backpressure survey
  coding        mid-build (silent)        -> one in-flight capture
  post-coding   a phase just ended        -> per-phase retro drain
  post-flight   all phases done, pre-ship close-out -> terminal harvest + improve

DISCOVERY
  --hooks [--json]   the five-hook manifest: { manifest_version, hooks[5] }
  --json             machine-readable routing envelope (+ the resolved hook)

--event <seam> is a permanent alias for --hook — session-start, pre-implement,
post-spec, task-pause, phase-end, plan-complete (see "Lifecycle hooks").
```

## References

- **Required reading** — `harness docs the-harness-distilled` (source: `harness-foundations/the-harness-distilled.md`) — the complete engineering-harness reference: definition, the four-layer stack, backpressure, encode-don't-document, the CLI front door, extensions, sensors, the operating loop, and the minimal nucleus. Read it once before running the loop.
- [`references/00-routing.md`](./references/00-routing.md) — the routing engine: signals A–J, the adoption gate (the Graph), the engineering dispatch, the precondition/conflict matrix, verb/slug resolution, the `--json` envelope + `--hooks` manifest (the byte-stable contract), and § Shared conventions.
- [`references/coach.md`](./references/coach.md) — the human-mode voice: the rail (both flows), the Orient→Flag→Insight→Suggest→Invite contract, the why-table, the Flag beat, tone.
- [`references/flight-plan-ops.md`](./references/flight-plan-ops.md) — **the dogfood**: how the router drives its two flows as CLI flight plans (nav model, spine-vs-excursion, the `harness flow` verb flags + gotchas, build-order, the AC-07 chore shape + dedup key, standalone loop). Load once, before the first flight-plan mutation of a session.
- [`references/getting-started.md`](./references/getting-started.md) — the visual guide to the whole skill family: the two-zone big picture, who pulls each trigger, a worked walkthrough, and the `.harness/` directory map. The on-ramp for anyone new to the loop.
- [`references/governance-doc.md`](./references/governance-doc.md) — what the governance doc (`.harness/engineering-harness.md`) contains, the `harness-change` record ledger semantics, and the write conditions.
- [`references/maturity-assessment.md`](./references/maturity-assessment.md) — the canonical L0–L4 maturity ladder and how to assess which rung a harness sits on.
