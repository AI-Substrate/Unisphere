---
record_kind: "retro"
harness_version: "0.14.0"
branch: "poc/prep-duckdb"
repo: "https://github.com/AI-Substrate/Unisphere.git"
created_at: "2026-09-28T14:32:38.230Z"
agent: "pij-peculiar-maliketh"
plan_id: null
schema_version: "1.2"
retro_id: "2026-09-28T14:32:38Z-pij-peculiar-maliketh-poc-prep"
started_at: "2026-09-28T13:48:00Z"
ended_at: "2026-09-28T14:35:00Z"
summary: "Sole-agent POC: built `unisphere prep` (Claude Code fold, incremental cursors/anchors, Parquet store), matched extract.py exactly on the RCA reference numbers, and compared DuckDB/DataFusion/SQLite over the same tables. Verdict and numbers live in the untracked scratch FINDINGS.md."
entries:
  - id: DL-001
    kind: difficulty
    description: "~/.claude/projects contains final symlinks into ~/.claude-alt; the no-follow loader policy silently diverges from glob-based tools by one source and prep reports no skipped-symlink count"
    target: project
    severity: degrading
    workaround: "Found by row-level parity against extract.py; documented as a finding"
    suggested_encoding: "Add a skipped_symlinks counter to prep discovery and a fixture with a symlinked transcript"
    fp: "b6929e8c032a"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-28T14:29:26.041Z"
  - id: DL-002
    kind: difficulty
    description: "Base 728dc13 fails cargo fmt --check (crates/cli/src/sessions.rs:111), so harness checks stops at the format gate before any other gate"
    target: project
    severity: annoying
    workaround: "Separate style commit ce16900 on the POC branch"
    suggested_encoding: "Run harness checks before landing default-limit fixes"
    disposition: fixed-now
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-28T14:29:25.000Z"
  - id: DL-003
    kind: difficulty
    description: "Column names input/output/offset/at collide with DuckDB keywords; DataFusion 54 parses 'a IS NOT DISTINCT FROM b AND c' as 'a IS NOT DISTINCT FROM (b AND c)'"
    target: tooling
    severity: annoying
    workaround: "Renamed offset to native_offset, AS on aliases, parenthesised predicates"
    suggested_encoding: "Keyword-free column names in the prep table contract"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-28T14:29:26.200Z"
  - id: INS-001
    kind: insight
    description: "Measured embed cost contradicts the engine research: DataFusion 55 (minimal features) clean release 367 s / 125 MB vs bundled DuckDB 127 s / 41 MB; Parquet writer alone adds 35 s / 5.7 MB to the app"
    target: project
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-28T14:25:00Z"
---

# Retro — POC prep → Parquet → SQL

Highest-value lesson: the row-level parity script (`scratch/poc-prep-duckdb/parity.py`) found the only real divergence
(symlinked sources). Encode it as a harness sensor: a prep smoke that compares window totals against the pinned
reference numbers for file F and the corpus.
