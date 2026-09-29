# Reviewer packet — Plan 028 decomposition review, round 2 (baseline for seal)

**From** PM `pij-native-tick` · **Reviewer** `pij-panicky-anteater` (same independent seat as round 1) · **Date** 2026-09-29

**You own (write only these):**
- Report: `/Users/jordanknight/substrate/unisphere/unisphere-prep/docs/plans/028-prep-canonical-tables/assets/reviews/decomposition-r2.md`
- Receipt proposal: `/Users/jordanknight/substrate/unisphere/unisphere-prep/docs/plans/028-prep-canonical-tables/assets/reviews/decomposition-r2.receipt.dd.json`

**You may read:** your clone `/Users/jordanknight/substrate/unisphere/unisphere-028-review` (`git fetch origin && git checkout --detach <subject>`), plus the inputs round 1 allowed. Nothing else in the PM workspace except your two owned paths.

**Your job:** decide whether the committed contract baseline may be sealed and released to the four phase-1 coder lanes. The seal binds your receipt to this exact commit, so the subject must be the commit that adds this packet.

## Bindings

| Binding | Value |
|---|---|
| Scope | decomposition |
| Exact subject SHA | the commit adding this file (`git -C /Users/jordanknight/substrate/unisphere/unisphere-prep log -1 --format=%H -- docs/plans/028-prep-canonical-tables/assets/team/reviewer-decomposition-r2.md`) |
| Product plan | `docs/plans/028-prep-canonical-tables/plan.dd.json` sha256 `e1872f2cc578e37b275fad3b3057c05b38ed9f7e719de360696c32bbcf5d6f86` (unchanged) |
| Implementation guide v2 | `docs/plans/028-prep-canonical-tables/assets/impl-guide.dd.json` sha256 `34d67d70b4f3a8fcbc4a59bd93696c62ca7653533da0d1a85a951fa0d59d8dba` |
| Phase-1 tasks | `docs/plans/028-prep-canonical-tables/assets/tasks/phase-1/tasks.dd.json` sha256 `2c5608160a6f0c1f874c68f9920eb490c116bd1ec509a41c248500807590f74f` |
| Requested role | `{"role":"reviewer","harness":"omp","model":"github-copilot/claude-sonnet-5.5","source":{"harness":"guide","model":"guide"}}` |
| Round-1 record | `assets/team/review-decomposition-rv-028-decomposition-r1.dd.json` (changes-requested; F-01, F-02, F-03) |
| PM evidence at this subject | `harness checks --json` → `ok` on the working tree that became this commit; `cargo test --locked -p unisphere-core --test prep_contract` → 6 passed; synthetic smoke of `unisphere prep` (new → unchanged 0 bytes → partial tail pending → appended; symlink + hidden counted) |

## What changed since round 1

- Guide v2 (`meta.updated` lists the deltas): F-01 → `PrepSourceStatus::Skipped` (explicit-scope exclusion, state kept) added beside `Missing`; F-02 → row nullability made part of the contract (all timestamps, counters, ids, native address `Option`); F-03 → `vd-0006` added to `cp-000b`. Baseline-authoring deltas the PM found while writing code: `PrepFold::open(meta, source, generation, saved)`, `fold_source(loader, fold, stat, source, generation, resume, options, limits, sink)`, turn origin uses the reference parser's kebab vocabulary, `PrepRecordRequest.max_bytes`, per-source policy compatibility via `checkpoint.policy`.
- Contract baseline (guide unit tk-0001): `crates/core/src/prep.rs` v2, `crates/core/tests/prep_contract.rs`, `crates/testkit/src/prep.rs` (MemoryPrepStore, MemoryLoader, RecordingFold), arch-check allowlist, output-prep dev-deps + Cargo.lock.
- Compile-level adaptation of the POC implementations that the wave-1 lanes will own: `crates/{sdk,loader-jsonl,adapter-claude,output-prep,cli}/src/prep*.rs`, `crates/app/src/main.rs`. `ParquetPrepStore::compact` returns `Unsupported` until tk-0004 delivers it.
- Flow spine expanded with phase 2 (flow files only).

## What to assess

1. F-01/F-02/F-03 closed as dispositioned? Evidence: guide v2 contract text and the code.
2. Does the committed `core::prep` match the guide v2 contract closely enough to freeze (names, signatures, nullability, vocabularies)? List any mismatch that would force a post-seal contract delta.
3. Are the testkit fakes sufficient for each wave-1 lane to prove its behaviour independently (engine: append/partial tail/rotate/rewrite/truncate/unreadable/snapshot/injected commit failure; CLI: fake PrepApi is trivially implementable against the port)?
4. Is the POC adaptation genuinely compile-level (no hidden new semantics the lanes must discover), and is ownership of each adapted file unambiguous per the guide's unit paths?
5. Any reason not to seal and dispatch.

## Return

Same as round 1: report + `builder/team` DD receipt proposal with `id:"rv-028-decomposition-r2"`, a `recorded_at` strictly later than round 1, `scope:"decomposition"`, the subject SHA, `plan` and `guide` path/sha256 bindings above, your observed runtime, verdict and findings (`disposition:"open"` unless you verified a fix; for round-1 findings you verify as fixed, list them with `disposition:"fixed"` and `evidence`). An approved receipt may contain only `low` findings left open. Validate with ddocs, then send the PM one pij message with verdict, counts and both paths + SHA-256.

## Boundaries

Unchanged from round 1: read-only; write only your two files; no commits, fixes, pushes or merges; no real transcript content in outputs; stop and say so if not running as the requested model.
