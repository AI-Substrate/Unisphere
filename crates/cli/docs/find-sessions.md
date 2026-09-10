# Find repository-associated sessions

## Motivating question

Which supported local stores provide evidence for this repository, and which sessions can be selected without confusing physical copies, reused native IDs, or unavailable stores?

## Prerequisites

Choose `--repo PATH` explicitly. `--repo .` inspects registered locations associated with that repository; it is not permission to recursively scan HOME. Use `--repo-scope exact`, `tree` (default), or `worktrees`. The last requires injected trusted Git capability.

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

## Creative use cases

- Audit why a known conversation is missing before assuming it was never recorded.
- Compare repository association evidence after moving a working tree.
- Select only `--source-adapter git-ai` while leaving excluded registrations unopened.
- Use explicit `--source PATH_OR_ID` for known evidence whose project association is unavailable.

## Limits and recovery

Zero sessions with unreadable sources is not the same as a complete zero-match view. Without `--allow-partial`, a requested source read failure is operational failure. With it, coverage remains partial. If worktree identity is ambiguous or Git is unavailable, supply the required trusted Git executable through the caller's composition or use `exact`/`tree` scope; blind retry cannot help.

**Next step:** bind one returned session ID and run `unisphere sessions tree "$SESSION_ID" --repo . --format json`.
