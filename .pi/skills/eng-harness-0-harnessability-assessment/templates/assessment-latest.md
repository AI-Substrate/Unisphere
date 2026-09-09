# Harnessability Assessment — sample-service

Run metadata
- Timestamp: 20260604T224407Z
- Repo root: .
- Branch / commit: main / unknown
- Mode: static
- Commands executed: none
- Commands skipped: service boot, dependency install, external calls
- Safety notes: no secret values read; no dependency install; no service boot; no product-code changes applied

## Verdict

- Operate-Today: C (62%)
- Adaptability: C (55%)
- Harnessability Index: 58.5% (C)
- Final grade: C
- Readiness: H2
- Highest proof level detected: L2 Static/build/test
- Target next proof level: L3 Runtime interaction
- Confidence: medium

> `final_grade` augments — it never replaces — the Operate-Today / Adaptability tuple above, which stays primary. Here both axes are C, so the headline grade matches; a weak axis would always be reported alongside it.

## Assessment matrix

| Area | Grade | Score | Rationale |
|------|-------|------:|-----------|
| Engineering flows | C | 60 | Build/test/CI present; no canonical SDD flow. |
| Pre-commit & local gates | D | 45 | Lint/test exist but no pre-commit hook wiring. |
| CI / local equivalence | B | 75 | Unit + lint identical local vs CI; no smoke either side. |
| Existing harness concept | B | 72 | Canonical front door via `harness/cli/commands.json`. |
| Test mechanisms | C | 58 | Unit tests hermetic; few fakes/sinks; direct SDK construction. |
| External-dependency pressure | D | 42 | Database + email have no local substitute. |
| Code composition & seams | C | 55 | Feature-grouped modules; few injection seams. |
| Observability & evidence | D | 40 | Console logging only; no structured artifacts. |

## Plain-English assessment

The harness front door is present and command candidates are mapped, but no product-behavior scenario has been verified. A fresh agent can read the harness, run candidate static checks, and discover env var names, but cannot yet prove runtime behavior locally because there is no smoke path and no local substitute for the database.

## Top blockers

1. No meaningful smoke scenario (G001) — a server starting is not enough; one product-behavior proof is missing.
2. No local substitute for the database (G002) — remote/shared state blocks L4 consequence proof locally.

## Highest-leverage improvements

- Inspect the integration/migration test setup for an existing seed/reset or restore path that can be wrapped as a harness `proof` command (reuse before building).
- Add one fixture-backed smoke test hitting a single route (unblocks L3).
- Add a containerized local database plus idempotent seed/reset (unblocks L4).
- Add a local email sink so external-effect payloads are observable.
- Populate candidate command tiers in `harness/cli/commands.json`.

## First safe agent session plan

1. Read `.harness/engineering-harness.md` (the canonical governance doc — the only location).
2. Inspect `harness/cli/commands.json`.
3. Confirm prerequisites and env var names without reading secret values.
4. Read the test suite to see how it mocks, injects, seeds, and resets state — those mechanisms are candidate harness affordances.
5. Run the candidate fast check only if safe and permitted.
6. Ask the human for the smallest meaningful smoke path before claiming runtime proof.

## Harness surfaces

| Surface | Path | Kind | Status | Notes |
|---------|------|------|--------|-------|
| Governance file | `.harness/engineering-harness.md` | canonical | present | Canonical governance file |
| Agent route | `AGENTS.md` | canonical | present | Routes agents to the harness |
| Command map | `harness/cli/commands.json` | canonical | present | Sensor inventory |

## Repository topology

Single-package TypeScript HTTP service on Node. Package root `.`; service type `api`; CI workflow at `.github/workflows/ci.yml`. Not a monorepo.

## Existing engineering environment survey

> Surveyed before scoring — what the repo already has comes first.

### Engineering flows

| Flow | Kind | Commands | Canonical | Where detected |
|------|------|----------|-----------|----------------|
| Build/test | test | `npm test`, `npm run lint` | yes | `package.json#scripts` |
| CI | ci | `.github/workflows/ci.yml` | yes | `.github/workflows/ci.yml` |
| Spec-driven planning | sdd | — | no | not detected (no `plans/` pipeline) |

### Pre-commit and local gates

| Gate | Mechanism | Checks | Local | CI-equivalent |
|------|-----------|--------|-------|---------------|
| Lint | npm_script | eslint | yes | yes (run manually; no git-hook wiring) |
| Unit tests | ci_only | `npm test` | yes | yes (enforced in CI, not pre-commit) |

### CI / local equivalence

| Check | CI command | Local command | Equivalence | Notes |
|-------|-----------|---------------|-------------|-------|
| Unit tests | `npm test` | `npm test` | identical | Same command both sides |
| Lint | `npm run lint` | `npm run lint` | identical | Same command both sides |
| Smoke | — | — | unknown | No smoke lane exists |

### Existing harness concepts (canonical vs diffuse)

| Concept | Kind | Surfaces | Coverage | Notes |
|---------|------|----------|----------|-------|
| Harness front door | canonical | `harness/cli/commands.json` | Partial | Maps some scripts; tiers not fully populated |
| Ad-hoc onboarding | diffuse | `README.md` | Weak | Run/test steps live only in prose |

### Test mechanisms

| Mechanism | Type | Deterministic | Where detected |
|-----------|------|---------------|----------------|
| Unit tests | unit | yes | `test/*.test.ts` |
| HTTP client mock | mock | yes | `test/routes.test.ts` (brittle under refactor) |
| Email sink | sink | unknown | none — recommended affordance |

### External-dependency pressure

| Dependency | Pressure | Local substitute | Proof impact |
|------------|----------|------------------|--------------|
| Primary database | high | none | Blocks L4 state-consequence proof locally |
| Transactional email | blocking | none | Blocks external-effect proof; production mutation risk |

### Code composition and seams

| Area | Kind | Test seam | Coupling |
|------|------|-----------|----------|
| Routes | layer | no | Handlers construct SDK clients directly |
| Data layer | module | no | Shared util module is broad |
| Notify | module | no | Email send called inline |

### Deterministic-encoding opportunities

| Opportunity | Current encoding | Proposed encoding | Proof level |
|-------------|------------------|-------------------|-------------|
| Run/test steps live in README prose | doc | Map into `harness/cli/commands.json` tiers | L2 |
| Architecture boundaries in a context file | context_file | Add a boundary linter rule | L2 |

### Manual / IDE-only signals

> Advisory. Influences A4/A5/A7/A8/A9/B5/B10 only; never over-penalises desktop, mobile, hardware, or brownfield topologies.

| Signal | Kind | Influences | Penalize |
|--------|------|------------|----------|
| Service started via README copy-paste, not a harness verb | manual_step | A4, A5 | yes (fixable: wrap as a boot verb) |
| No manual/IDE-only blockers for this API topology | unknown | — | no (API service — no desktop/mobile penalty) |

### Candidate first harness surfaces

> Derived AFTER the survey above — never before existing flows and commands are inventoried.

| Surface | Rationale | Proof level | Already exists | Priority |
|---------|-----------|-------------|----------------|----------|
| smoke | One fixture-backed route test unblocks L3 runtime proof | L3 | no | high |
| seed/reset | Containerized DB + seed/reset unblocks L4 state proof | L4 | no | medium |

## Axis A — Operate-Today scorecard

| Dimension | Band | Points | Evidence | Notes |
|-----------|------|-------:|----------|-------|
| A1 Cold-start orientation | Partial | 2 | README, AGENTS.md | No explicit first-session steps |
| A2 Setup/environment contract | Partial | 2 | .env.example, lockfile | No doctor/preflight |
| A3 Locality / dependency exposure | Weak | 1 | DATABASE_URL, no compose | No local DB substitute |
| A4 Harness front door | Strong | 3 | harness/cli/commands.json | Wraps existing scripts |
| A5 Boot and health path | Weak | 1 | start script only | Readiness not provable |
| A6 Seed/fixture/reset/cleanup | Absent | 0 | none detected | No repeatable state |
| A7 Supported interaction surfaces | Partial | 2 | route handlers | No OpenAPI spec |
| A8 Deterministic back-pressure sensors | Partial | 2 | test+lint, CI | Unit lane only |
| A9 Observability and evidence | Weak | 1 | console logging | No run artifacts |
| A10 Compounding harness loop | Weak | 1 | empty docs/harness/ | No friction ledger |

## Axis B — Adaptability scorecard

| Dimension | Band | Points | Evidence | Notes |
|-----------|------|-------:|----------|-------|
| B1 Structural coupling / blast radius | Partial | 2 | feature modules | One broad util module |
| B2 Temporal/change coupling | Unknown | 0 | — | Not computed in static mode |
| B3 Cohesion / locality | Partial | 2 | tests near code | Some catch-all helpers |
| B4 Seams / substitution | Weak | 1 | direct SDK clients | Few injection seams |
| B5 Hermetic testability | Partial | 2 | offline unit tests | Integration not isolated |
| B6 Side-effect isolation / sinks | Weak | 1 | direct email send | No local sink |
| B7 State evolution / consequence | Weak | 1 | migration tool | No consequence checks |
| B8 Architecture boundary enforceability | Absent | 0 | no boundary linter | Prose rules only |
| B9 Complexity / navigability | Partial | 2 | eslint | No complexity rules |
| B10 Inner-loop speed | Partial | 2 | fast unit lane | No watch mode |

## Back-pressure surface inventory

- Static: `npm test` (L2, candidate), `npm run lint` (L2, candidate).
- Runtime: smoke path unknown (no command detected).
- Consequence, external-effect, observability, production/customer: not yet proven locally.

## Scenario probes

- API behavior loop (applicable): highest plausible proof today L3; missing a fixture-backed smoke test on one route.
- State/storage loop (applicable): highest plausible proof today L2; missing a containerized local database plus seed/reset.

## Command tiers

| Tier | Command or check | Status | Proof | Notes |
|------|------------------|--------|-------|-------|
| bootstrap | `npm install` | candidate_unverified | L1 | Not executed in static mode |
| fast | `npm test` | candidate_unverified | L2 | Not executed in static mode |
| boot | `npm start` | candidate_unverified | L3 | Requires services and secrets |

## Services, environment, and remote dependency exposure

Env var names only: `DATABASE_URL` (required, secret-like, no safe default), `LOG_LEVEL` (optional, safe default). External dependencies: primary database (remote required, shared-state risk, blocks proof partially); transactional email (remote required, production-risk side effect). No secret values were read.

## State, fixtures, reset, and cleanup

No seed, reset, or fixture command detected. State repeatability is currently unknown.

## Observability and evidence

Console logging only; no structured run artifacts, traces, or JSON report modes detected.

## Codebase affordance recommendations

| ID | Risk | Status | Recommendation |
|----|------|--------|----------------|
| CBA001 | medium | proposed | Add a local/test-only email sink that captures payloads; must not apply in production. |

## Harness-only recommendations

- P001: Populate candidate command tiers in `harness/cli/commands.json` from detected scripts (low risk, requires human review).

## Onboarding consolidation notes

- Run-and-test steps currently in `README.md` should be folded into the harness front door and assessment report.

## Human questions

| ID | Question | Reason |
|----|----------|--------|
| Q001 | What is the smallest meaningful smoke path for this repo? | Repository evidence did not identify a verified product-behavior scenario. |

## Evidence and inference log

| Source | Provenance | Claim |
|--------|------------|-------|
| `package.json` | inference | Candidate fast command may be `npm test`. |
| `harness/cli/commands.json` | evidence | A harness front door exists. |
