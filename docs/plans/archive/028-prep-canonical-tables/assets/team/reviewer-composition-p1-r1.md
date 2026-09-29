# Reviewer packet — Plan 028 phase-1 composition review, round 1

**From** PM `pij-native-tick` · **Reviewer** `pij-panicky-anteater` (independent seat, same as decomposition) · **Date** 2026-09-29

**You own (write only these):**
- Report: `/Users/jordanknight/substrate/unisphere/unisphere-prep/docs/plans/028-prep-canonical-tables/assets/reviews/composition-p1-r1.md`
- Receipt proposal: `/Users/jordanknight/substrate/unisphere/unisphere-prep/docs/plans/028-prep-canonical-tables/assets/reviews/composition-p1-r1.receipt.dd.json`

**You may read:** your clone `/Users/jordanknight/substrate/unisphere/unisphere-028-review` (`git fetch origin && git checkout --detach <subject>`); the numbers-only evidence under `docs/plans/028-prep-canonical-tables/assets/evidence/` at the subject; the reference/brief inputs named in `original-ask.md`. You may run read-only commands, tests and `harness builder on-track` in your clone. Do not read or copy real transcript content; real-corpus proofs were run by the PM into gitignored scratch and only numbers were committed.

**Your job:** independently review the phase-1 **composition** — the exact verified artifact below — against product intent (ph-ed41 ACs), guide v2 and the phase-1 tasks. Decide approved / changes-requested / blocked.

## Bindings

| Binding | Value |
|---|---|
| Scope | composition |
| Exact subject SHA | verified artifact `d80094913d16f68cefbca128015531c5aa3fc197` (recorded in `assets/team/composition.dd.json` `artifact_sha` at the evidence commit) |
| Product plan | `docs/plans/028-prep-canonical-tables/plan.dd.json` sha256 `e1872f2cc578e37b275fad3b3057c05b38ed9f7e719de360696c32bbcf5d6f86` |
| Implementation guide | `docs/plans/028-prep-canonical-tables/assets/impl-guide.dd.json` sha256 `34d67d70b4f3a8fcbc4a59bd93696c62ca7653533da0d1a85a951fa0d59d8dba` |
| Requested role | `{"role":"reviewer","harness":"omp","model":"github-copilot/claude-sonnet-5.5","source":{"harness":"guide","model":"guide"}}` |
| Composition | `harness builder compose --import` (integration 67bd121, units tk-0003, tk-0004, tk-0002, tk-0005) then PM composition 99ac093 + follow-up 1fb05c2; `compose --verify` ran vd-0008 (prep proof) and vd-0015 (full boot incl. harness checks) |
| Evidence | `assets/evidence/` — parity (vd-000a), live (vd-0016), measurement (vd-000b) JSON, numbers only |

## What to assess

1. **Wiring and ownership:** `crates/app/src/prep.rs` is the only composition root; core/sdk carry no Parquet/SQLite/engine dependency; CLI holds no prep semantics; read-only `prep record`; catalogue `cli_persisted_resume` true for claude-code only.
2. **Per-AC evidence (phase 1):** ac-0001 (idempotent/incremental/atomic/crash), ac-0002 (live), ac-0003 (CLI/SDK parity, external consumer with its own store), ac-0004 (cost correctness and real-corpus parity), ac-0005 (typed facts), ac-0007 (coverage), ac-0008 (contract, views, compaction), ac-000a (native address + record fetch), ac-000b (privacy/safety), ac-000c (Claude-scale measurement; full corpus is phase 2), ac-000d (prep docs topic). Say which are proven by this subject and which remain partial.
3. **AC-0004 divergences — you accept or reject each explicitly:**
   - (a) 10 call rows where the reference writes gap `-1` because it resets prior-call state at its extract-window start; prep reports the real gap across the boundary.
   - (b) Gap display: prep stores integer milliseconds; the reference rounds a float to 0.1 s. Every compared gap is within 50 ms (522 rows sit exactly at a 50 ms rounding tie); none differs beyond rounding.
   - (c) 2 reference-only rows (1 trigger, 1 synthetic/limit notice) come from a symlinked source: prep never follows symlinks by design (AC-0007) and reports `skipped.symlinks: 3`; the source is reachable via an explicit `--root`.
   - (d) Unrecorded counters are null in prep where the reference writes 0; sums unchanged (compare with coalesce).
4. **Store design choices** (tk-0004): views.sql lists committed parts explicitly and is paths-relative (`cd TARGET && duckdb -init views.sql`); snapshots before state (crash window between snapshot write and state rename, repaired on next load); id-less calls are separate rows.
5. **Tests are behavioural, not wiring echoes**; fixtures synthetic and content-free.

## Return

Report + `builder/team` DD receipt proposal with `id:"rv-028-composition-p1-r1"`, `scope:"composition"`, `subject_sha` = the artifact SHA above, `plan`/`guide` path+sha256 bindings above, your observed runtime, verdict and findings (`severity` high|medium|low; `disposition` open unless verified fixed). An approved receipt may leave only low findings open. Validate with ddocs, then message the PM with verdict, counts, both paths and SHA-256. Record your AC-0004 divergence decisions (a)–(d) explicitly in the report.

## Boundaries

Read-only; write only your two files; no commits, pushes or merges; no real transcript content in outputs; stop and say so if not running as the requested model.
