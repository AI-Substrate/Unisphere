---
record_kind: "retro"
harness_version: "0.14.0"
branch: "main"
repo: "https://github.com/AI-Substrate/Unisphere.git"
created_at: "2026-09-08T01:54:39.807Z"
agent: "pij-female-varl"
plan_id: null
schema_version: "1.2"
retro_id: "2026-09-08T01:54:39.807Z-unisphere-adapter-research-48e06d43bc36"
started_at: "2026-09-08T01:46:09.272Z"
ended_at: "2026-09-08T01:54:39.807Z"
summary: "Read-only telemetry research exposed a gap between safe structural inspection and generic file rendering, plus a failed model-selection batch; this record contains tooling-improvement feedback, not product progress."
entries:
  - id: DL-001
    kind: difficulty
    description: "Scout tooling lacks a direct bounded JSON/JSONL structure projection, while the generic reader rejects SQLite state.vscdb by extension; rendering native session files directly would expose unnecessary private content."
    target: tooling
    severity: degrading
    workaround: "Main projected keys/types/enums/counts in memory and opened SQLite with a read-only URI transaction; no private payloads were printed or retained. One scout used a supervised Python process, so the limitation is the direct read-only surface, not an absolute inability to execute a probe."
    suggested_encoding: "Add a bounded structure-only reader for JSON/JSONL and SQLite recognized by file signature, with explicit content exclusion and read-only queries."
    fp: "48e06d43bc36"
    disposition: kept
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-08T01:46:09.272Z"
---

# Retro — structural research tooling

The source observation also recorded six background scouts failing before any work
with `No model selected`; the user-requested retry succeeded. A resolved-model
preflight would expose that batch-wide prerequisite before allocating six jobs.
No authentication or global configuration change was made by this research.

Observed reader failure: `state.vscdb` was treated as binary and its query selector
was rejected, while SQLite `mode=ro` opened the same file successfully. The safe
projection returned schema, counts and key/type shapes, never text or secret values.
The projection also caught a semantic trap: VS Code `.jsonl` chat files contain
snapshot mutations, so file extension alone cannot select a source-event decoder.

The improvement is a safer evidence-gathering tool, not more process gates.
Only bucket `unisphere-adapter-research` is drained into this record.
Harness-engineering code and fixtures remain optional inspiration, not authority
over Unisphere's requirements or observed native formats.
