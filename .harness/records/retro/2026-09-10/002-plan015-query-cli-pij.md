---
schema_version: "1.2"
retro_id: "2026-09-10T10:00:10.744Z-pij-empirical-tiger-query-cli"
agent: pij-empirical-tiger
plan_id: 015-session-query-cli
started_at: "2026-09-10T08:07:27.534Z"
ended_at: "2026-09-10T10:00:10.744Z"
summary: "Preserved two integration lessons; non-Git query and Pij implementation composed, with Git Notes history convergence still awaiting explicit merge authorization."
entries:
  - id: DL-001
    disposition: kept
    kind: difficulty
    description: "Exact-source boot capture exceeded the Eval240s timeout; VM state reset before receipt persistence. Determine child state and rerun through supervised captured execution, never infer success."
    severity: degrading
    fp: 8cf5835cd6e3
    suggested_encoding: "Add durable stdout/stderr spool paths and before/after source identity to the standard proof runner so a tool timeout or truncated envelope cannot lose executed evidence."
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-10T08:07:27.534Z"
  - id: DL-002
    disposition: fixed-now
    kind: difficulty
    description: "Actual installed-style query smoke caught CLI min-duration producing Unsigned while schema requires FiniteF64; parser-only catalogue coverage did not catch SDK interoperability."
    severity: degrading
    fp: 8428a31427bb
    target: project-sensor
    suggested_encoding: "Exercise real native data through built and installed query CLIs inside the existing native proof instead of relying on parser/FakeQueryApi checks."
    system:
      compound:
        status: encoded
        source: agent-self
        first_seen_at: "2026-09-10T09:28:07.946Z"
        resolved_by: "f818e2f5e6c4e5fb34ab8a00c34cc9928dc0d283:crates/testkit/src/bin/proof/query.rs"
system:
  compound:
    bubble_action: "all-save"
---

The existing native proof now reuses its installed/built binaries for query lineage, time/text privacy, context, statistics, decimal duration filtering, offline identity and bundled documentation. Actual successful targeted execution is retained in plan assets/verification/installed-query-working-proof.json; final committed boot is recorded separately rather than inferred.

Only this PM's observation bucket is eligible for clearing after this record validates. Original failed proof attempts and withdrawn review findings remain preserved.
