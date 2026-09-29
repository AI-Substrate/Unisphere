# Plan 028 post-flight: prep canonical tables

**PM** `pij-native-tick` · **Prime** `pij-suspicious-meadowlark` · **Date** 2026-09-29 · **Branch** `builder/028-prep-canonical-tables`

## What finished

`unisphere prep --target DIR` writes incremental, idempotent Parquet canonical tables for every catalogued session harness:

- **Append JSONL:** Claude Code, Oh My Pi, Pi, Codex, Copilot CLI events, Cursor transcripts.
- **Snapshots:** the Copilot CLI legacy document, VS Code documents and journals, and Cursor IDE SQLite. Each source is read again whole when its file stat changes, and folded only when its content revision changes.

Around the tables, the plan also delivered:

- the DuckDB views (`views.sql`);
- `prep record` native-address fetch, for append offsets, snapshot keys and JSON pointers;
- `prep compact`;
- 24 research recipes (`prep recipes` and `prep recipe NAME`);
- the `prep` and `research-recipes` docs topics;
- the SDK `Preparer` and `fold_source`, which Plan 029 reuses;
- boot proofs through the built CLI, the installed CLI and an external SDK consumer.

Two changes came from the prime's composition rulings:

- **Incremental discovery.** A per-directory mtime index is persisted, so only changed directories are listed again.
- **Commit waves.** `--max-run-bytes`, default 256 MiB, bounds the rows held before each durable commit.

## Tested and reviewed artifacts

| Phase | Verified artifact | Checks | Independent review |
|---|---|---|---|
| 1 (Claude) | `d80094913d16f68cefbca128015531c5aa3fc197` | vd-0008, vd-0015 exit 0 | `rv-028-composition-p1-r1` approved (low finding closed) |
| 2 (all harnesses, recipes) | `19ace79fd493350525323dd5da0cff8e3d3a020c` (receipt `9a09aab`) | vd-0008, vd-0015 exit 0 | `rv-028-composition-p2-r1` approved, 0 findings |

The reviewer for both rounds was `pij-panicky-anteater`, running `github-copilot/claude-sonnet-5.5` in its own clone. It re-ran the proofs and tests itself.

## Proof pointers

All 13 acceptance criteria are checked, and each `proven_by` link resolves to an entry in `assets/execution-log.dd.json`:

- **Phase 1:** lg-0003, lg-0004, lg-0005, lg-0007.
- **Phase 2:** lg-000a (recipes), lg-000e (full-corpus measurement), lg-000f (coverage, parity, live), lg-0011 (review and docs).

The numbers-only evidence in `assets/evidence/` is summarised below.

| Evidence file | Check | Result |
|---|---|---|
| `parity-dfb8bf8.json` | vd-000a | 55,607 of 55,607 call rows; the phase-1 accepted divergences are unchanged. |
| `live-4074389.json` | vd-0016 | 344 runs during concurrent writes with 0 failures; totals equal a fresh prep. The fleet check ran 10 times with 0 failures. |
| `coverage-dfb8bf8.json` | vd-0017 | 9 of 9 sets supported across 5,423 sources. Two empty VS Code documents are reported unreadable. |
| `measure-dfb8bf8.json` | vd-000b | See the three runs below. |
| `recipes-9820bb5.json` | vd-000c | 24 recipes with 0 failures, on both the synthetic and the real target. |

The three vd-000b runs, measured at load average 73–109:

| Run | Result |
|---|---|
| Cold | 13.99 GB read in 30.6 s; peak RSS 1.76 GB; 57 commits |
| Unchanged | 1.42 s (0.42 s CPU); 0 of 14,707 directories listed; 600 MB RSS |
| Append | 0.66 s |

Earlier measurements are kept for comparison: `measure-d800949.json` (Claude only, phase 1), `measure-6161ba6.json` and `coverage-9820bb5.json` (before the rulings).

## Open or deferred (named, not hidden)

- **Memory.** Peak memory is the committed state held in full (58.6 MB `state.json`, about 0.6 GB parsed, stored as serde Value checkpoints) plus one wave. The reviewer accepted this as bounded by documented, adjustable limits. Going lower would need a compact checkpoint format, which would be a follow-up plan.
- **VS Code call timestamps.** Current VS Code session documents have no `responseTimestamp`, so call `ts` is null for most VS Code calls; request time is on the trigger. This is documented. Deriving call time from `result.timings` would be a fidelity follow-up.
- **Empty VS Code documents.** Zero-byte documents are reported `unreadable`, so every run on this machine exits 3. This is documented.
- **Stale guide text.** The guide's `rk-000b` text still describes whole-run buffering. It was resolved by commit waves and recorded in `meta.updated`; other guide sections are treated as material drift, so they were left unchanged.
- **Tidy not run.** No workspace was retired. The coder clones `unisphere-028-p{1,2}-*`, the review clone `unisphere-028-review` and this plan worktree remain. Coder deliveries are preserved at `refs/preserve/plan-028/tk0002` through `tk0005` and `tk0007` through `tk000d`.

## Remaining human calls (Jordan, via the prime)

- **Ship.** Push and PR are not done. Local `main` is 42 commits ahead of `origin` and 4 behind. The prime will bring Jordan one landing plan that covers 028 and 029; 029 rebases onto the archived 028 head.
- **Upstream issues.** The 9 builder issues from the phase-1 and phase-2 retros (DL entries) have not been filed on AI-Substrate/harness-engineering.

## Observations and highest-leverage improvement

The retros are in `.harness/records/retro/2026-09-29/`: `001-…-phase-1.md` and `002-…-phase-2.md`. The post-flight harvest (`harness retro insights --plan 028-prep-canonical-tables`) found 2 records and 15 entries, all open. The largest cluster is `difficulty/harness-itself` with 9 entries, flagged as a proof gap.

**Highest leverage.** Builder should model a baseline and a composition receipt per phase. Four of those entries share this root:

- advance gate E475 after re-seal;
- second import E472;
- dispatch readiness E471;
- guide and plan drift rules.

Together they cost three re-reviews and a receipt rename on this plan.

**Encodable in this repo.** Run the numbers-only real-corpus scripts (coverage and measure) when a lane is delivered, not only at composition. Phase 2's three real-corpus defects (record limit, warm walk, memory) were invisible to the synthetic lane tests.
