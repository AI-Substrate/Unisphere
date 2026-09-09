---
schema_version: '1.2'
retro_id: 2026-09-07T00:38:03.219Z-pij-right-kotallo-e94c
agent: pij-right-kotallo
plan_id: null
started_at: '2026-09-07T00:31:44.296Z'
ended_at: '2026-09-07T00:38:03.219Z'
summary: 'Adopted the repo-local harness on main without inventing a product lane:
  installed two project-local pi skills, added governance and AGENTS routing, verified
  loaded extensions and honest unconfigured checks/boot, and preserved machine collector
  settings.'
entries:
- id: DL-001
  kind: difficulty
  description: Fresh repository has no product manifest, source, tests, or canonical
    validation command; product readiness cannot yet be proved
  target: product-sensor
  severity: blocking
  workaround: Complete harness adoption with honest unconfigured checks and boot;
    do not invent product code
  suggested_encoding: When the first product lane exists, wrap its canonical check
    and add a fixture-backed telemetry ingestion smoke scenario
  fp: e94cae526f76
  system:
    compound:
      status: open
      source: agent-self
      first_seen_at: '2026-09-07T00:31:44.296Z'
  disposition: kept
- id: DL-002
  kind: difficulty
  description: Core observe has no instructions page despite the general per-verb
    self-briefing rule; its help surface provides the supported flags
  target: tooling
  severity: annoying
  workaround: Read harness observe --help and the core instructions instead
  suggested_encoding: Make core verb instructions resolve to their own usage or an
    explicit briefing
  fp: 27919ef42d65
  system:
    compound:
      status: open
      source: agent-self
      first_seen_at: '2026-09-07T00:31:57.151Z'
  disposition: kept
- id: DL-003
  kind: difficulty
  description: Unfiltered skills install added nine skills and temporary local-source
    provenance rather than the guide's two harness skills
  target: tooling
  severity: annoying
  workaround: Removed only newly installed extras and reinstalled the two harness
    skills with explicit filters and the public GitHub source
  suggested_encoding: Use explicit --skill eng-harness-flow eng-harness-0-harnessability-assessment
    and durable source in the onboarding install command
  fp: c77346b3312b
  system:
    compound:
      status: open
      source: agent-self
      first_seen_at: '2026-09-07T00:33:42.645Z'
  disposition: kept
- id: WIN-001
  kind: win
  description: Doctor reports two loaded extensions with no convention complaints;
    checks and boot preserve unconfigured exit 2 instead of claiming product readiness
  target: engineering-harness
  fp: dc8b64b9b616
  system:
    compound:
      status: open
      source: agent-self
      first_seen_at: '2026-09-07T00:34:48.591Z'
  disposition: kept
system:
  compound:
    bubble_action: all-save
  harness:
    record_kind: retro
    harness_version: 0.14.0
    branch: main
---

# Harness onboarding retrospective

## Verified evidence

- Global `harness --version`: 0.14.0; `harness --help` renders usage.
- `npx skills@latest list --agent pi`: exactly the two harness skills, sourced from AI-Substrate/harness-engineering.
- `harness doctor --json`: 2 loaded extensions, 0 failures, 0 conflicts, no convention complaints; overall degraded on capture-liveness and git-ai collector machine warnings.
- `harness checks --json` and `harness boot --json`: unconfigured, exit 2, explicit missing-product-lane next action.
- `node --test .harness/extensions/boot/extension.test.mjs`: 5 passed, covering missing/unconfigured/failed/passing/degraded checks. These are harness composition tests, not product validation.
- `harness instructions checks`, `harness instructions boot`, and both `--help` surfaces resolve.
- Observation capture/list and schema-valid durable record generation were exercised; only this session's bucket is drained.

## Boundaries and machine warnings

No product source, Cargo manifest, product tests, service startup, or external telemetry ingestion was added or claimed. The read-only architecture peer confirmed implementation has not begun. Global Git trace2 settings and pre-existing collector metadata were preserved. Product runtime maturity remains L0; completing repository onboarding does not complete the product-readiness bridge.

The installed CLI's default skills scope and the supplied guide disagree; the installation was narrowed to the two named skills with a durable GitHub source. Core observe instructions and the v2 authoring-doc ID were unavailable; existing help and the canonical source documentation supplied the contracts. These upstream documentation/installer gaps remain candidates, not silently patched vendored skills.

## Magic wand and missing proof

Highest-value next encoding: a fixture-backed telemetry input-to-normalized-output smoke command, once the first real product lane exists. It would prove collector behavior rather than asking an agent to infer readiness from healthy harness plumbing. This requires product implementation and was not invented during setup.
