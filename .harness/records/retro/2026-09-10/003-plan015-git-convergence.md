---
schema_version: "1.2"
retro_id: "2026-09-10T22:13:10.895Z-pij-empirical-tiger-git-convergence"
agent: pij-empirical-tiger
plan_id: 015-session-query-cli
started_at: "2026-09-10T22:09:10.877Z"
ended_at: "2026-09-10T22:13:10.895Z"
summary: "History-preserving Git Notes convergence exposed and fixed a shared output-safety gap; exact final proof remains separately receipted."
entries:
  - id: DL-001
    disposition: fixed-now
    kind: difficulty
    description: "Git query reused generic staging and could create a file in an admitted Git store; real linked-worktree output regression exposed the gap. Shared pure native/query root policies plus pre-staging source-boundary proof now protect both stores without forbidding normal checkout query output."
    severity: degrading
    fp: 2e269400a967
    target: security
    system:
      compound:
        status: encoded
        source: agent-self
        first_seen_at: "2026-09-10T22:09:10.877Z"
        resolved_by: "71182b502b1d5a63a2237237b117580cdc19e2f5:crates/app/tests/git_query.rs"
system:
  compound:
    bubble_action: "all-save"
---

The permanent real-Git regression checks common/per-worktree store roots, linked worktrees, source-ID and repository scopes, Git present/absent, partial-read mode and unchanged directory entries. Positive tests prove checkout output and Git-less non-Git output still succeed. Destination validation precedes staging; unknown boundaries fail closed only for admitted Git sources. Native export keeps the stricter full-triple policy through the same pure core predicate.

Next useful improvement: run the same nonmutation matrix for every future source-store adapter instead of inferring that create-new publication is source-safe.
