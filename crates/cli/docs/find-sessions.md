# Find repository-associated sessions

## Motivating question

Which supported local stores provide evidence for this repository, and which sessions can be selected without confusing physical copies, reused native IDs, or unavailable stores?

## Prerequisites

Choose `--repo PATH` explicitly for repository discovery. `--repo .` inspects registered locations associated with that repository; it is not permission to recursively scan HOME. Use `--repo-scope exact`, `tree` (default), or `worktrees`. The last requires injected trusted Git capability. As an optional alternative, `--pij ID` asks the application to resolve one seat through an installed, available Pij CLI before opening a native source; ordinary queries do not require Pij.

## Recipe 1 — discover, diagnose, select

```sh
unisphere sources list --repo . --format json
SOURCE_ID='q1:source:1111111111111111111111111111111111111111111111111111111111111111'
unisphere sources check --source "$SOURCE_ID" --format json
unisphere sessions list --repo . --harness claude --format json
SESSION_ID='q1:session:2222222222222222222222222222222222222222222222222222222222222222'
unisphere sessions show "$SESSION_ID" --repo . --format json
```

Expected: source results distinguish readable, absent, unreadable, unsupported, partial, unassociated and conflicting facts. Session results retain every supporting source reference and use local `q1:` identities. Two source rows can support one session; equal native IDs or equal text do not merge entities by themselves.

Interpretation: Unisphere supplies association basis, revisions and coverage. A human decides whether the selected evidence answers the review question. A location hint, matching basename, remote URL, or prompt mention is not repository proof.

## Recipe 2 — select the latest native session recorded for a Pij seat

```sh
unisphere sessions show --pij pij-example-seat --format json
unisphere tools extract --pij pij-example-seat --tool-family shell --format jsonl
```

Expected: each new command resolves the seat's latest recorded native harness and session, verifies a local native source, then executes through the normal query path. A retired seat remains usable when its native session mapping and transcript still exist. Resolution provenance is written to the diagnostic channel; JSONL rows remain data-only.

Interpretation: the resolved native source and session are pinned for that operation and any continuation. A later command resolves the seat again, so a new Pij incarnation can select a different native session without mixing identities inside one query.

Limits: `--pij` is an alternative source/session identity. Do not combine it with `--repo`, `--source`, `--input`, `--session`, `--native-id`, harness/adapter selectors, or a nondefault `--repo-scope`. Pij lookup is latest-only and local to the selected Pij instance: Unisphere does not cache aliases, reconstruct seat history, search federated stores, fetch remote transcripts, install/start Pij, or revive seats. If Pij is unavailable, use explicit native selectors or install/repair it separately.

## Creative use cases

- Audit why a known conversation is missing before assuming it was never recorded.
- Compare repository association evidence after moving a working tree.
- Select only `--source-adapter git-ai` while leaving excluded registrations unopened.
- Use explicit `--source PATH_OR_ID` for known evidence whose project association is unavailable.

## Limits and recovery

Zero sessions with unreadable sources is not the same as a complete zero-match view. Without `--allow-partial`, a requested source read failure is operational failure. With it, coverage remains partial. If worktree identity is ambiguous or Git is unavailable, supply the required trusted Git executable through the caller's composition or use `exact`/`tree` scope; blind retry cannot help.

**Next step:** bind one returned session ID and run `unisphere sessions tree "$SESSION_ID" --repo . --format json`, or use `--pij ID` when one current Pij mapping is the intended selector.
