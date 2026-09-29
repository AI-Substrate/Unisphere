# PM handoff — Plan 028 `prep-canonical-tables`

**From** o-prime `pij-suspicious-meadowlark` · **Date** 2026-09-29 · **Operator** Jordan

You are the PM for this plan. You own the implementation guide, decomposition, coder dispatch, composition, quality and proof. The prime owns product intent (this plan's ACs) and government; the operator owns scope, landing and publication.

## Where

- Workspace: `/Users/jordanknight/substrate/unisphere/unisphere-prep`, branch `builder/028-prep-canonical-tables`, allocation `al-028-05480739-c322-4d8f-9827-66481950518d`.
- Base: `poc/prep-duckdb` @ `0c4dbdf` (POC code + the 3 MiB per-record default fix `728dc13`). The POC is a starting point to review and harden, not accepted product code.
- Product plan (ready, 13 ACs, 2 outcome phases): `docs/plans/028-prep-canonical-tables/plan.dd.json`. Read `original-ask.md` beside it first; it points to every input (POC findings and runbook, the consumer oracle, engine research, founding brief, Flowspace3's seam brief).
- Repo rules: `AGENTS.md`, `.harness/engineering-harness.md`. Repo-local `ddocs` is not installed in this worktree; use `/Users/jordanknight/substrate/unisphere/unishpere-main/node_modules/.bin/ddocs` or run `npm ci` here if the guide flow needs it locally.

## Seats (Jordan's ruling, 2026-09-29)

- Coders: OMP, `github-copilot/claude-opus-5.5`. Choose the count the guide's independent lanes justify; isolate each in its own clone/worktree.
- Independent reviewer: Claude Sonnet 5.5. It is **not yet** in the GitHub Copilot catalog (only `claude-sonnet-5` on 2026-09-29); Jordan is adding it. Do everything else; hold only reviewer dispatch until it appears. Do not substitute a different reviewer model without Jordan's say.

## Lifecycle

Use installed `harness builder` (read live `--help`; current capabilities outrank stale skills): implementation guide → independent guide review → tasks → contract seal → dispatch → composition/verify → independent code review → real-workload proof. Guide approval, green tests or a compile are not acceptance. Record honest tooling friction with `harness observe`; never fabricate a receipt or weaken a gate.

## Non-negotiables

- SDK-first, hexagonal: loaders do I/O, pure folds interpret supplied records, SDK owns semantics, CLI parses/composes/renders. Core/SDK ports carry no Parquet/SQLite/engine dependency (Flowspace3 must be able to reuse the fold).
- No analytical engine linked into the build in this plan (see non-goals).
- Native stores read-only. Real-corpus runs write only to gitignored scratch; committed evidence is numbers only — no prompts, bodies or private identifiers.
- `extract.py` writes content-bearing CSVs: run copies outside Git only.
- No push, PR, merge or main change. Commits via `harness commit` with `pij commit-trailers` appended.
- AC-0004 parity against the consumer's reference parser is exact or explained and accepted by the reviewer — never rounded away.

## Reporting

- `pij report now "<did>" "<next>"` at the start and end of each unit (you owe a card).
- Report to the prime by `pij_send` pointer at: guide ready for review, dispatch, each delivery accepted/rejected, and phase end. Line 1 = the action or `NO ACTION`.
- Material scope questions go to Jordan directly (one context sentence, one question), with a pointer to the prime. Do not wait on the prime for work-local decisions.
