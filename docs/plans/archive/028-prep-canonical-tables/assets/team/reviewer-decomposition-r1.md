# Reviewer packet — Plan 028 decomposition review, round 1

**From** PM `pij-native-tick` · **Reviewer seat** you (spawned for this review; record your own pij id) · **Date** 2026-09-29

**You own (write only these):**
- Report: `/Users/jordanknight/substrate/unisphere/unisphere-prep/docs/plans/028-prep-canonical-tables/assets/reviews/decomposition-r1.md`
- Receipt proposal: `/Users/jordanknight/substrate/unisphere/unisphere-prep/docs/plans/028-prep-canonical-tables/assets/reviews/decomposition-r1.receipt.dd.json`

**You may read:** everything in your clone `/Users/jordanknight/substrate/unisphere/unisphere-028-review` (checked out at the subject SHA; `git fetch origin` there if the PM names a newer subject), the POC scratch and consumer inputs named in `docs/plans/028-prep-canonical-tables/original-ask.md`, and the Plan 029 consumer answers `/Users/jordanknight/substrate/unisphere/unishpere-main/scratch/session-status-prime-answers.md`. Do not read or write the PM workspace except your two owned paths.

**Your job:** independently review the Plan 028 **decomposition** (implementation guide v1 + backpressure survey) at the exact subject SHA below, against product intent. Receiving this packet starts the review; no separate release.

## Bindings

| Binding | Value |
|---|---|
| Scope | decomposition |
| Exact subject SHA | the commit that adds this packet (`git -C /Users/jordanknight/substrate/unisphere/unisphere-prep log -1 --format=%H -- docs/plans/028-prep-canonical-tables/assets/team/reviewer-decomposition-r1.md`); your clone must `git fetch origin && git checkout --detach <that SHA>` |
| Product plan | `docs/plans/028-prep-canonical-tables/plan.dd.json` sha256 `e1872f2cc578e37b275fad3b3057c05b38ed9f7e719de360696c32bbcf5d6f86` |
| Implementation guide | `docs/plans/028-prep-canonical-tables/assets/impl-guide.dd.json` sha256 `bafc934ed01117b6149aba0322f11c4e9fe32cc2a678284a3e7d7909caca1d82` |
| Backpressure survey | `docs/plans/028-prep-canonical-tables/assets/backpressure.dd.json` sha256 `6dd72fa0f0c4e94ccbb0a23fc106f11ddcf487f1bb964248848c390f537e8b1d` |
| Requested role (resolved by `harness builder settings`) | `{"role":"reviewer","harness":"omp","model":"github-copilot/claude-sonnet-5.5","source":{"harness":"guide","model":"guide"}}` (no effort requested) |
| Actual observed configuration | yours to observe: pij id (`pij whoami --json`), native OMP session id, PID, harness, model, argv where observable (`pij state <your id> --json`, `ps -o pid,args -p <pid>`); unobservable facts go in `observed.gaps` |
| Relevant evidence | `harness builder guide docs/plans/028-prep-canonical-tables --check` (run it in your clone: ok with 3 capability-owner warnings for phase-2 criteria), the POC code at the subject (crates/{core,sdk,loader-jsonl,adapter-claude,output-prep,cli}/src/prep*.rs, crates/app/src/main.rs) |

## What to assess

Read `plan.dd.md`, `original-ask.md`, `assets/impl-guide.dd.md`, `assets/backpressure.dd.md` at the subject. Judge architecture, not structure:

1. Are the four phase-1 coder lanes genuinely independently executable and provable against the frozen `core::prep` v2 contract plus testkit fakes? Name any hidden coupling (e.g. engine ↔ store state protocol, fold ↔ engine checkpoint semantics, CLI ↔ composition root resolution).
2. Is the v2 contract (architecture.contracts) complete and correct enough to freeze before coders start: ports, DTOs, row/table columns, SessionFacts (reused by Plan 029 and Flowspace3 without a target dir), checkpoint versioning, status vocabulary, CLI argv? Missing fields an AC needs? Engine/Parquet/SQLite leakage into core/sdk?
3. Does every phase-1 AC have a real entrypoint, one accountable owner and an executable check that proves assembled behaviour (not just unit green)? Is AC-0004 parity proof capable of exact-or-explained results?
4. Is deferring phase-2 lanes to a guide v2 (re-sealed on the phase-1 composed SHA) sound, and does the v1 contract already carry what phase 2 needs (Snapshot kind, record addressing, per-harness patterns)?
5. Privacy/safety (AC-000b), live-file safety (AC-0002), crash atomicity (AC-0001), compaction (AC-0008): are the planned proofs strong enough?
6. Backpressure: do the selected RUN/EXTEND/BUILD proofs name real instruments; is certainty `Partial` honest?

## Return

1. Write the report (Markdown): verdict, findings with stable ids (`F-01`…), severity `high|medium|low`, the evidence you inspected, what you ran, and remaining unproven items.
2. Write the receipt proposal as a DD document of schema `builder/team` with exactly one section named `review` whose value is a raw `ReviewReceipt`:
   `record_type:"review"`, `id:"rv-028-decomposition-r1"`, `recorded_at` (your actual ISO-8601 time with offset), `scope:"decomposition"`, `subject_sha` (full 40-hex subject), `plan` and `guide` as `{path, sha256}` using the repo-relative paths above, `reviewer_id` (your pij id), `requested` (exactly the requested role object above), `observed` `{peer_id, root, ready:true, harness, model, native_session, pid, argv?, evidence:[...], gaps:[...]}`, `verdict` `approved|changes-requested|blocked`, `report` `{path:"docs/plans/028-prep-canonical-tables/assets/reviews/decomposition-r1.md", sha256}`, `findings:[{id, severity, description, disposition:"open", evidence?}]`.
   Wrap: `{"dd":{"schema":"builder/team"},"sections":[{"name":"review","value":{…}}],"references":[]}`. Validate with `/Users/jordanknight/substrate/unisphere/unishpere-main/node_modules/.bin/ddocs validate <path>` from inside `/Users/jordanknight/substrate/unisphere/unisphere-prep`.
3. Send the PM (`pij-native-tick`) one pij message: verdict, finding count by severity, both paths and their SHA-256.

## Boundaries

Read-only against code, plan, guide, flow and receipts. Do not edit anything except your two owned files; do not fix findings yourself; no commits, pushes or merges. Do not copy real prompts, message bodies or private identifiers from any real transcript into your outputs. Requested model settings are not provider attestation; report what you observed. If you cannot run as the requested model, say so and stop — never substitute.
