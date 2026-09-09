# eng-harness-0-harnessability-assessment

Assess how harnessable a repository is: how easily a human or agent can enter, operate, modify, observe, prove, and improve it through an engineering harness.

The assessment scores two axes — **Operate-Today** (can a fresh agent use the repo as it exists now?) and **Adaptability** (can the repo be safely and cheaply changed to create stronger harnesses and proof loops?) — and maps the repository's back-pressure surfaces, proof ceilings, external-dependency exposure, and highest-leverage improvements. Cold-start onboarding difficulty is one part of harnessability, not the whole assessment.

The agent harness drives. The engineering harness proves.

## What it produces

The skill writes a per-run history directory plus stable root "latest" files:

```text
.harness/reports/harnessability/<ordinal>-<slug>/report.md
.harness/reports/harnessability/<ordinal>-<slug>/report.json
.harness/reports/harnessability/<ordinal>-<slug>/summary.md
.harness/reports/harnessability/<ordinal>-<slug>/evidence.jsonl
.harness/reports/harnessability/latest.md
.harness/reports/harnessability/latest.json
.harness/reports/harnessability/schema.json
```

Every run overwrites the root `latest.*`/`schema.json` so the newest run is always at a stable path. The root `latest.json` is the sentinel the adoption flow (`/eng-harness-flow`) reads to decide whether an assessment already exists.

The Markdown report is for humans and agent skim-reading. The JSON report follows `templates/assessment-report.schema.json` (schema version `harnessability-assessment.v0.2`) and is for comparison, automation, and future skills. See `templates/assessment-latest.md` and `templates/assessment-latest.json` for sanitized examples.

v0.2 **surveys the repo's existing engineering environment first** — engineering flows (including SDD-like pipelines), pre-commit/local gates, CI/local equivalence, test mechanisms (mock vs fake/sink/stub/contract/testcontainer), external-dependency pressure, code composition, canonical-vs-diffuse harnesses, and advisory manual/IDE-only signals — then layers an **A–F assessment matrix** over the two axes and emits a terminal-sized `summary.md` alongside the detailed `report.md`. It prefers recommending **deterministic encoding** over context-file accretion: a context file is orientation, never proof.

## Default safety posture

Default mode is static and conservative. It reads repository files, inspects harness surfaces, classifies evidence versus inference, reports environment-variable names only, and writes assessment reports.

It does not install dependencies, boot services, mutate state, read secrets, call external services, perform auth flows, or edit product code by default. Optional flags (`--execute-safe-probes`, `--deep`, `--apply-safe-harness-patches`) widen the read-only or harness-only surface; product-code recommendations always stay proposal-only.

## Where it fits

```text
/eng-harness-flow (adopt) -> eng-harness-0-harnessability-assessment -> /eng-harness-flow (loop)
```

- The adoption flow (`/eng-harness-flow`) creates or validates the local harness nucleus.
- `eng-harness-0-harnessability-assessment` reports target-aware harnessability and next safe actions.
- The loop (`/eng-harness-flow`) operates: boot, observe, retro, harvest, and the advisory Backpressure Check.

The advisory Backpressure Check surveys whether enough deterministic sensors exist for scoped work. It is not itself proof, and this skill never introduces a generic core `backpressure` command. When a gap is found, it recommends the specific sensor, command, fixture, fake, sink, diagnostic, schema check, smoke path, architecture rule, or evidence capture that would prove the scoped work.

## Product-code affordances

Some repositories are not harnessable until the product exposes better local/test affordances. The report may propose changes such as a local/test auth path, seed/reset command, health endpoint, side-effect sink, stable UI hooks, or structured diagnostics.

Those recommendations are proposal-only by default. They are not applied by this skill unless a later, explicit implementation task is approved.

## Validation expectations

For this skill package, structural validation is enough:

- `just list-skills` discovers the skill;
- JSON templates parse and the schema validates as a Draft 2020-12 schema;
- the example JSON validates against the schema;
- shipped surfaces preserve the boundary sentence;
- shipped surfaces avoid private-source/source-ID leakage;
- placeholder syntax is well-formed where placeholders are allowed;
- no generic core backpressure command key is introduced.
