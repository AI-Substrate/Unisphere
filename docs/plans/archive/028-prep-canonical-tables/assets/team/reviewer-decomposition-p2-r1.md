# Reviewer packet — Plan 028 phase-2 decomposition review (guide v3), round 1

**From** PM `pij-native-tick` · **Reviewer** `pij-panicky-anteater` · **Date** 2026-09-29

**You own (write only these):**
- Report: `/Users/jordanknight/substrate/unisphere/unisphere-prep/docs/plans/028-prep-canonical-tables/assets/reviews/decomposition-p2-r1.md`
- Receipt proposal: `/Users/jordanknight/substrate/unisphere/unisphere-prep/docs/plans/028-prep-canonical-tables/assets/reviews/decomposition-p2-r1.receipt.dd.json`

**You may read:** your clone (`git fetch origin && git checkout --detach <subject>`), the adapter crates and their existing fixtures, the POC recipe script named in unit tk-000d, the consumer brief. Nothing else in the PM workspace except your two owned paths.

**Your job:** decide whether guide v3 (phase-2 decomposition) may be sealed on the current commit and its seven coder lanes released. There is no new contract code: `core::prep` v2 and the testkit fakes are unchanged since the phase-1 seal; the baseline for phase 2 is the verified phase-1 composition plus phase-1 acceptance records. The seal binds your receipt to this exact commit.

## Bindings

| Binding | Value |
|---|---|
| Scope | decomposition |
| Exact subject SHA | the commit adding this file (`git -C /Users/jordanknight/substrate/unisphere/unisphere-prep log -1 --format=%H -- docs/plans/028-prep-canonical-tables/assets/team/reviewer-decomposition-p2-r1.md`) |
| Product plan | `docs/plans/028-prep-canonical-tables/plan.dd.json` sha256 `7da3d4a4d05949ddba02368c80dbe145189a63214c43bd0335c381e36ce64e46` (changed since r2 only by phase-1 progress/proof links) |
| Implementation guide v3 | `docs/plans/028-prep-canonical-tables/assets/impl-guide.dd.json` sha256 `5ae69d772341f1753534f4f4dfee301a912155e7f4e2b9f11522dbd3f65864a4` |
| Phase-2 tasks | `docs/plans/028-prep-canonical-tables/assets/tasks/phase-2/tasks.dd.json` sha256 `84a537ef9e7ba9bde13c4bba20d2f54cd5bdb4ebb4ec5e6344e578211d54a63a` |
| Requested role | `{"role":"reviewer","harness":"omp","model":"github-copilot/claude-sonnet-5.5","source":{"harness":"guide","model":"guide"}}` |
| Structural check | `harness builder guide docs/plans/028-prep-canonical-tables --check` → ok with 26 write-overlap warnings (phase-1 historical PM units vs phase-2 lanes, sequential ownership) |

## What changed from v2

- Phase-1 units tk-0001…tk-0006 are PM-owned history (role pm); the coder roster is tk-0007 (OMP + Pi folds), tk-0008 (Codex), tk-0009 (Copilot CLI events + legacy snapshot), tk-000a (VS Code JSON + journal), tk-000b (Cursor transcript + IDE SQLite), tk-000c (snapshot prep loader), tk-000d (CLI: recipes, snapshot-limit flags, docs); PM composition tk-000e.
- Prep harness key = adapter catalogue descriptor id (phase-1 composition finding); each fold's pattern = the descriptor's session_glob; fold lanes flip their own descriptors' `cli_persisted_resume`.
- New real-corpus check vd-0017 (per-harness coverage); new risks rk-000c (Cursor IDE many composers per database), rk-000d (snapshot limits), rk-000e (descriptor-id keys); rk-0007 updated (DuckDB installed with Jordan's approval).
- Composition order: tk-000c, folds, tk-000d.

## What to assess

1. Are the seven lanes independently executable and provable against the unchanged contract (no hidden fold↔loader coupling, e.g. snapshot record keys and `native_key` conventions, JsonJournal reduction, SQLite table naming)?
2. Is the unchanged v2 contract sufficient for every representation (Snapshot input, nullable rows, per-source SessionFacts) or does any lane need a contract delta before seal? Assess the Cursor IDE many-sessions-per-database limitation.
3. Coverage of AC-0006, AC-0009, AC-000c, AC-000d: entrypoint, owner and executable proof for each; is vd-0017 plus fold tests adequate for "every registered harness ... explicit nulls with per-source coverage"?
4. Are the CLI lane's recipe output contract (`SET file_search_path`, `.read views.sql`, query) and the one-command DuckDB pipe sound?
5. Any reason not to seal and dispatch.

## Return

Report + `builder/team` DD receipt proposal: `id:"rv-028-decomposition-p2-r1"`, `recorded_at` later than every earlier decomposition review, `scope:"decomposition"`, subject SHA, plan/guide bindings above, observed runtime, verdict and findings. Validate with ddocs; message the PM with verdict, counts, paths and SHA-256.

## Boundaries

Read-only; write only your two files; no commits, fixes, pushes or merges; no real transcript content; stop if not running as the requested model.
