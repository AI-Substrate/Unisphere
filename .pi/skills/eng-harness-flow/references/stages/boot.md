# boot

> Sub-skill — a harness-blind verb module. Knows only its own domain work
> (the `harness` CLI, `.harness/`, the governance doc, its verdict). No sibling
> names, no flow position, no lifecycle-hook self-reference, no routing.
> Composition is the router's job.

**Verb**: boot
**Purpose**: Validate that the engineering harness is healthy and report its maturity. `--validate` runs the live Boot → Interact → Observe health check; `--status` gives a quick read-only maturity report. Boot is **read-only** — it never writes governance or history.
**Consumes**: `.harness/engineering-harness.md` (the canonical governance doc — the only location). Reads: boot command, **checks command** (the mandated quality gate boot composes), health check, interaction method, observe method, current maturity snapshot, deterministic signal inventory, evidence paths, declared back-pressure gaps.
**Flags**: `--validate` (live 3-stage check; default if the governance doc exists) · `--status` (read-only maturity report — no boots, no changes) · (no flags) auto-detect: validate if the doc exists, else report `UNAVAILABLE`.
**Produces**: a boot verdict (`HEALTHY` / `SLOW` / `UNHEALTHY` / `UNAVAILABLE`) + a signal-readiness summary + a maturity reading. No file writes.
**Side effects**: none — boot reads; it never creates `.harness/engineering-harness.md`, `docs/harness/`, command maps, fixtures, or CLI scripts.

> **The harness IS the product.** Development infrastructure — CLI tools, build scripts, test harnesses, `just`/`make` recipes, seed scripts, environment setup, plus the agent-facing Boot/Interact/Observe loop on top — is not scaffolding. It is the first-class product of engineering work. Boot exists because if a brand-new agent session can't reach a healthy, observable running system in 30-60 seconds using only the governance doc, that is the most important thing to fix before any feature work. Every "no" here is harness work to do.

**Engineering harness governance**: `.harness/engineering-harness.md` — the canonical and ONLY governance location (the legacy fallback chain is retired). This module never *creates* the governance doc; provisioning it is the `harness init` CLI writer's job — it stamps the skeleton, seeded empty (L0, `TODO` fields). If the doc is absent, boot degrades gracefully (reports `UNAVAILABLE`) rather than blocking the session. See [`../governance-doc.md`](../governance-doc.md) for what the governance doc contains and when it is written.

**Layering**: the agent-facing Boot/Interact/Observe loop sits **on top of** the engineering substrate (the project's `justfile`/`Makefile`/`package.json scripts.dev` boot command, test runner, etc.). The boot command the governance doc records IS the engineering harness substrate. If no substrate exists, the verdict is `UNAVAILABLE` — boot can't work without something to boot.

**Signal readiness**: boot reports more than "can the process start?" It also checks whether the harness exposes enough deterministic signals for a human or agent to prove behavior without inference: runtime inspectability, smoke paths, architecture/static checks, security/dependency/schema checks, evidence paths, and known back-pressure gaps. Missing signals are improvement candidates, not blockers or scores.

---

## Procedure

```
$ARGUMENTS
# Flags:
# --validate   Run the live 3-stage Boot → Interact → Observe health check (default if governance doc exists)
# --status     Quick read-only maturity report (no boots, no changes)
# (no flags)   Auto-detect: VALIDATE if governance doc exists, else report UNAVAILABLE
```

### Step 0: Self-brief from the harness CLI (agent instructions) — REQUIRED when a CLI is present

If the repo has the harness CLI installed, you **must** brief yourself BEFORE validating — and before leaning on the repo for any real codebase work. Discovery is the whole point: the harness exists so you use the commands it already exposes instead of re-deriving them by hand.

```
npx harness instructions --json
```

Read the core agent briefing it returns (envelope contract, role split, discovery loop), then for every harness verb you expect to use this session, read its briefing too: `npx harness instructions <verb>`. The `verbs_with_instructions[]` field tells you which briefings exist. Skip this step **only** when the repo genuinely has no harness CLI — then skip silently. When a CLI *is* present, skipping discovery is a defect, not an optimisation: it is precisely how a built harness goes unused.

### Step 0b: Read the governance doc (canonical path)

```
Check governance file — ONE location, no fallbacks:
  .harness/engineering-harness.md                  ← the canonical path

Mode resolution:
  ├── EXISTS + no --status flag → VALIDATE mode
  ├── EXISTS + --status         → STATUS mode (read-only report)
  └── MISSING                   → report UNAVAILABLE gracefully (no governance doc to validate;
                                   provisioning it is the
                                   `harness init` CLI writer, not this module — run `npx harness init`)
```

### VALIDATE mode

#### Step 1: Read the engineering harness governance doc

Read the governance doc at `.harness/engineering-harness.md` (the only location — Step 0b). Parse: boot command, health check, interaction method, observe method, current maturity level, deterministic signal inventory, evidence paths, and any declared back-pressure gaps.

If the doc is missing or unparseable → report `UNAVAILABLE` (verdict table below). Provisioning the governance doc is the `harness init` CLI writer's job, not this module — so boot does not block the session; it notes the harness is not yet provisioned (run `npx harness init` to stamp the doc) and proceeds.

If the governance doc exists but omits signal-readiness sections, continue normally and report those dimensions as "not declared". Do not scaffold or rewrite the doc just to add them.

#### Step 2: Execute 3-stage validation

Run checks using the bash tool:

**Stage 1: Boot Check** (5s if running, 60s cold boot)
```
1. Check if already running: run health check command from engineering-harness.md
   ├── Healthy → "Already running" (skip boot)
   └── Not responding → Run boot command, retry health check (30 × 2s = 60s max)
```

**Stage 2: Interact Check** (5s, single attempt)
```
1. Send test input per engineering-harness.md § Interact
   ├── Response received → ✅
   └── No response / error → ❌ (log specific error)
```

**Stage 3: Observe Check** (5s, single attempt)
```
1. Capture evidence per engineering-harness.md § Observe
   ├── Evidence non-empty and readable → ✅
   └── Empty or failed → ❌
```

#### Step 3: Classify verdict

| Verdict | Criteria |
|---------|----------|
| **✅ HEALTHY** | All 3 checks pass, boot ≤ 45s |
| **⚠️ SLOW** | All 3 checks pass, boot > 45s |
| **❌ UNHEALTHY** | Any check fails |
| **🔴 UNAVAILABLE** | No governance doc at `.harness/engineering-harness.md` and no boot command |

#### Step 4: Read signal/back-pressure readiness

Build a short signal-readiness summary from the governance doc and observed evidence. Use plain categories; do not invent a numeric score or threshold:

| Dimension | What to look for | Report value |
|-----------|------------------|--------------|
| Runtime inspectability | App/API/CLI can expose current health/state to the agent. | present / missing / not declared |
| Smoke paths | A deterministic route, command, or scenario proves the main behavior starts. | present / missing / not declared |
| Architecture/static checks | Dependency rules, lint, type checks, ArchUnit/Roslyn/CodeQL, or similar checks exist. | present / missing / not declared |
| Checks gate | A mandated `harness checks` quality gate (lint/test/typecheck) exists and `boot` composes it. If the `checks` extension is **absent** (`.harness/extensions/checks/` missing), surface the deterministic warning the authored `boot`/`harness checks` emits: *"No `checks` extension exists — create one (`harness new checks --wrap "…"`) or move existing quality checks (linters, unit tests, typecheck) into a `checks` extension so `boot` and agents can gate on it."* | present / missing / not declared |
| Security/dependency/schema checks | Dependency audit, schema validation, CodeQL, data checks, or equivalent proof exists. | present / missing / not declared |
| Evidence paths | Screenshots, logs, traces, snapshots, artifacts, or command output locations are discoverable. | present / missing / not declared |
| Back-pressure gaps | The doc names behaviors that still rely on inference or human eyeballing. | list / none declared |

Treat absent dimensions as harness-improvement signals. They do not change `HEALTHY` to `UNHEALTHY` unless the actual Boot, Interact, or Observe checks fail.

#### Step 5: Report

Boot is **read-only**: it reports the current maturity it *read* from the governance doc — it does **not** write governance or history. (Provisioning happens at inception; the body + maturity snapshot change only when an improvement is encoded — see [`../governance-doc.md`](../governance-doc.md).)

(If the verdict is `UNAVAILABLE`, there is no governance doc — report the gap.)

Report:
```
🔍 Engineering Harness Validation Report:

  Boot:      [✅/❌] [detail] ([duration])
  Interact:  [✅/❌] [detail] ([duration])
  Observe:   [✅/❌] [detail] ([duration])
  Signals:   [runtime/smoke/static/security/evidence summary]
  Gaps:      [known back-pressure gaps or "none declared"]

  Verdict:   [verdict]
  Maturity:  L[N] ([description])
  Checklist: [X/15] items passing
  Missing:   [list unchecked items]
```

### STATUS mode

Quick read-only report — no validation, no changes.

Read the governance doc at `.harness/engineering-harness.md` and report: project type, maturity level (the current snapshot in the doc), and checklist completion. If `harness-change` records exist, the most recent one is the last *trajectory* change; boot does not itself track a "last validation date" (it writes nothing), so report that only if a `harness-change` record supplies it, otherwise omit it. No harness boots or health checks. If the doc is absent → report `UNAVAILABLE`.

### Measure compounding value

> **Measure.** Note what each session encodes — not for estimates, for evidence. The maturity level boot reports IS the dashboard reading. If a later session boots faster, cleaner, or at a higher maturity level than an earlier one because the previous session encoded what it learned, that is data proving the loop is closing. The `harness-change` record ledger — one record **per encoded improvement**, written when a harness change actually ships, **not** per session/boot — is the trajectory; a maturity level that climbs (or a boot time that shrinks) across those records is the compounding value made visible. Boot only *reads* this trajectory; it never writes to it. A flat or regressing trajectory is a signal that observed friction is not getting encoded — surface it for curation.

### Maturity model (reference)

The canonical maturity ladder is the **nucleus / self-improving** ladder — the same one `harness init` seeds into the governance doc at inception (maturity L0). Boot reports the level that is *actually working* (not aspirational), reading the current snapshot from the governance doc. The single canonical **L0–L4 ladder** (and how to assess which rung holds) is a **shared convention** — pull it from [`../00-routing.md` § Shared conventions](../00-routing.md), which points to [`../maturity-assessment.md`](../maturity-assessment.md). Boot does not restate it.

#### Agent-harness capability axis (separate from maturity)

This is a **capability axis**, not the maturity ladder — it describes what the agent-facing Boot → Interact → Observe layer can do, independent of where the engineering harness sits on the nucleus ladder above. It is deliberately **unnumbered** so it never collides with the L0–L4 maturity levels: a bare "L2" always means maturity, never capability. Useful when reporting how richly an agent can drive and observe the running system, roughly progressing:

**no interaction** (agent writes code, human tests) → **manual boot + API** (human starts the stack, agent sends requests) → **auto boot + API** (agent starts the stack, health check, API interaction) → **full interaction + evidence** (agent boots, drives UI/CLI, captures screenshots) → **self-healing** (auto-recovery from stale processes, auth expiry).

### What boot does NOT do

- **No setup or scaffolding**. It never creates `.harness/engineering-harness.md`, `docs/harness/`, command maps, fixtures, or harness CLI scripts. Those are provisioned by the `harness init` CLI writer, not by boot.
- **No gates, scores, or thresholds for back-pressure**. Signal-readiness gaps are advisory improvement candidates. Boot only fails when the live Boot, Interact, or Observe checks fail.
- **No product-specific sensor implementation**. It reports whether sensors are present or missing; it does not invent downstream project checks.

## Exit

Print the output-contract summary (✅: what was produced, where, key fields). Then STOP — do not name a next step or route onward. Routing is the router's job.
