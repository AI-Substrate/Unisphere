# backpressure

> Sub-skill — a harness-blind verb module. Knows only its own domain work
> (filesystem sensor discovery, the spec it surveys against, its artifact).
> No sibling names, no flow position, no lifecycle-hook self-reference, no
> routing. Composition is the router's job.

**Verb**: backpressure
**Purpose**: **Select the deterministic proof** for the planned work — per acceptance criterion / failure mode, the exact repo-local **paved command** (build, typecheck, test, lint, runtime/smoke check, boot probe, architecture check — dependency rules, ArchUnit, Roslyn analyzers, CodeQL — schema validator, data-check script) whose green output will show that criterion holds — rather than leaving "done" to agent **inference** or human **eyeballing**. The survey doesn't stop at *can this be proven?*; it commits to *how it will be proven*, and where no sensor exists it specifies the Phase-0 build that creates one. The computational-control tier pulled forward to design time, so missing backpressure is caught and planned for **before** code is written.
**Consumes**: `PLAN_FILE` = `docs/plans/<ordinal>-<slug>/<slug>-plan.md` (required) — the unified document: `## Business Specification` (its `## Acceptance Criteria`, `## Target Domains`, `## Risks & Assumptions`) **plus** `## Implementation Plan` (its `#### Phase Index` when Full; a Simple plan is one phase). A legacy split folder (a `<slug>-spec.md` with no unified plan) remains readable — survey the spec and treat the work as one phase. Plus read-only repo signals (workspace manifests, build/task files, test/e2e signatures, CI config, analyzer/architecture configs).
**Flags**: `--plan <path>` / `--spec <path>` (resolve the plan; legacy spec accepted) — survey is idempotent; re-run any time the plan changes (the artifact records the surveyed plan's full SHA-256 as its **Basis**, so a changed plan is a changed basis).
**Produces**: one artifact — `${PLAN_DIR}/assets/backpressure-coverage.md` (overwrite-safe; create `assets/` if missing — a plan made before the assets layout may carry a legacy root copy, which stays untouched). No persisted index / rollup / ledger.
**Side effects**: none beyond writing the single artifact. Read-only against the repo.

## 🟢 ADVISORY INVARIANT — read first, never violate

This verb is **best-effort and advisory**. It exists to *inform a conversation*, not to police one.

"Advisory" describes this survey's **authority over the plan** — it never gates, blocks, or flips a plan to DRAFT. It is **not** the agent's licence to skip the survey before building: the agent runs it as part of "prove before you build," and the *user* may wave it past. Running it and being overruled is the contract; silently not running it is the failure mode.

- It **NEVER blocks** anything and **NEVER flips a plan to DRAFT**.
- The certainty rating is **qualitative** (Strong / Partial / Weak). It emits **no numeric score, percentage, floor, or SLA**.
- It produces **one artifact** (`assets/backpressure-coverage.md`) and **no persisted index / rollup / ledger** files. Cross-cutting views are recomputed at read time, never stored.
- The Recommended Phase 0 is a **recommendation**. It may be taken or ignored. It is never mandatory.

If a future change to this verb adds a threshold, a gate, a blocking behaviour, or a persisted index, that change is **wrong** — revert it.

**Always on.** There is no `.disabled` opt-out — if a user doesn't want a backpressure survey, they simply don't invoke it (or say so in chat).

**Grounding** (harness-foundations):
- **Rule 3** — prefer deterministic validation over agent inference: "The agent can say it is done. The harness should decide whether that claim is supported by evidence."
- **Principle 16** — "Improving the harness means reducing friction and increasing deterministic back pressure."
- **Principle 33** — "Verification must cover experienced failure modes" — startup, integration, rendering, hydration, side effects, architecture drift, security boundaries — not just the failures that are easy to unit-test.
- **Pattern 18** — tier computational vs inferential controls; **run computational controls early and often**.

A *Backpressure Check is distinct from back pressure itself*: it is an advisory, LLM-assisted look at the scoped work and the deterministic sensors the repo exposes — the proof still comes from the sensors, never from the LLM saying things look good.

---

## Procedure

```md
Inputs:
  PLAN_FILE  = `docs/plans/<ordinal>-<slug>/<slug>-plan.md`  (required; legacy fallback: `<slug>-spec.md`)
  PLAN_DIR   = dirname(PLAN_FILE)
  OUT_FILE   = `${PLAN_DIR}/assets/backpressure-coverage.md`  (create `assets/` if missing)
  BASIS      = sha256(PLAN_FILE bytes) — recorded in the artifact header
  Repo signals (read-only, all optional — probe recursively across the repo root AND every workspace/package root, never root-only):
    - workspace manifests: `pnpm-workspace.yaml`, `package.json#workspaces`, `Cargo.toml [workspace]`, `go.work`, `lerna.json`, `nx.json`
    - build/task files: `justfile`, `Makefile`, `package.json` scripts, `pyproject.toml`, `Cargo.toml`, `bin/dev`, `scripts/*`
    - test/e2e signatures: `**/playwright.config.*`, `**/cypress.config.*`, `**/vitest.*.config.*`, `**/jest.config.*`, `**/*.spec.*`, `**/*.e2e.*`, `connectOverCDP`
    - CI config (`.github/workflows/*`, `.gitlab-ci.yml`, etc.) — the de-facto PR proof gate
    - analyzer/architecture configs (`.dependency-cruiser.*`, `archunit`, Roslyn `.editorconfig`/`*.ruleset`, `codeql/`, JSON-schema files)
    - docs/governance (`.harness/engineering-harness.md` — the canonical and only governance location) — CORROBORATION ONLY, never a precondition
  today {{TODAY}}.
```

### PHASE 0 — Setup

1. Resolve PLAN_FILE (from `--plan`/`--spec` arg, the current plan folder, or an ordinal branch): prefer the unified `<slug>-plan.md`; fall back to a legacy `<slug>-spec.md` (treat the work as one phase). If neither exists → there is nothing to survey against; say so and STOP. (This verb surveys against a plan; it does not invent one.)
2. Read the `## Acceptance Criteria`, `## Target Domains`, and `## Risks & Assumptions` — the things the work must make true — plus the `#### Phase Index` (Full) or the single implementation block (Simple/legacy → one phase): each proof selected below is mapped to the phase that makes its criterion true.
3. Compute `BASIS = sha256(PLAN_FILE bytes)` — the artifact records it; a re-plan changes the basis, which is what triggers re-selection.

### STEP 1 — Inventory existing deterministic sensors

Discover sensors by **surveying the filesystem for actual tooling**, not by reading what docs claim exists. Governance docs (`engineering-harness.md`, harness READMEs, recipe comments) are **corroboration only** — they are frequently absent or actively misleading, so they are never the source of truth and never a precondition for finding a sensor. Ground the inventory in files that exist on disk. (A backpressure survey discovering its own sensors by inference would contradict Rule 3 — so discovery here is deterministic by design.)

#### 1a — Map the workspaces first (do NOT assume root)

Sensors commonly live one directory down in a sub-package, not at the repo root. Before probing, enumerate every workspace/package root:
- `pnpm-workspace.yaml`, `package.json#workspaces`, `lerna.json`, `nx.json` (JS/TS monorepos)
- `Cargo.toml [workspace]` (Rust), `go.work` (Go), `pyproject.toml` / `uv.workspace` (Python)
- Common harness locations even when no workspace manifest exists: `harness/`, `e2e/`, `tests/`, `test/`, `apps/*`, `packages/*`, `crates/*`

Run every probe below across **the root AND each discovered package root** — never root-only.

#### 1b — Deterministic signature probes (glob, don't eyeball)

Run this signature checklist with recursive globs (`**/…`) across all roots from 1a. Each hit maps to a sensor class. Treat the list as **extensible and language-agnostic** — add signatures for stacks you encounter:

| Signature (recursive glob / file content) | Sensor class proven |
|---|---|
| `**/playwright.config.*`, `@playwright/test` in any `package.json` | real-browser DOM e2e |
| `connectOverCDP`, `:9222` in configs/fixtures | live-browser / CDP driving harness |
| `**/cypress.config.*`, `cypress/` dir | browser e2e |
| `**/*.spec.*`, `**/*.e2e.*` under `tests/` \| `e2e/` \| `test/` | e2e / integration suite |
| `jsdom`, `happy-dom`, `@testing-library/*` in vitest/jest config | component DOM tests |
| `**/vitest.*.config.*`, `**/jest.config.*` (all variants) | unit / component test entry points |
| `.dependency-cruiser.*`, ArchUnit, Roslyn `*.ruleset`, `codeql/` | architecture-fitness checks |
| `**/*.schema.json`, schema validators | data / contract integrity checks |
| build / test / lint / typecheck targets in `justfile` / `Makefile` / scripts | maintainability + behaviour gates |

**Enumerate ALL test entry points across ALL packages before classifying any dimension.** A loud-but-narrow signal — a root recipe named `test-e2e` commented "CLI only", or a `test-harness` wrapper that actually runs vitest — does NOT imply the corresponding browser/DOM sensor is absent. The real sensor may sit in another package under a differently-named wrapper. Confirm by glob, not by recipe name.

#### 1c — Mine the named precedent (cheap, high-yield)

If the plan (or a research dossier, if present) cites a **precedent feature** (a prior plan / component of the same shape), look at *how that precedent is tested* — its test files are a direct map of the available sensors. Following the precedent's own spec files often surfaces a nested harness immediately.

#### 1d — Corroborate with docs + CI (optional, last)

Now read `.harness/engineering-harness.md` (the canonical governance doc), CI config (`.github/workflows/*` — the de-facto PR proof gate), and recipe comments to *enrich* what 1a–1c found: boot / health / validate / smoke / doctor commands, stated maturity. **If a doc disagrees with the filesystem, the filesystem wins.**

For each sensor found, capture: **name**, its **paved command** — the exact repo-local invocation a teammate would run: a `justfile`/`Makefile` recipe, a manifest script (`npm run <script>`), or a repo CLI verb — the **dimension** it guards (Pattern 19: `maintainability` | `architecture-fitness` | `behaviour`), and **where it was found** (root or which package). **Never record a floating/ambient invocation** (a raw `npx …`, a hand-typed one-liner nobody paved): a sensor reachable only through an unpaved command is a **pave-target** — classify it `BUILDABLE` with "pave the command" as its Phase-0 build, not `EXISTS`.

If **no signatures match after probing all roots**, record "no deterministic sensors found" *with the probe trail* (which §1b signatures were searched, across which roots) and expect certainty to trend **Weak** with a Recommended Phase 0. A missing governance doc is NOT itself evidence of absent sensors — when the doc is missing, the §1b sweep is the *only* ground truth, so run it thoroughly.

### STEP 2 — Derive this feature's experienced failure modes

From the plan's acceptance criteria, target domains, and risks, enumerate the concrete ways **this specific work** could be "green but wrong" (Principle 33) — and map each criterion/failure mode to the phase that makes it true (Phase Index when Full; everything is phase 1 when Simple/legacy). Do not limit to easy-to-unit-test failures. Consider: startup/boot, integration between components, rendering/hydration, side effects, **architecture drift** (boundary/dependency-direction violations), **contract breakage**, security-sensitive boundaries, and data integrity.

This is where the agent is encouraged to **get creative** about what *kind* of sensor each failure mode needs — anything from a one-line data-check script to a CodeQL query or a Roslyn analyzer, and everything in between. Sometimes the blocker isn't a missing check but that **the product exposes nothing checkable** — then the right suggestion is a **product-code affordance** that makes a sensor possible (a health endpoint, a smoke route, a testability seam). Affordances are **recommendations only**: the survey names them and their payoff; whether to touch product code is entirely the plan owner's call.

### STEP 3 — Build the coverage matrix and SELECT each proof

One row per acceptance criterion / derived failure mode. For each, **select the proof, don't just classify the gap**: the cheapest paved command that actually proves *this* criterion (a targeted spec file over "the whole suite"; a smoke route over "CI is green") —

- **`RUN:`** — an `EXISTS` sensor: the verbatim paved command, runnable today.
- **`EXTEND→RUN:`** — an `EXTEND` gap: an existing sensor covers most of it — name the **extension** (a rule added to the arch checker, a case added to a spec file, a route added to the smoke check), then the **same paved command** proves it. No new surface, no new invocation to learn; the extension rides the sensor's existing wiring (CI, habits, docs). **Prefer extending over building** — it is almost always the cheaper move and lands in a proven home.
- **`BUILD→RUN:`** — a `BUILDABLE` gap: the Phase-0 sensor build **plus the paved command it will expose** (proposed, clearly marked — never printed as if runnable today).
- `ABSENT` rows name their tier honestly (inferential / human-judgement) and carry **no** command — never a fake proof line.

The status↔mode mapping is fixed and coherent end-to-end: `EXISTS`→`RUN:` · `EXTEND`→`EXTEND→RUN:` · `BUILDABLE`→`BUILD→RUN:` · `ABSENT`→no command. Certainty and the closing verdict both derive from these modes.

Then classify each row:

- **Status** — `EXISTS` (a current sensor from Step 1 already proves it) | `EXTEND` (an existing sensor proves it after a named extension — same paved command) | `BUILDABLE` (no sensor today, but one can be specified within plan scope) | `ABSENT` (cannot be proven deterministically — legitimately inferential/human, routed to after-the-fact review, and that is fine).
- **Tier** (Pattern 18) — `computational` (deterministic check) | `inferential` (AI/eyeball review) | `human-judgement` (product/UX/taste decision).
- **Probe trail (REQUIRED for `ABSENT`)** — every `ABSENT` row must carry a one-line record of what was searched (the §1b signatures + which workspace roots), e.g. *"globbed `**/playwright.config.*`, `**/cypress.config.*`, `**/*.spec.*` under root + `harness/` + `packages/*` — no match"*. `ABSENT` is the most consequential verdict (it routes to manual gaps + a Phase 0), so it must never be asserted without evidence of having looked. An `ABSENT` row with no probe trail is a smell — re-run the §1b sweep before trusting it. (Mirrors the evidence-before-assertion discipline. This is a *record*, not a gate — it adds no threshold and never blocks.)

A row being `ABSENT` / `inferential` / `human-judgement` is **not a failure** — some things genuinely cannot be proven by a machine. The matrix just makes the split explicit and honest.

### STEP 4 — Advisory verdict

#### Certainty rating (qualitative — NO numbers, derived from the Proof Plan modes)
Rate the deterministic coverage of the **behaviour + architecture** rows (maintainability gaps and inherently-inferential rows do not drag the rating down):
- **Strong** — every behaviour/architecture criterion has a selected `RUN:` proof (an `EXISTS` sensor with a paved command).
- **Partial** — the behaviour/architecture gaps are `EXTEND→RUN:` or `BUILD→RUN:` (specified extensions or builds with the command they'll strengthen/pave — extensions being the cheaper rung).
- **Weak** — material behaviour/architecture criteria are `ABSENT`, or no deterministic sensors were found at all.

State the rating with a **one-line rationale tied to the matrix** (e.g., "3 of 4 behaviour criteria have EXISTS sensors; the 4th is BUILDABLE → Partial").

#### Counts and the next-move lookup (per-task decision aids — never a score)

Alongside the qualitative rating, emit the **mode-mix counts** over the behaviour + architecture rows — `<n> RUN · <n> EXTEND · <n> BUILD · <n> ABSENT` — transparent, task-local tallies. **No scalar, no percentage, no threshold** is ever derived from them (the 🟢 advisory invariant stands): counts *describe* this task's mix; they never grade it.

Then read the **next-move lookup** — a deterministic table, so "what do I do with this?" is a read, not a judgement. The decision is owned by **this task's mix**, never by a global or ad-hoc score:

| This task's mix | Recommended next move (advisory) |
|---|---|
| all `RUN` | start building — the proofs already exist |
| any `EXTEND` gaps | propose the extension(s) first — the cheapest move, landing in a proven home |
| any `BUILD` gaps on risk-linked criteria (named in the plan's Risks) | propose Phase 0 before feature code |
| material `ABSENT` on behaviour/architecture | **decision pause** — put it to the human plainly: proceed as-is, accept the standing inference tax, or build the check/affordance. A recommendation the human may override, never a plan gate |

**Gap ordering (ordinal only).** When several gaps exist, order which to close first deterministically: ① gaps tied to a named plan Risk; ② architecture > behaviour > maintainability; ③ extend before build. An ordering, not a score — there is nothing to pass or fail.

The closing verdict and its `In summary:` cite the table's recommended move **for this task**.

#### Recommended Phase 0 (conditional — routing trigger, NOT a threshold)
Include a **Recommended Phase 0: Establish Backpressure (build or extend)** table **iff** ≥1 behaviour/architecture criterion is `EXTEND`, `BUILDABLE`, or `ABSENT` with no `EXISTS` sensor. **Omit** it entirely when all behaviour/architecture criteria are `EXISTS`, or when the only gaps are inferential / human-judgement / testing-doc rows.

(This is a *routing* decision about whether to print a table — not a quality bar, score, or pass/fail gate.)

Each Phase 0 row specifies a sensor to **build or extend** — **extensions ranked first** (cheaper, no new surface, lands in a proven home): **what to build/extend**, **what it proves** (which criterion/failure-mode), a suggested **form** (extension to a named existing sensor / data-check script / dependency-direction rule / ArchUnit / Roslyn analyzer / CodeQL query / smoke route / schema check), and the **paved command it strengthens or exposes** (the `EXTEND→RUN:` / `BUILD→RUN:` line the Proof Plan carries). A row may additionally *recommend* a product-code affordance that would make the sensor possible — recommendation only, never a plan edit.

#### The Proof Plan is the primary product (selection, not enforcement)

Assemble `## Proof Plan (selected)` — per phase, the ordered list of proof lines whose green output shows that phase's criteria hold: `RUN:` lines verbatim-paved and runnable today, `EXTEND→RUN:` lines naming the extension to an existing sensor first (same paved command, made stronger), `BUILD→RUN:` lines naming their Phase-0 build first. These are the ready-to-fold *"<criterion> — done when `<paved command>` is green"* lines for whatever the plan uses to decide "done" (an acceptance criterion, a DoD item, a task) — handed to the plan's owner to fold into the re-plan, never applied by this verb. **Honesty boundary: this is selection, not enforcement** — nothing in this survey executes at phase end or guarantees the proofs are run; what binds it is the artifact + its `Basis` hash (a re-plan changes the basis, forcing re-selection against the latest plan).

#### Closing verdict (mandatory — plain human register, derived from the modes)

The survey **ends by answering its own question out loud**: *how will we know this work is actually done?* One short spoken block (also written into the artifact's `## Closing Verdict`), written the way a **principal engineer explains to a less experienced one, assuming zero context**. The register contract:

- **Plain labels only.** Never speak bare ids or internals (`AC-6`, `basis_sha256`, `EXTEND→RUN`) — name each promise in words ("the pricing-tier boundary rules"), with the id in parentheses at most. The modes and hashes stay in the artifact's tables; the spoken block *translates* them.
- **Say what was already done vs what needs the human's OK — as two explicit beats.** *"One thing I already did, automatically: …"* (e.g. wrote the how-to-prove-it commands into the coverage artifact) and *"One thing I'd like your OK on: …"* (e.g. update the plan so tasks inherit the proofs). Silence is neither acceptance nor decline, so the ask ends with a real question.
- **One breath of *why* per beat.** Each proposed beat carries its reason, briefly: commands-not-opinions (*"when these pass, those promises are kept — no judgement calls"*); written-where-the-work-lives (*"whoever picks this up later sees it, even after this conversation is gone"*); and fix-the-checker-first (*"if the checks pass but a human says it's not done, the checks are wrong — we fix them first, then the code, and that mistake can never slip through again"*). These are harness-foundations Rules 6 and 7, spoken plainly.
- **The verdict rides the four-rung ladder** the modes define, cheapest rung first — and **commands never overclaim the whole outcome**: certainty rates only the machine-checkable rows, so a Strong rating can coexist with a decision only a human can make. Say *"the work is done"* **only when no inferential / human-judgement row remains**; otherwise name that remaining human call plainly, in the same breath:
  1. **Fully provable today** (all `RUN:`) — *"When these commands pass, every machine-checkable promise in this plan is kept."* Add either *"— and nothing here needs a human judgement call, so green genuinely means done"* **or** *"— <the remaining call, e.g. whether the pricing page feels right> still needs your eyes; no command can judge that."*
  2. **Provable after extending** (`EXTEND→RUN:` gaps) — *"I'd like your OK to teach our existing <checker> <rule> — the same command everyone already runs, made smarter; once it lands, <that promise> becomes machine-checked too."*
  3. **Needs a new check** (`BUILD→RUN:` gaps) — *"<These promises> have no check yet. I'd like your OK to add a task that builds <check>, paving `<command>` — then they're covered."* (A proposed action, never a presumed plan edit.)
  4. **Partial at best** (material `ABSENT`) — *"Some of this can only be judged by people. Here's the highest-leverage check that would shrink that — or (a recommendation only, your call) the small product change that would make such a check possible."*
- **Always end with `In summary:`** — two or three plain sentences repeating the honest split: **what the commands will prove**, **what human judgement remains** (named, or explicitly "none"), **the recommended next move for this task** (from the next-move lookup — counts, never a score), and **exactly which approval is being requested**.

Also **flag thin coverage** here when it applies — rough-size what closing it would take: a single criterion line, extra work in this plan, or its own follow-up. All of it informs the conversation — the survey writes its artifact and leaves any editing to the plan's owner.

### OUTPUT — write `${PLAN_DIR}/assets/backpressure-coverage.md`

Overwrite if it exists (regeneration-safe). Use this template:

```markdown
# Backpressure Coverage — <feature>

**Plan**: [<slug>-plan.md](./<slug>-plan.md)   <!-- legacy: <slug>-spec.md -->
**Basis (plan SHA-256)**: <full 64-hex digest of the plan file surveyed>
**Generated**: <today>
**Certainty**: Strong | Partial | Weak

> Advisory only. Never blocks, never gates, no scores. (Advisory backpressure survey.)
> Selection, not enforcement: nothing here executes at phase end — the proof lines
> below are what the plan's owner folds into each criterion's "done when".

## Existing Sensors (inventory)

| Sensor | Paved command | Dimension | Found in |
|--------|---------------|-----------|----------|
| harness smoke | `just smoke` | behaviour | `harness/` |
| typecheck | `just typecheck` | maintainability | root |
| (none found) | — | — | — |

## Coverage Matrix

| Criterion / failure mode | Phase | Selected proof | Status | Tier | Probe trail (required if ABSENT) |
|--------------------------|-------|----------------|--------|------|----------------------------------|
| <AC-1 / failure mode> | <N> | RUN: `<paved command>` · EXTEND→RUN: add <rule/case/route> to <sensor> then `<same command>` · BUILD→RUN: <Phase-0 build> then `<command it paves>` · — | EXISTS / EXTEND / BUILDABLE / ABSENT | computational / inferential / human-judgement | <globs searched + roots, for ABSENT rows; — otherwise> |

## Proof Plan (selected)

<!-- Per phase, in order. RUN: lines are runnable today (verbatim paved commands).
     BUILD→RUN: lines are proposed — their Phase-0 build comes first. -->

### Phase <N>: <title>
| Proves | Mode | Proof line |
|--------|------|------------|
| <AC-1> | RUN | `just smoke checkout` |
| <AC-2> | EXTEND→RUN | add <rule> to <existing sensor>; then `just check-arch` |
| <AC-3> | BUILD→RUN | Phase 0 builds <sensor>; then `just check-<name>` |

## Certainty: <Strong|Partial|Weak>

Counts (behaviour/architecture rows): <n> RUN · <n> EXTEND · <n> BUILD · <n> ABSENT
Recommended next move (per-task lookup, advisory): <the table's move for this mix>

<one-line rationale tied to the Proof Plan modes>

## Recommended Phase 0: Establish Backpressure (build or extend)

<!-- Include this section ONLY if the routing trigger fires; otherwise omit the whole
     section. Extensions ranked first. An affordance is a recommendation only. -->

| Sensor to build/extend | Proves | Suggested form | Paved command it strengthens/exposes |
|------------------------|--------|----------------|--------------------------------------|
| extend <existing sensor> | <criterion / failure mode> | extension: <rule/case/route> | `just check-arch` (same command, stronger) |
| <new sensor> | <criterion / failure mode> | data-script / dep-rule / ArchUnit / Roslyn / CodeQL / smoke / schema | `just check-<name>` |

## Closing Verdict

<!-- Mandatory. Plain human register (STEP 4 contract): plain labels, never bare
     ids/internals; "one thing I already did" vs "one thing I'd like your OK on";
     one breath of why per beat; four-rung ladder (fully provable today / provable
     after extending X / needs new check / partial at best + affordance
     recommendation); fix-the-checker-first line when any RUN: proofs selected;
     commands never overclaim — "done" only when no human-judgement row remains,
     else the remaining human call is named; rungs 2/3 ASK ("I'd like your OK
     to extend/build…"), never presume a plan edit; MUST end with "In summary:"
     (2–3 plain sentences: what commands prove · what human judgement remains ·
     exactly which approval is requested). -->

<the spoken verdict, verbatim, ending with its "In summary:">
```

### How this differs from a measurability gate and from after-the-fact review (include a short note in the artifact if useful)

- A **testing-alignment gate** checks that *test tasks exist* and acceptance criteria are *measurable* — it does not ask whether a deterministic **sensor covers the experienced failure modes**.
- The **inferential / eyeball** tier is human/AI judgement after the fact. Legitimate and unchanged — this survey does not replace it.
- This survey covers the **computational** tier *before* building: can the work be *proven deterministically*, and if not, should we build the sensor first?

### Graceful degradation

A missing governance doc (`.harness/engineering-harness.md`) is **not** evidence of missing sensors — undocumented repos are exactly where de-facto sensors are most likely present-but-undocumented. When the governance doc is absent, the STEP 1b signature sweep is the *only* ground truth, so run it thoroughly across all workspace roots (STEP 1a) before concluding anything. Only after that sweep comes back empty across every root do you report "no deterministic sensors found" (with the probe trail), classify the behaviour/architecture criteria honestly (mostly `BUILDABLE`/`ABSENT`), let certainty trend **Weak**, and recommend a Phase 0. Either way the survey is useful — it either finds the nested harness or tells the user the repo genuinely has weak backpressure for this work.

## Exit

**Speak the Closing Verdict first, in the plain human register STEP 4 defines** — plain labels, the did-vs-needs-your-OK split, one breath of why per beat, the four-rung answer (fully provable today / provable after extending X / needs a new check / partial at best, with the fix-the-checker-first line when `RUN:` proofs were selected), **ending with `In summary:`**. Then print the output-contract summary (✅: what was produced, where, key fields — Certainty, the Basis hash, the mode-mix counts `<n> RUN · <n> EXTEND · <n> BUILD · <n> ABSENT`, and the next-move lookup's recommendation for this task), and hand over the per-criterion *"done when `<paved command>` is green"* lines for the plan's owner to fold into the re-plan. Picking the next harness stage is the router's job — this survey just informs the planning conversation and leaves the decision with whoever owns the plan.
