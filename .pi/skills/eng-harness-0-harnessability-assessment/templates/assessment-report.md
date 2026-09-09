<!-- foundations: first-principles#11, #15, #21, #22, #23, directives#D2-D6 -->

# Harnessability Assessment — {{REPO_NAME}}

Run metadata
- Timestamp: {{TIMESTAMP_UTC}}
- Repo root: {{REPO_ROOT}}
- Branch / commit: {{BRANCH}} / {{COMMIT_OR_UNKNOWN}}
- Mode: {{MODE}}
- Commands executed: {{COMMANDS_EXECUTED}}
- Commands skipped: {{COMMANDS_SKIPPED}}
- Safety notes: {{SAFETY_NOTES}}

## Verdict

- Operate-Today: {{OPERATE_TODAY_GRADE}} ({{OPERATE_TODAY_PERCENT}}%)
- Adaptability: {{ADAPTABILITY_GRADE}} ({{ADAPTABILITY_PERCENT}}%)
- Harnessability Index: {{HARNESSABILITY_INDEX}}
- Final grade: {{FINAL_GRADE}}
- Readiness: {{READINESS_LEVEL}}
- Highest proof level detected: {{HIGHEST_PROOF_LEVEL}}
- Target next proof level: {{TARGET_PROOF_LEVEL}}
- Confidence: {{CONFIDENCE}}

> `final_grade` augments — it never replaces — the Operate-Today / Adaptability tuple above, which stays primary. A weak axis is always reported alongside the headline grade.

## Assessment matrix

| Area | Grade | Score | Rationale |
|------|-------|------:|-----------|
{{ASSESSMENT_MATRIX_TABLE}}

## Plain-English assessment

{{PLAIN_ENGLISH_ASSESSMENT}}

## Top blockers

{{TOP_BLOCKERS}}

## Highest-leverage improvements

{{HIGHEST_LEVERAGE_IMPROVEMENTS}}

## First safe agent session plan

{{FIRST_SAFE_SESSION_PLAN}}

## Harness surfaces

| Surface | Path | Kind | Status | Notes |
|---------|------|------|--------|-------|
{{HARNESS_SURFACES_TABLE}}

## Repository topology

{{REPOSITORY_TOPOLOGY}}

## Existing engineering environment survey

> Surveyed before scoring — what the repo already has comes first.

### Engineering flows

| Flow | Kind | Commands | Canonical | Where detected |
|------|------|----------|-----------|----------------|
{{ENGINEERING_FLOWS_TABLE}}

### Pre-commit and local gates

| Gate | Mechanism | Checks | Local | CI-equivalent |
|------|-----------|--------|-------|---------------|
{{PRE_COMMIT_GATES_TABLE}}

### CI / local equivalence

| Check | CI command | Local command | Equivalence | Notes |
|-------|-----------|---------------|-------------|-------|
{{CI_LOCAL_EQUIVALENCE_TABLE}}

### Existing harness concepts (canonical vs diffuse)

| Concept | Kind | Surfaces | Coverage | Notes |
|---------|------|----------|----------|-------|
{{EXISTING_HARNESS_CONCEPTS_TABLE}}

### Test mechanisms

| Mechanism | Type | Deterministic | Where detected |
|-----------|------|---------------|----------------|
{{TEST_MECHANISMS_TABLE}}

### External-dependency pressure

| Dependency | Pressure | Local substitute | Proof impact |
|------------|----------|------------------|--------------|
{{EXTERNAL_DEPENDENCY_PRESSURE_TABLE}}

### Code composition and seams

| Area | Kind | Test seam | Coupling |
|------|------|-----------|----------|
{{CODE_COMPOSITION_TABLE}}

### Deterministic-encoding opportunities

| Opportunity | Current encoding | Proposed encoding | Proof level |
|-------------|------------------|-------------------|-------------|
{{DETERMINISTIC_ENCODING_OPPORTUNITIES_TABLE}}

### Manual / IDE-only signals

> Advisory. Influences A4/A5/A7/A8/A9/B5/B10 only; never over-penalises desktop, mobile, hardware, or brownfield topologies.

| Signal | Kind | Influences | Penalize |
|--------|------|------------|----------|
{{MANUAL_OPERATION_SIGNALS_TABLE}}

### Candidate first harness surfaces

> Derived AFTER the survey above — never before existing flows and commands are inventoried.

| Surface | Rationale | Proof level | Already exists | Priority |
|---------|-----------|-------------|----------------|----------|
{{CANDIDATE_FIRST_HARNESS_SURFACES_TABLE}}

## Axis A — Operate-Today scorecard

| Dimension | Band | Points | Evidence | Notes |
|-----------|------|-------:|----------|-------|
{{AXIS_A_SCORECARD}}

## Axis B — Adaptability scorecard

| Dimension | Band | Points | Evidence | Notes |
|-----------|------|-------:|----------|-------|
{{AXIS_B_SCORECARD}}

## Back-pressure surface inventory

{{BACKPRESSURE_SURFACE_INVENTORY}}

## Scenario probes

{{SCENARIO_PROBES}}

## Command tiers

| Tier | Command or check | Status | Proof | Notes |
|------|------------------|--------|-------|-------|
{{COMMAND_TIERS_TABLE}}

## Services, environment, and remote dependency exposure

{{SERVICES_ENVIRONMENT_DEPENDENCIES}}

## State, fixtures, reset, and cleanup

{{STATE_FIXTURES_RESET}}

## Observability and evidence

{{OBSERVABILITY_AND_EVIDENCE}}

## Codebase affordance recommendations

{{CODEBASE_AFFORDANCE_RECOMMENDATIONS}}

## Harness-only recommendations

{{HARNESS_ONLY_RECOMMENDATIONS}}

## Onboarding consolidation notes

{{ONBOARDING_CONSOLIDATION_NOTES}}

## Human questions

{{HUMAN_QUESTIONS}}

## Evidence and inference log

| Source | Provenance | Claim |
|--------|------------|-------|
{{EVIDENCE_AND_INFERENCE_LOG}}
