---
record_kind: "retro"
harness_version: "0.14.0"
branch: "builder/029-session-status"
repo: "https://github.com/AI-Substrate/Unisphere.git"
created_at: "2026-10-01T12:28:04.835Z"
agent: "pij-specific-kiwi"
plan_id: "029-session-status"
schema_version: "1.2"
retro_id: "2026-10-01T12:28:04Z-pij-specific-kiwi-029"
started_at: "2026-09-29T04:10:34Z"
ended_at: "2026-10-01T12:30:00Z"
summary: "Plan 029 session status: PM seat with three OMP Opus 5.5 coders and a Sonnet 5.5 reviewer. Phase 1 (Claude) went through decomposition review (r1 changes-requested, r2 approved), seal, two-coder dispatch, composition and a real-machine proof; Pij dogfooded it live (Plan 157 cold-wake guard). Scope moved several times: Linux in, Windows in then out, then OMP/Codex/Copilot CLI (phase 2), solved by absorbing Plan 028's unlanded phase-2 folds instead of rewriting them. Composition review r1 found a path-traversal (fixed) and r2 approved; PR #10 merged to main as 0370cab."
entries:
  - id: DL-001
    kind: difficulty
    description: "Plan worktree has no node_modules; the repo-local ddocs writer is only reachable through the main checkout's absolute path."
    target: tooling
    severity: degrading
    workaround: "Called /…/unishpere-main/node_modules/.bin/ddocs with the plan path relative to the worktree."
    suggested_encoding: "harness builder new links or installs node_modules into the allocated worktree (or prints the ddocs path to use)."
    fp: "185580466ced"
    disposition: kept
  - id: DL-002
    kind: difficulty
    description: "harness commit 92fc17c: ingress connected but no refs/notes/ai note within 5000 ms; authorship unrecorded."
    target: infra
    severity: degrading
    workaround: "None; later commits verified. Recorded here."
    suggested_encoding: "harness commit retries the note wait once before reporting DEGRADED."
    fp: "94af94c5432e"
    disposition: kept
  - id: CONF-001
    kind: confusion
    description: "Builder skill team-lifecycle.md prescribes 'harness builder ack', but installed harness 0.14.0 has no ack command (dispatch: 'no acknowledgement gate')."
    target: skill
    severity: degrading
    workaround: "Kept coder AckReceipts as evidence under assets/team/acks; dispatch already granted scope."
    suggested_encoding: "Skill reads 'harness builder --help' and drops the ack step when the verb is absent."
    fp: "ce6e10854adf"
    disposition: kept
  - id: DL-003
    kind: difficulty
    description: "Coder follow-ups after compose --import had no harness import path; git cherry-pick bypassed harness commit attribution, which main's ruleset flags (extra approval for unattributed changes)."
    target: tooling
    severity: degrading
    workaround: "Cherry-picked; later follow-ups were fast-forwarded on top of the plan head instead."
    suggested_encoding: "harness builder compose --import accepts follow-up deliveries on an already-composed head (replay with attribution)."
    fp: "085b2facb414"
    disposition: kept
  - id: DL-004
    kind: difficulty
    description: "compose --verify refuses (E475 material drift) once a plan gains a phase after seal; no lighter path to verify a composed SHA without a reseal and re-import."
    target: tooling
    severity: degrading
    workaround: "Ran every guide check directly at the reviewed SHA; the composition reviewer accepted it (F-04)."
    suggested_encoding: "A phase-scoped guide v2 seal that keeps phase-1 imports valid, or compose --verify --phase."
    fp: "dc751c0cbf5b"
    disposition: kept
  - id: DL-005
    kind: difficulty
    description: "harness builder dispatch spawns pij with a bare '--bin omp'; current pij-rs refuses non-absolute --bin, so dispatch fails E473 after creating the clone. Also omp spawn refuses linked worktrees."
    target: tooling
    severity: degrading
    workaround: "Spawned the OMP coder in the created clone with pij-rs spawn, then dispatch --adopt-peer."
    suggested_encoding: "dispatch passes --harness omp without --bin (or an absolute path)."
    disposition: kept
  - id: DL-006
    kind: difficulty
    description: "Checks envelope embedded every passing gate's full test log (73 KB); CI-pinned harness 0.13.0 boot failed to parse >64 KiB on macOS (E_CHECKS_ENVELOPE). Then the grown boot proofs hit the 30-minute macOS job timeout."
    target: project
    severity: degrading
    workaround: "Bounded passing-gate output to a 2000-char tail (13.5 KB envelope); raised CI timeout to 45 min."
    suggested_encoding: "A checks test asserting the ok envelope stays under 32 KiB."
    disposition: fixed-now
  - id: INS-001
    kind: insight
    description: "Searching sibling branches before building saved a whole phase: Plan 028's approved phase-2 folds existed unlanded; merging them beat three from-scratch folds (a coder noticed first)."
    target: plan
    disposition: kept
  - id: WIN-001
    kind: win
    description: "Dogfooding by the real consumer (pij Plan 157) found the one perf defect (warm cost scaled with history) before review; the guard then refused a real cold wake of the prime ($4.82) the same day."
    target: project
    disposition: kept
---

# Retro — Plan 029 session status

Highest-value encodable lesson: DL-006 — guard the checks envelope size with a test so a growing test suite can never again break CI parsing silently.
