# Git-AI attribution evidence

## Motivating question

Which commits carry registered Git-AI attribution for this repository, and can that evidence be related to a supported session without inventing a transcript or timing?

## Prerequisites

Use a Git-AI-capable installed registration and an explicit repository. Keep native note inventory distinct from logical query filtering:

```sh
unisphere sessions list --adapter git-ai --repo .
unisphere sessions list --repo . --source-adapter git-ai --format json
```

The first is the existing native Git-note listing; `--adapter` is a native dispatch selector. The second is a logical session query; `--source-adapter` is the representation predicate. Mixed native/query selectors are invalid.

## Recipe 7 — attribution-assisted review

```sh
unisphere sources list --repo . --source-adapter git-ai --format json
unisphere sessions list --repo . --source-adapter git-ai --columns transcript_available --format json
unisphere sessions tree "$SESSION_ID" --repo . --source-adapter git-ai --columns parent_ids,branch_ids --format json
unisphere events list --repo . --source-adapter git-ai --session "$SESSION_ID" --format json
```

Expected: notes provide revision-qualified attribution/source facts and commit/range evidence supported by the registered adapter. A link to a transcript session exists only when verified namespace/provenance evidence supports it. Otherwise attribution remains source/event evidence with unavailable session/transcript/timing fields.

Declared attribution sessions are source-scoped rows with `transcript_available: false`;
legacy prompt/human records and unresolved references remain source/event-only.
Use `events extract --include-content --columns parts` to inspect supported commit
and range facts. No note supplies a message, turn, tool invocation or clock.

Interpretation: Unisphere supplies observed note and association facts. It does not prove the named agent authored every byte, that a transcript exists, that a session is complete, or how long work took. Git attribution cannot globalize a reused native ID or merge equal text.

## Limits and recovery

Git notes access requires the registered capability and a trusted standard Git executable supplied by composition. `UNI-QUERY-SOURCE-READ` with `git_unavailable` requires that executable before retry. Ambiguous repository/worktree identity requires explicit user input; do not choose by path prefix. Retired harness telemetry is unsupported and has no compatibility bridge.

Query files may be created in the checkout, but never inside the common or
per-worktree Git directories. Native Git Notes exports retain the stricter rule
that also excludes the checkout. If Git is admitted and its source directories
cannot be established, file output is refused even with `--allow-partial`:
linked worktrees can place those directories outside the selected repository.
Restore Git/source access or explicitly exclude `git-ai` to query other sources.
Selecting a different output directory cannot repair unknown source boundaries.

**Next step:** inspect the selected source/session coverage before using attribution in a human review.
