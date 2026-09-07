---
schema_version: '1.2'
retro_id: 2026-09-07T00:39:40.920Z-pij-right-kotallo-8c72
agent: pij-right-kotallo
plan_id: null
started_at: '2026-09-07T00:39:14.767Z'
ended_at: '2026-09-07T00:39:40.920Z'
summary: Fixed the scratch ignore template self-ignore encountered during the scoped
  adoption commit; protection is trackable and runtime scratch remains ignored.
entries:
- id: DL-001
  kind: difficulty
  description: CLI-generated scratch .gitignore ignores itself, so explicit scoped
    onboarding commit rejected the protection file
  target: tooling
  severity: annoying
  workaround: Make the protection file trackable while keeping all session scratch
    ignored
  suggested_encoding: Generate a !.gitignore exception in the scratch ignore template
    so new clones inherit protection
  fp: 8c1402b324d4
  system:
    compound:
      status: encoded
      source: agent-self
      first_seen_at: '2026-09-07T00:39:14.767Z'
      resolved_by: .harness/temp/.gitignore
  disposition: fixed-now
system:
  compound:
    bubble_action: all-save
---

# Scratch protection

`git check-ignore --no-index --quiet` returned exit 1 for the protective `.gitignore` and exit 0 for collector metadata and the session buffer. Only the protection file is staged. No force-add and no global ignore settings were required.
