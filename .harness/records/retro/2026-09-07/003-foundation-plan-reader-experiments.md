---
record_kind: "retro"
harness_version: "0.14.0"
branch: "builder/001-sdk-cli-foundation"
repo: "https://github.com/AI-Substrate/Unisphere.git"
created_at: "2026-09-07T03:25:01.881Z"
agent: "pij-female-varl"
plan_id: "001-sdk-cli-foundation"
schema_version: "1.2"
retro_id: "2026-09-07T03:25:15Z-pij-female-varl-6f8cd71eb181"
started_at: "2026-09-07T03:18:53.276Z"
ended_at: "2026-09-07T03:25:15Z"
summary: "Authored and independently validated the prime-owned foundation product plan, completed a standard-first output workshop including pij metadata, and composed separately authorized gitignored readers. Shared-kernel fixture state collision was corrected and fixture checksums verified; real error smoke found and corrected a Copilot diagnostic payload leak before acceptance."
entries:
  - id: DL-001
    kind: difficulty
    description: "Parallel task agents shared the persistent eval kernel and a generic fixture_bytes variable was overwritten between reader tasks."
    target: tooling
    severity: degrading
    workaround: "Agents used reader-prefixed/local variables, repaired copied fixtures, and Main verified SHA-256 provenance before accepting tests."
    suggested_encoding: "Isolate task eval state by agent or require lexical/task-scoped fixture transforms in the runner."
    fp: "6f8cd71eb181"
    disposition: kept
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-07T03:18:53.276Z"
  - id: INS-001
    kind: insight
    description: "A Result-returning Rust example main Debug-formatted FromUtf8Error and exposed input bytes even though its success path was counts-only."
    target: project
    disposition: fixed-now
    system:
      compound:
        status: encoded
        source: agent-self
        first_seen_at: "2026-09-07T03:24:01Z"
---

# Retro — Foundation plan and reader experiments

Product plan: 11 unchecked ACs, one outcome checkpoint; structural checks passed, two independent critics found no material issues. Product readiness is not implementation/dispatch readiness; the real guide and tasks remain outstanding.

Scratch proof: 68 tests and clippy passed after the error-rendering correction; valid fixture smoke read Claude35, omp193 and Copilot28 records. Invalid-UTF8 smoke now exits1 with empty stdout and no payload byte array for all readers. The added Copilot regression checks exercise the real example code without recursively running Cargo.

Output workshop: Standard OTel/GenAI fields first, namespaced pij identity/role/lineage and historical provenance as proposals; three JSON illustrations parse and six relative links resolve. Status Review/Preferred Direction, not an approved schema or Collector interoperability claim.

Detailed proof is in docs/plans/001-sdk-cli-foundation/assets/research/scratch-reader-proof.md. Scratch stays ignored and must be retained or explicitly preserved before worktree retirement; no shipping source imports it. No pushes, native private stores, global settings or other peers' observation buckets were touched.
