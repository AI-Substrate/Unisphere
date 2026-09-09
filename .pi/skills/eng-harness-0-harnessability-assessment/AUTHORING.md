# Authoring notes - eng-harness-0-harnessability-assessment

**This file is repo-internal and NOT installed by the skill.** It documents conventions future editors should preserve.

## Why this skill exists

The adoption flow (`/eng-harness-flow`) creates or validates the repo-local engineering harness nucleus. `eng-harness-0-harnessability-assessment` runs after adoption and answers a different question: how harnessable is this target repository now (Operate-Today and Adaptability), what can a fresh agent safely do first, what proof level is reachable, and what should be encoded next?

Do not turn the assessment into setup v2. Do not turn it into a runtime loop. It is a whole-repo, structural, evidence-first read.

## Load-bearing invariants

These invariants apply to shipped surfaces only: `SKILL.md` and files under `templates/`.

### 1. Canonical boundary sentence

The sentence below must remain byte-identical wherever the agent/engineering harness boundary is stated:

```text
The agent harness drives. The engineering harness proves.
```

The canonical source is `templates/canonical-boundary.txt` (59 bytes, trailing newline pair).

### 2. Setup / assessment / runtime separation

Shipped surfaces must preserve this responsibility split:

- setup creates or validates the engineering harness nucleus;
- assessment reports target-aware harnessability, scoring, and recommendations;
- tools runtime skills operate the loop.

### 3. Neutral framing — no "replaces / supersedes / legacy"

This skill covers cold-start onboarding as one part of harnessability (A1/A2/A4 inside Operate-Today). Shipped surfaces must NOT claim the skill replaces, supersedes, or deprecates any named predecessor skill. Use neutral framing: "folds in", "covers", "consolidates", "earlier reports if present". Avoid the words `supersede`, `successor`, `legacy`, and `migration` except where `migration` means a genuine technical database/schema migration.

### 4. Foundation citations

Markdown templates that paraphrase foundation principles should carry an HTML-comment citation near the top:

```html
<!-- foundations: first-principles#NN, patterns-that-work#PNN, directives#DN -->
```

Use foundation IDs only. Do not cite raw source IDs.

### 5. No private-source contamination

Shipped surfaces must not reference private scratch material, source-note paths, plan artifacts, source IDs, private substrate names, customer-specific details, or unreleased platform details.

The shipped-surface grep should include at least:

```text
harness-foundations/
docs/plans/
scratch/
source-notes/
S001|S002|S003|S004|N2-S00|M00
```

Sanitized examples must use the obviously-fabricated `sample-service`, never a scrubbed real repository. Do not embody a real repo's internal service names, env vars, URLs, ports, cloud regions, or team names in examples. HTML-comment foundation citations are allowed.

### 6. Placeholder syntax

Every placeholder in templates must use canonical upper-snake form:

```text
{{[A-Z_][A-Z0-9_]*}}
```

Filled example files such as `assessment-latest.md` and `assessment-latest.json` should contain no unresolved placeholders.

### 7. Backpressure boundary

Backpressure Check is advisory over deterministic sensors. Shipped surfaces may describe Backpressure Check, but they must not introduce a generic core `backpressure` command key.

### 8. Schema is authoritative

`templates/assessment-report.schema.json` is the v0.2 contract. Top-level `additionalProperties` is `false` with the required minimum-shape keys plus the carried-back keys (`harness_surfaces`, `topology`, `applied_patches`, `onboarding_consolidation`, `first_safe_session_plan`). v0.2 adds **optional** top-level survey arrays (`report_paths`, `assessment_matrix`, `engineering_flows`, `pre_commit_gates`, `ci_local_equivalence`, `existing_harness_concepts`, `deterministic_encoding_opportunities`, `test_mechanisms`, `external_dependency_pressure`, `code_composition`, `candidate_first_harness_surfaces`, `manual_operation_signals`), the optional `verdict.final_grade`, and grade `F` — none of the additions are `required`, and `additionalProperties: false` is preserved. Nested record `$defs` keep `additionalProperties: true` (except `environmentVariable`, which is `false` so values can never sneak in) to stay non-brittle. The example JSON must validate against the schema. Prefer additive optional fields over changing the core schema.

### 9. Fan-out merge contract

The skill may run linearly or fan out across six read-only subsystem subagents (see SKILL.md "Parallel execution: subsystem fan-out"). When editing that section, preserve three invariants: (a) the schema is the merge contract — each subagent returns a fragment validating against its slice, and the orchestrator validates the merged whole; (b) subagents are read-only and the orchestrator is the only writer (and the only applier of `--apply-safe-harness-patches`); (c) the subagent-to-schema-slice ownership map stays collectively exhaustive over dimensions A1-A10 and B1-B10 and over the top-level array keys — **including every v0.2 survey array**. Changing the schema and changing the ownership map must stay in lockstep.

## Structural validation checklist

Run these checks before committing changes to this package:

1. `just list-skills`
2. Parse all JSON templates and confirm the schema validates as a Draft 2020-12 schema:
   - `templates/assessment-report.schema.json`
   - `templates/assessment-latest.json`
   - `templates/codebase-affordance-record.json`
3. Validate `templates/assessment-latest.json` against `templates/assessment-report.schema.json`.
4. Compare `templates/canonical-boundary.txt` with every shipped surface that states the canonical sentence.
5. Grep shipped surfaces for private-source/source-ID patterns listed above.
6. Grep templates for malformed placeholders.
7. Confirm example reports have no unresolved placeholders.
8. Confirm no generic core `backpressure` command key appears in shipped surfaces.
9. Confirm no `supersede`/`successor`/`legacy` replacement framing appears in shipped surfaces.
10. Confirm `templates/summary.md` is terminal-sized and the filled examples carry no unresolved placeholders.
11. Confirm the fan-out subagent→schema-slice ownership map covers every v0.2 survey array (collectively exhaustive).

## Extension guidance

Keep v0.2 focused. Add new report fields only when they are needed by a named downstream consumer. Prefer additive optional fields over changing the core schema.
