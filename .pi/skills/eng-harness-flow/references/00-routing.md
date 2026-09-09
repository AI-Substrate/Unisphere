# 00-routing — the eng-harness-flow routing engine

> **The Graph + the public contract.** `SKILL.md` is the thin dispatch; this file is
> where detection, the adoption gate, the engineering dispatch, the precondition
> matrix, and the byte-stable public contract (`--hook` / `--event` / `--hooks` /
> `--json`) actually live. The dispatch points here. The verb modules under
> `references/stages/` are **harness-blind** — they never read this file, never name
> a sibling, and carry no flow position; routing, ordering, and the lifecycle-hook
> vocabulary are this engine's job alone (one graph, one owner). The coaching voice
> lives in [`coach.md`](./coach.md).

---

## The state contract (the engine's first principle)

**Routing is stateless; flow position is persisted (plan 032).** `eng-harness-flow`'s *detection* is a **pure dispatcher**: `(repo signals, conversation, optional hint) → next harness action`. It **re-derives every call** (re-entry after `/compact` needs nothing reloaded; the rail is recomputed from substrate, never remembered); **never gates, scores, or blocks** (every route is a suggestion; `at=`/`--hook` is a hint, never a command; declining the harness is conversational, not a `.disabled` file); **never runs `/compact`** (it can only recommend it); and **never invents a verdict** (`harness doctor` answers whether the harness is healthy, not the router's opinion).

> **"Never gates / blocks" is about the *user*, not the *agent*.** The router never hard-blocks, scores, or installs a `.disabled` sentinel — the **user** can always override or decline. That is not the **agent's** licence to silently skip a stage: the agent still boots before leaning on the system, surveys before building, captures friction as it bites, and at plan completion surfaces the top improvement and offers the encode — **out loud, every time**. The user may wave any of these past; the agent staying silent is the defect the loop exists to prevent.

What it **does** persist — the dogfood — is the **position of the flow it drives**, as a CLI-owned flight plan written **only** through `harness flow` (§ The two first-class flows; mechanics in [`flight-plan-ops.md`](./flight-plan-ops.md)). This is a *scoped* supersession of the old "writes nothing" stance: **flow position only**. Routing, verdicts, and the verb modules are unchanged — modules stay harness-blind, the five hooks + envelope contract stay frozen.

> **The unifying rule:** routing state that *could* drift lives in deterministic substrate a child verb owns (reports, buffers, `.retro.md`, the governance doc) — **never remembered by the router**. *Flow position* is persisted in the flight plan **by the CLI** (the single writer) — observable substrate, exactly as the-flow does it, not router memory.

---

## The two first-class flows (CLI-driven flight plans)

The router drives **two distinct, mutually-exclusive flows** as real `harness flow`
flight plans (the dogfood). The adoption gate below decides which is live; **exactly
one ever runs** — they never co-run. The *mechanics* of driving them (nav, verbs,
gotchas, chore shape) are in [`flight-plan-ops.md`](./flight-plan-ops.md); this is
the **graph + selection** owner.

### Selection predicate

> **gate(S0 install ∧ S2 governance ∧ S4 boot) ⇒ ⚙️ loop is live; else 🧰 adopt is live.**

The same required rungs that hard-gate the engineering zone (below) decide the flow:
the loop only runs once adoption has established install + governance + a working
boot; until then the live flow is adopt. The router re-derives this every call from
signals (A·B·C·D) — it never remembers which flow it picked.

### 🧰 adopt (`harness-adopt` overlay)

A finite onboarding flow, once per repo. Spine `install → governance → build-boot →
bridge`; `scout` (branch_of `install`) and `inject` (branch_of `governance`) are
skippable excursions; **boot is built LAST**, then you run it and cross the bridge.
`bridge` is a `decision` node (`next:[]`) — the adopt→loop gate. Rail title `[adopt]`.
Created with `harness flow create harness-adopt --slug <s> --title adopt`.

### ⚙️ loop (`harness-loop` overlay)

A cycling flow, re-entered every session. Spine `boot → backpressure → observe →
drain-gate → retro-drain → retro-harvest → improve`. `drain-gate` is a `decision`;
`retro` is split into `retro-drain` (post-coding) + `retro-harvest` (post-flight,
which also flushes telemetry — § engineering dispatch);
`improve.next:[]` — **the cycle is a nav RESET** (move `nav.now` back to
`observe`/`boot`), so the graph stays acyclic. Rail title `[harness-loop]`. The four
fire nodes carry `command: run /eng-harness-flow --hook <hook>`. Where it lives
depends on whether a the-flow is active:

#### Alongside an active the-flow → chores in `the-flow.json` (no separate plan)

When an active `the-flow.json` exists (a the-flow is mid-journey), the loop does **not**
author its own plan. Instead the router places its **four fire hooks as chores** on
the the-flow flight plan, so the-flow's rail tracks them and **they stop getting
missed**. Exact shape (full detail in [`flight-plan-ops.md`](./flight-plan-ops.md)):

- `chore.kind = command`, `command = run /eng-harness-flow --hook <hook>` for
  `<hook> ∈ {pre-flight, pre-coding, post-coding, post-flight}`, status `todo`.
- `importance = recommended`, except `pre-flight`/boot = `strongly-recommended`.
- **Anchored, never orphaned.** Each chore is `branch_of` its spine node (the
  **hook → anchor map** in [`flight-plan-ops.md`](./flight-plan-ops.md)), so it renders as a
  connected dotted excursion and `harness flow chores --at <node>` / `nav show`
  (`due_chores`) can surface it as **due at that node** — never a bare `add-node` orphan
  (`anchor:null`, floating off the rail with no run point).
- **Dedup key = the `--hook <X>` token** — exactly one chore per hook; re-running the
  injection is **idempotent** (byte-identical node set).
- **R-1 (single owner).** `eng-harness-flow` owns the chore flag. If a the-flow seam
  node (`harness-boot`/`backpressure`/`harness-retro`) already carries that hook's
  command, **flag it in place** (`set-node --chore-kind command --importance …`) rather
  than adding a duplicate; else `insert-node --branch-of <anchor>` a fresh **anchored**
  chore. (the-flow's `harness-seams.md` records this ownership so emission + injection
  don't double-fire.)
- `coding` gets **no `/eng-harness-flow` fire-hook chore** (silent in *that* sense), but
  `observe` **is** a per-phase chore — the-flow bakes the `harness observe` capture
  `branch_of` the phase, separate from these four fire hooks; `improve` follows a retro
  (no chore).
- **No `.harness/loop.flow.json` is authored while the-flow is active.**

#### Standalone (no the-flow) → its own `.harness/loop.flow.json`

With no active the-flow, the router creates and drives `.harness/loop.flow.json`
(`harness flow create harness-loop --path .harness/loop.flow.json --title harness-loop`),
moving `nav.now` along the spine and **resetting** to re-enter the cycle. That file is
**tracked** (committed, like the-flow's flight plans — the dogfood record); only the
observe scratch (`.harness/temp/`) stays gitignored.

---

## The two zones — 🧰 adoption gate, then ⚙️ engineering loop

Two grouped concepts, kept separate:

- **🧰 Harness adoption** — the one-time *journey* (the `adopt` verb) by which a repo takes the harness into how it works: install, scout, a **working boot command + governance**, and the injection point. Adoption wraps what the repo already has and weaves the loop into its extant flow. **Boot is built LAST** in adoption, deliberately, so the moment it's built you *run* it and flow straight into the dev loop ("build boot, run boot, then try the shiny new harness").
- **⚙️ Engineering flows** — the repeated ready-for-coding loop that *runs* it. Its first step **re-runs** the boot that adoption built; it never *creates* boot.

The router doesn't sit *inside* either zone — it sits *beside* them and points the caller at the right step. A single "harness functional?" gate is **not** enough to enter the engineering loop: the 🧰 adoption journey must have *established* the substrate, in order, ending with boot. The router walks the rungs and routes the **first missing one** to the adoption step that owns it — for the governance rung that means routing to `harness init` (the CLI writer that stamps the doc), noting that `init` seeds it *empty*, so a bare doc isn't a finished harness (see *seeded, not populated* below).

**The no-harness moment is an invitation, not a deficiency.** When nothing holds at all (S0 misses on a repo with source), narrate it as adoption, never as missing setup steps: *"Looks like this repo hasn't adopted an engineering harness yet. Adoption wraps what you already have — build, test, run, as-is — weaves the loop into your existing flow, and leaves a working `boot`. Want to walk through it?"* — then route to the `adopt` verb.

### The 🧰 adoption gate — THE GRAPH (in order; boot LAST)

This ordered gate **is the adoption graph** — its sequence is owned here, never inside `adopt.md` (the module knows only its own install + inject-map procedure).

| Rung | What adoption must have established | Read via signals | Required? | If missing → route to |
|---|---|---|---|---|
| **S0 · Install** | CLI present + `harness doctor` healthy | A · B | **required** | `adopt` (install) |
| **S1 · Scout** | a harnessability report exists | F | *skippable* | `assess` → `eng-harness-0-harnessability-assessment` (public peer) |
| **S2 · Governance** | governance doc (BIO contract) + `docs/harness/` ledger | D · E | **required** | `harness init` (stamps the doc; seeded empty — see below) |
| **S3 · Inject** | a recorded injection map — where the user's extant dev/SDD flow calls `eng-harness-flow` | D — governance doc `## Injection map` | *advisory* | `adopt` (Step 3 — inject) |
| **S4 · Build + run boot (LAST)** | a **working boot command** (authored, recorded into governance, **and run once**) — authored alongside a `checks` quality gate it composes (S4a checks → S4b boot) | C | **required** | `add-extension` (author `checks`, then boot; validate it boots) |
| **E1 · Re-run boot** | — (an **action**, not a presence-check) | — | — | **re-run** the boot adoption built — `boot` verb (per coding session) |

- **Required rungs** (S0 install, S2 governance, S4 built boot) hard-gate the engineering zone: without governance + a working boot the loop verbs report `UNAVAILABLE`/no-op, so the router **stays on the 🧰 adoption track**. The router routes the **first** missing required rung with a one-line *why*.
- **Skippable / advisory rungs** (S1 scout, S3 inject) are **offered, never blocking**. Because the router is stateless, a skip is *re-offered next call* unless the child artifact exists or the parent passes `--prompt-optional=false`.
- **Inject (S3) comes before boot (S4)** so that the instant boot works, the user already knows where `eng-harness-flow` plugs into their flow and can dive straight into real work.
- **`checks` rides along in S4, not as a separate required rung.** `add-extension` authors the mandated `checks` gate (lint/test/typecheck) just before boot, and boot **composes `harness checks`**. The gate is the engineering deliverable agents run before work is *done*; its *absence* surfaces as a deterministic `boot` warning (pointing at `harness new checks`), never as a hard gate — so a repo is never blocked on it.

> **Seeded, not populated (honesty about the skeleton).** The governance doc *writer* — `harness init` — **ships** (FX001): `harness init` stamps `.harness/engineering-harness.md` at inception, but seeds it **empty** (maturity L0, every other BIO field a `TODO`). So for S2 the router routes to `harness init` to *create* the doc; because `init` seeds it empty, the doc's *presence* satisfies S2 (the router reads presence, not contents — signal D), but the engineering loop still won't run until S4 builds the boot command — a bare seeded doc is the start of adoption, not its end. For S3 (inject) the `## Injection map` is one of those `TODO` sections `init` stamps empty and S3 fills. If the installed CLI is old enough to predate `init` (or `init` was never run and the doc is absent), the router reports `UNAVAILABLE` and stays on the adoption track (nothing errors). See [`governance-doc.md`](./governance-doc.md) for what the doc contains and when it's written.

### The ⚙️ engineering dispatch (only once S0 + S2 + S4 hold)

Once the required adoption rungs hold, the router crosses into the loop and dispatches by *where in the work* the caller is:

| Where in the work (signals H · I / `--event`) | Route to | Produces |
|---|---|---|
| session start / unknown | `boot` verb (re-run the boot adoption built) | a boot verdict (healthy / SLOW / UNHEALTHY / UNAVAILABLE) |
| spec done, pre-architect | `backpressure` verb | `backpressure-coverage.md` |
| mid-build (doing work) | capture is one CLI call — `harness observe "<what>" --kind <kind>` *(silent; capture judgment lives in the `retro` verb § in-flight capture)* | one buffer entry per call |
| phase / session end · **buffer non-empty** | `retro` verb `--drain` | buffer drained → `.retro.md` (`next_suggested: --harvest`) |
| phase / session / plan end · **buffer empty** | `retro` verb `--harvest` | curated cross-plan view |
| ad-hoc cross-plan analysis (`at=insights`; scope flags pass through) | `retro` verb `--harvest`, backed by `harness retro insights --json` | narrated cross-plan insights + cluster member provenance |
| improvement chosen | route the improvement by its shape — **sensor-shaped friction (a recurring check / diagnostic / proof you kept inferring → targets `project-sensor` / `runtime-inspectability` / `architecture-fitness` / `security` / `schema`) routes to `add-extension`** (scaffold a first-class, discoverable `harness <verb>`, the encoding move proper) · a one-off convenience → retro `[e]ncode` (recipe/doc) · larger work → a fix-plan command · (harness-product friction in a consumer repo) → an upstream issue on `AI-Substrate/harness-engineering` | the encoded harness change (or the filed upstream issue) |

- **Backpressure → `/grill-agent-done` (optional peer skill).** When the `backpressure` survey returns `ABSENT`/`BUILDABLE` sensors, the coach may offer `/grill-agent-done` before architect — a standalone skill in this repo (`skills/grill-agent-done/`) that interrogates and defends the definition of done one claim at a time, lining each against the right proof grade. **Not** a routed stage: the router points at it, exactly as the survey informs but never gates. Skippable; offer once.
- **Drain before harvest.** At phase/session/plan end, if the observe buffer is non-empty the router routes `--drain` *first* (harvest only reads `.retro.md`, so harvesting a non-drained buffer would miss the latest session). One command per call; the parent calls again for harvest.
- **Improve is where the loop compounds.** The loop only *compounds* when a retro leads to an encoded improvement. A single mid-plan session may encode nothing — but **`post-flight` (plan complete) must not end silently**: after harvest, name the highest-leverage candidate out loud and make the encode offer explicit (`[e]ncode` / `add-extension` / a fix-plan command / an upstream issue). The user may decline the offer; the agent may not skip making it. **Prefer `add-extension` for the sensor-shaped candidates** — a recurring inference (an eyeballed check, a re-derived diagnostic, a "no command proves X") is a missing `harness <verb>`, and a discoverable verb is the encoding that stops the loop re-paying that inference in tokens every session; a justfile recipe is the fallback for one-off convenience, not for a proof that recurs.
- **Flush telemetry at close (post-flight).** At session/plan end — after the drain/harvest — the router runs `harness telemetry sync`: a best-effort, **no-confirm** push of the counts-only telemetry buffer to its out-of-tree shard refs (`refs/harness-telemetry/<date>/<session>` — never the working tree or a PR; fail-safe and offline-safe, so it can never disturb the host). This is the loop's **guaranteed flush point**, complementing `checks`' build-time auto-push so telemetry is never left stranded by a session that didn't run `checks`. Honour the kill-switches: **skip** entirely when `HARNESS_NO_TELEMETRY=1`, and when `HARNESS_NO_TELEMETRY_AUTOSYNC=1` treat it as a reminder (nudge) rather than a push.
- **Ambiguous, never guessed.** With >1 candidate plan and no `--plan-dir`, the router returns `ambiguous` and **asks** — it does not guess the loop position from conversation alone.

> **This repo is the worked example.** `harness-engineering` has the CLI **and** two extension packages (`.harness/extensions/validate-harness-flow/` and `.harness/extensions/validate-harnessability/` — each a folder with `extension.ts` + `instructions.md`) **and**, since plan 014, its own governance doc at `.harness/engineering-harness.md` (boot = the CLI's vitest suite via `just test`). S0–S2 hold here; use it as the reference shape when routing other repos.

---

## Detection signals A–J

The router decides purely from signals it can **read** (no state of its own). This is the catalog; the decision order below applies them (hint first, then the adoption gate, then the engineering dispatch). There is **no `.disabled` opt-out** — opting out is conversational.

| # | Signal | How it's read | Tells us |
|---|---|---|---|
| A | **Harness CLI present** | `harness --version` resolves on PATH (the CLI is an ambient global tool, not a repo dependency); or a `.harness/` dir exists (the repo is already adopted) | Is there a harness at all? |
| B | **CLI healthy** | `harness doctor` **JSON envelope** read by `exit_code`/`status` field (not prose — the CLI loading and returning an envelope is the signal; a consumer repo can still show *degraded* on individual layers, but `cli-build` is `ok`/n/a there since FX001) | Does the CLI itself load/run? |
| C | **Working boot command** | a `boot` verb/recipe exists (`.harness/extensions/boot/`, a `justfile`/`package.json` boot, or governance declares it) **and** boots cleanly | Did adoption establish a boot we can run? |
| D | **Governance doc** | `.harness/engineering-harness.md` (the canonical and only location) | Is the Boot/Interact/Observe contract present? (Boot needs this or it reports `UNAVAILABLE`.) Its `## Injection map` section is S3's durable signal: which seams the host flow fires, from where |
| E | **Loop substrate** | `.harness/temp/` (gitignored observe scratch) + `.harness/records/retro/` (committed retro records, created via `harness record retro`) — legacy `docs/harness/agents/` retros are still read by harvest for back-compat | Can Observe/Retro actually record anything? |
| F | **Harnessability report** | any report under `.harness/reports/harnessability/` (path inconsistency noted — see "limits"; the router detects *any* report present) | Has the repo been sized up? |
| G | **Repo shape** | source tree empty vs. has source (e.g. `src/`, `package.json`, a language toolchain) | Fresh on-ramp vs. adopt-existing |
| H | **In-a-plan position** | `docs/plans/*/` artifacts: `*-spec.md` (post-spec), `*-plan.md` (post-architect), `tasks/phase-*/` + `execution.log.md` (mid-build), `reviews/` (reviewed) | Which loop stage the work is at *(best-effort — see "limits")* |
| I | **Conversation history** | the live session (what the parent just did/said) | Disambiguates G/H when files are inconclusive |
| J | **Parent hint (params)** | `at=<stage>` / `--event` / `--plan-dir` / `--spec` / `--phase` (see § Parameter contract) | Lets the parent pin position and skip detection |

### Decision order

1. **Parent hint first (J).** An explicit `at=`/`--event` is honoured **only if its precondition holds** (validated by the adoption gate + the conflict matrix). Otherwise the router **redirects** to the adoption step that owns the missing rung and says why — it never blindly runs the named stage when signals contradict it. (For the governance rung, "owns" means routes to `harness init` — the CLI writer the router calls but does not itself implement; see *seeded, not populated*.)
2. **Then the 🧰 adoption gate** (S0 → S1 → S2 → S3 → **S4 boot last**). Route the first missing **required** rung (S0, S2, S4); *offer* the skippable rungs (S1, S3) without blocking.
3. **Then the ⚙️ engineering dispatch** (the table above), keyed on `--event` / signals H · I, with drain-before-harvest and ambiguous-not-guessed.

### Where statelessness has limits (and who absorbs them)

1. **Skipped optionals re-offer.** With no memory, a declined scout/backpressure offer comes back next call. **Absorbed by**: the parent (skip-suppression via `--prompt-optional=false`) and by treating the **child artifact** as the only durable "done" signal.
2. **In-plan position is inferred, not known.** Plan-dir artifacts are ambiguous in real repos (multiple plans, stale phases, post-`/compact` context loss). **Absorbed by**: the parent pinning `--plan-dir`/`--spec`/`--phase`/`--event`; when unpinned and ambiguous the router returns `ambiguous` and **asks**.
3. **Provisioning gaps look like engineering entry.** A repo can pass S0 (CLI installed) yet lack S2 (governance) or S4 (a working, run-once boot), so a naive "functional?" check would route into the engineering zone whose verbs then report `UNAVAILABLE`/no-op. **Absorbed by**: the adoption gate — the router refuses to enter the ⚙️ engineering dispatch until S0 + S2 + S4 hold.

---

## Parameter contract

The skill works with **no** arguments (full auto-detect), but a parent driving its own flow can **pin** position so detection is never ambiguous.

```
/eng-harness-flow [--hook <name>] [at=<stage>] [--event <seam>] [--plan-dir <path>] [--spec <path>]
                  [--phase <id>] [--prompt-optional <bool>] [--repo <path>] [--json] [--hooks] [--help]
                  [--plan <slug>...] [--agent <slug>] [--since <date>] [--kind <kind>]

at=auto            (default) detect from signals A–J
at=adopt           force the on-ramp (install / finish adoption / stamp governance); alias at=setup
at=boot            force the boot verb (boot validation)
at=backpressure    force the backpressure verb (post-spec seam)
at=observe         guidance only; with --entry-* it silently runs `harness observe`
at=retro-drain     force the retro verb --drain (phase/session end)
at=retro-harvest   force the retro verb --harvest (plan complete)
at=insights        route the ad-hoc cross-plan insights surface (retro harvest mode)
at=improve         route a chosen improvement (retro [e]ncode / add-extension / fix-plan)

--hook <name>      pre-flight | pre-coding | coding | post-coding | post-flight
                   the PRIMARY invocation — names one of the five neutral lifecycle
                   hooks directly (§ Lifecycle hooks)
--event <seam>     session-start | post-spec | pre-implement | task-pause |
                   phase-end | plan-complete
                   a permanent, transparent ALIAS for --hook (maps per § Lifecycle
                   hooks); kept zero-break for every existing call site — never deprecated
--plan-dir <path>  pin the plan the loop stage refers to (disambiguates >1 plan)
--spec <path>      pin the spec for backpressure scoping
--phase <id>       pin the phase for boot/retro
--plan <slug>      insights scope: include one plan slug; repeatable
--agent <slug>     insights scope: include one agent slug
--since <date>     insights scope: include records at or after this ISO date
--kind <kind>      insights scope: include one exact observation kind
--prompt-optional  <bool>  parent owns skip-suppression for optional offers (default true)
--repo <path>      operate on a repo other than cwd (multi-repo callers; reserved for v2)
--json             return the routing decision as a machine-readable envelope
```

- **`--hook`/`at=`/`--event` is a hint, not a command.** The router validates the precondition (the adoption gate + the conflict matrix below) and **redirects** when a hint contradicts the signals (e.g. `at=boot` with no governance doc) — it never blindly runs the named stage. `--repo` is **reserved for v2** (the router operates on `cwd`).
- **Observe needs a payload; optionals re-offer.** `at=observe` with no payload is **guidance only**; to actually record, the parent passes the entry fields and the router runs `harness observe "<what>" --kind <kind>` silently (one entry per call). Because the router is stateless, a skipped optional (scout, backpressure) is **re-offered next call** unless `--prompt-optional=false` or the child artifact exists — only child artifacts count as durable completion.

### Lifecycle hooks

The router exposes its loop to host flows as a **closed set of five neutral lifecycle hooks** — the stable, stage-neutral vocabulary a host names when it calls the router (`--hook <name>`). The set is **fixed at five** (never grown per-repo) and **stateless** (derived every call, never stored). Each hook names a *moment in the host's lifecycle*, never a child-verb slug.

| Hook | When in the host's lifecycle | Behaviour | What the call drives |
|---|---|---|---|
| `pre-flight` | before work starts — session open, or about to implement | fire | boot validation (`harness-boot`) |
| `pre-coding` | spec settled, before building | fire | backpressure survey |
| `coding` | mid-build, in flight | **silent** — one capture per call | in-flight capture (`harness observe`) |
| `post-coding` | a phase / work-unit just ended | fire | per-phase retro drain |
| `post-flight` | the whole plan / journey is complete | fire | terminal close-out — harvest + present improvements + encode |

**`--event` seam → `--hook` mapping** (the six host seams alias onto the five hooks — `session-start` and `pre-implement` both open onto `pre-flight`):

| `--event` seam | `--hook` |
|---|---|
| `session-start` | `pre-flight` |
| `pre-implement` | `pre-flight` |
| `post-spec` | `pre-coding` |
| `task-pause` | `coding` |
| `phase-end` | `post-coding` |
| `plan-complete` | `post-flight` |

Two mappings are load-bearing and easy to get wrong:

- **`pre-implement` opens onto `pre-flight`, not `pre-coding`.** It fires the `harness-boot` node — prove the system runs before a line of code — the same boot `session-start` re-runs. (Naming it `pre-coding` would route it to the backpressure survey, which is wrong.)
- **`phase-end` → `post-coding` and `plan-complete` → `post-flight` are distinct.** Per-phase drain (`post-coding`) and the terminal harvest-and-improve (`post-flight`) are different lifecycle positions; collapsing both onto one hook would bury the **Improve** beat.

### Verb / slug resolution (avoid version drift)

Like `the-flow`'s alias table, the router maps friendly stage names → their **resolution target at call time** and **never appends a guessed version suffix**. Post-consolidation, the loop and adoption verbs resolve to **modules** under `references/stages/`; `assess` stays a **public peer skill**; `observe` is a **CLI verb** (not a skill). The current map:

| Friendly name | Resolves to |
|---|---|
| `adopt` (alias: `setup`) | `references/stages/adopt.md` (module) |
| `assess` | `eng-harness-0-harnessability-assessment` (public peer skill) |
| `add-extension` | `references/stages/add-extension.md` (module) |
| `boot` | `references/stages/boot.md` (module) |
| `backpressure` | `references/stages/backpressure.md` (module) |
| `observe` | `harness observe` *(CLI verb — in-flight capture; the `retro` module's § in-flight capture carries the judgment)* |
| `retro` | `references/stages/retro.md` (module) |

If a verb fails to resolve at runtime, **do not guess a suffix** — fall back to printing the bare verb name and point at `references/stages/`.

### Precondition / conflict matrix

When a hint conflicts with the detected signals, the router resolves **deterministically** (never guesses, never blindly runs):

| Hint / event | Conflict | `decision` | Router does |
|---|---|---|---|
| `at=boot` | no governance (S2) or boot not built yet (S4) | `redirect` | route to the missing step — stamp governance via `harness init` (S2), then build+run boot last (S4); `missing_rung: S2`/`S4` |
| `at=backpressure` | no spec, or >1 spec and no `--spec` | `redirect` / `ambiguous` | ask for `--spec`, or route to `/plan-1b` first |
| `at=retro-drain` | buffer empty | `noop` | "nothing to drain"; suggest `--harvest` if `.retro.md` exist |
| `at=retro-harvest` | buffer non-empty | `redirect` | drain first; `next_suggested: --drain` then `--harvest` |
| `at=insights` | buffer non-empty | `route` | run the read-only insights surface now; carry a drain advisory in the envelope (`buffer_pending`, `next_suggested: --drain`) but **never redirect**; pass `--plan` / `--agent` / `--since` / `--kind` through to `harness retro insights --json` |
| `at=adopt` (or alias `at=setup`) | adoption already complete | `noop` | "harness already adopted"; suggest `at=boot` |
| `at=auto` | >1 candidate plan, no `--plan-dir` | `ambiguous` | list plans, ask / require `--plan-dir` |

### The `--json` routing envelope

`--json` returns the routing decision as a machine-readable envelope so a parent agent can act without parsing prose (the harness "return prompting" ethos). It carries at least these fields:

```jsonc
{
  "requested_stage": "boot",
  "actual_stage": "adopt",
  "hook": "<the lifecycle hook this call resolves to: pre-flight|pre-coding|coding|post-coding|post-flight — the --hook value, or the hook the --event seam aliases to>",
  "decision": "route | redirect | noop | ambiguous",
  "command": "<exact next harness command>",
  "why": "<one line>",
  "produces": "<artifact the routed skill will create, or null>",
  "preconditions_met": false,
  "missing_rung": "S4-build-and-run-boot",
  "next_suggested": "<the command after this one, e.g. --harvest after --drain>",
  "bypass_recommended": false,
  "bypass_cause": "<a harness-bypass `cause` enum value when bypass_recommended is true, else null>",
  "rail":  { "zone": "adopt", "adopt_pips": "◆◆◐◇◇", "loop_pips": "◇◇◇◇◇", "cursor": "governance" },
  "now":   "<current stage, one line>",
  "next":  "<what follows, one line>",
  "flags": [ "<must-see item lifted verbatim from the artifact>" ],
  "insight": "<one interesting real detail>"
}
```

The `hook` field is **additive** — no existing field is reshaped or renamed. A routing call (`--hook X --json`, or its `--event` alias) returns the envelope above **plus** `hook` (the resolved lifecycle hook for the call); the routing envelope never embeds a hooks manifest.

The `rail`/`now`/`next`/`flags`/`insight` fields carry the UX signals (see [`coach.md`](./coach.md)) so a machine caller can render the same pleasant rail + flag beat a human gets. This matters precisely *because* the router is stateless: the rail, now/next, and flags are all **recomputed from substrate every call** — a pure function of "what the repo looks like right now," which is why the UX survives `/compact`, serves any caller, and never drifts from reality.

`bypass_recommended` / `bypass_cause` are **advisory flags only**. When the router detects that the caller hit (or is about to hit) harness friction worth recording, it *flags* it — setting `bypass_recommended: true` and a `bypass_cause` drawn from the `harness-bypass` `cause` enum — so the parent can offer `harness record harness-bypass`. Consistent with the stateless contract, the router **never writes a record and never blocks** on this; it only reads/derives the flag (default `false` / `null`).

### The `--hooks` discovery manifest

`--hooks [--json]` is the **discovery** surface — the routing envelope's counterpart. A routing call (`--hook X`) answers *"what do I do now?"*; `--hooks` answers *"what hooks exist, and how does a host wire them?"* — a host (e.g. `the-flow`) reads it once to learn the contract, then routes against it.

`--hooks --json` returns a **top-level** object (Shape A — never wrapped in `data`), so a host detects future shape changes via `manifest_version`:

All five entries pin the **same nine fields** (`hook`, `intent`, `run_at`, `kind`, `invoke`, `aliases`, `produces`, `needs`, `preconditions`). Comments annotate the first entry; the fields repeat. Note `coding`'s `invoke` — it is the **silent `harness observe` capture**, *not* an inferable `/eng-harness-flow --hook coding` fire; a host must hand-wire it from the exact string here:

```jsonc
{
  "manifest_version": 1,
  "hooks": [
    {
      "hook": "pre-flight",                              // one of the fixed five lifecycle hooks
      "intent": "prove the system runs before work starts",
      "run_at": "session open / before implementing",
      "kind": "fire",                                    // "fire" | "silent"
      "invoke": "/eng-harness-flow --hook pre-flight --json",
      "aliases": ["session-start", "pre-implement"],     // the --event seams this hook subsumes
      "produces": "boot verdict",                        // artifact/effect, or null
      "needs": [],                                       // upstream inputs this hook expects
      "preconditions": ["S2-governance", "S4-boot"]      // adoption rungs that must hold; [] otherwise
    },
    {
      "hook": "pre-coding",
      "intent": "survey what the work can prove deterministically, before building",
      "run_at": "spec settled, before building",
      "kind": "fire",
      "invoke": "/eng-harness-flow --hook pre-coding --json",
      "aliases": ["post-spec"],
      "produces": "backpressure-coverage.md",
      "needs": [],
      "preconditions": []
    },
    {
      "hook": "coding",
      "intent": "capture in-flight friction, one entry at a time",
      "run_at": "mid-build, in flight",
      "kind": "silent",
      "invoke": "harness observe \"<what>\" --kind <kind>",
      "aliases": ["task-pause"],
      "produces": "one observe-buffer entry",
      "needs": [],
      "preconditions": []
    },
    {
      "hook": "post-coding",
      "intent": "drain this phase's captured friction into a retro",
      "run_at": "a phase / work-unit just ended",
      "kind": "fire",
      "invoke": "/eng-harness-flow --hook post-coding --json",
      "aliases": ["phase-end"],
      "produces": "drained .retro.md",
      "needs": [],
      "preconditions": []
    },
    {
      "hook": "post-flight",
      "intent": "close out the whole plan — harvest, present improvements, encode",
      "run_at": "the whole plan / journey is complete",
      "kind": "fire",
      "invoke": "/eng-harness-flow --hook post-flight --json",
      "aliases": ["plan-complete"],
      "produces": "harvested cross-plan view + encoded improvements",
      "needs": [],
      "preconditions": []
    }
  ]
}
```

- The spine is **fixed at five** — `--hooks` never grows or shrinks per-repo (closed spine); the *only* per-repo variability rides in each entry's `preconditions`.
- `--hooks` is a **pure discovery** call: derived every call (never stored), it runs no detection, reads no plan signals, and writes nothing.
- **Routing and discovery stay separate**: a routing call (`--hook X --json`) returns the envelope **plus** `hook` and **never** embeds this manifest; a discovery call (`--hooks --json`) returns **only** the top-level `{ manifest_version, hooks }` and never a routing envelope.

---

## Called repeatedly along an externally-managed flow

The router is designed to be **invoked again and again** by a parent running its *own* flow (`the-flow`, a human, a CI agent). The seams where the parent calls the router are exactly the **injection points** adoption step S3 mapped and recorded in the governance doc's `## Injection map` — `the-flow` is one host implementation of this contract, not the contract itself; any SDD or dev flow plugs in the same way. Each call is a fresh, stateless detection; the router holds no memory between calls. A parent passes a light hint at each seam and the router returns the right harness action:

```
P → H: session start    (--event session-start)            → boot verb (validate)
P → H: pre-implement     (--event pre-implement --phase <p>) → boot verb (validate)   (--hook pre-flight)
P → H: post-spec         (--event post-spec --spec <path>)   → backpressure verb
P → H: task-pause        (--event task-pause)                → harness observe        (--hook coding, silent)
P → H: end-of-phase      (--event phase-end --plan-dir <p>)  → retro verb --drain   (buffer non-empty)
P → H: plan-complete     (--event plan-complete)             → retro verb --harvest (buffer now empty)
```

Each seam aliases onto a lifecycle hook — `session-start`/`pre-implement` → `pre-flight`, `post-spec` → `pre-coding`, `task-pause` → `coding`, `phase-end` → `post-coding`, `plan-complete` → `post-flight` (§ Lifecycle hooks). Note on `post-flight` timing: "the whole plan / journey is complete" means **the work is complete**, not that it shipped — the-flow fires it at its **pre-ship `post-flight` close-out stage** (all phases + reviews done, before the plan folder archives; a ship may follow later, or never). The hook's frozen wording is host-neutral; where in its own lifecycle a host places the close-out is the host's call.

This is the inversion of `the-flow`'s hard-coded harness cues: instead of a parent hard-coding *which* harness verb to mention at each seam, it can simply call `/eng-harness-flow --hook <name>` (or the `--event <seam>` alias) and let this skill own the harness-routing logic in **one** place.

---

## Shared conventions

Conventions shared across the dispatch, this engine, and the verb modules. A module may **lazily pull** a single named convention from here when it cites one (the progressive-disclosure sanctioned exception) — but a module never reads the rest of this file.

- **Maturity vocabulary.** The L0–L4 maturity ladder and how to assess which rung a harness sits on is the canonical reference in [`maturity-assessment.md`](./maturity-assessment.md). The `boot` verb cites it via this shared-conventions pull (never by naming a sibling).
- **No time estimates — ever.** Complexity is expressed as a **Complexity Score (CS 1–5)**, never as hours/days/sprints. This holds in every artifact the loop produces.
- **Verbs are harness-blind.** Modules under `references/stages/` know only their own domain work — calling `harness` CLI verbs, reading `.harness/`, writing their artifact (artifacts are the sanctioned wire protocol). They carry **no** sibling-verb names, **no** SDD-flow position, **no** lifecycle-hook self-reference, and **no** "next routing"/"next step" section. Routing, the lifecycle-hook vocabulary, and the adoption-gate order are this engine's job (one graph, one owner); narration is [`coach.md`](./coach.md)'s.
- **Declared delegation (sanctioned exception).** When a verb genuinely orchestrates a peer (e.g. `adopt` drives `assess` + `add-extension`), it names them only under a `**Delegates**:` line resolved through the Verb/slug resolution table above — never as inline flow prose.
- **Anti-reinvention / separation of concerns.** `the-flow` owns the **SDD** journey (stateful) and narrates its own cues; `eng-harness-flow` owns the **harness loop** (stateless). The `adopt` verb *drives* establishment of the harness (install, scout, weave the injection map, help author boot); the governance doc itself is stamped by the `harness init` CLI writer (seeded empty), which adopt calls. This router owns only "which adoption rung is still missing, or are we past adoption and into engineering?"
