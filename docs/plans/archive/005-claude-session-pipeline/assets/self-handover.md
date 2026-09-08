# Self-handover — Plan005

Memory-only, operator-requested seam, 2026-09-08.

## Authority

Finish shared fakeable loader → pure Claude adapter → OTLP writer, SDK/CLI, proof, independent review and post-flight/archive.

User: “they shoudl just be provided the data and return a contracted result”.
User: “we should report on what full fidelity means and what the gaps are once the work is done.”
Latest: “i will organise the compaction” (Main relay).

**Stop after handover; do not self-compact/reload/restart.** Jordan controls timing. Plan005 implementation/closeout authorized; main landing needs approval. No remote publication, globals, private-content export or retirement. Additional client implementations are researched only, not authorized.

## Location and exact state

PM `pij-right-kotallo`; native file tools remain rooted in main clone: use absolute paths and explicit cwd.

ROOT: `/Users/jordanknight/substrate/unisphere/unisphere-claude-session-pipeline`
PLAN: `docs/plans/005-claude-session-pipeline` under ROOT.
Branch: `builder/005-claude-session-pipeline`
Candidate: `093a89811296efb681386047b1da912348c63f10`
Baseline: `ac4d28f682739c36f08bb5b8f3dd1c605a7e1a86`; eight frozen files unchanged.

**Actual first-class compose verification succeeded** on candidate: all eight checks `vd-0005` through `vd-000c` exited 0.

Receipt: PLAN `/assets/team/composition.dd.json`
SHA256: `fef0a9a5d8be8a2e66f8e749279156dc5a7726ba3dc319bb13d181da0553b6ed`
Summary: PLAN `/assets/composition-verification-summary.json`

## Review just arrived — next action

Reviewer `pij-huge-nigel` reported **APPROVED, zero open findings**. PM subsequently read the handover, report, receipt and fidelity document at Jordan's request. Independent `shasum -a 256` checks matched all three report/review/compose digests below. **Formal receipt intake remains pending**; skip the now-completed hash check in Resume step 1. No new review needed.

Report: PLAN `/assets/reviews/composition-opus-r1.md`
SHA256: `836ad3930008adea0b81bcbe2b96c15617ce7597e06ff87f1caf89360d93f5b3`
Receipt: ROOT `/.harness/temp/plan005-composition-review-r1.dd.json`
SHA256: `8f4bc570f5909c11ef621003d23fcd21cc591a45ea334d0216afce3af12fff99`
ID `rv-plan005-composition-opus-r1`; scope composition; subject candidate.

Reviewer: OMP `github-copilot/claude-opus-5`, high; session `01a07e75-cb05-70b6-a10d-db61a6ed87d8`, PID 71450. Compact sent immediately after verdict; **PM not compacted**.

Accepted advisories: duplicate selector parsing drift, dropped native top-level fields, APFS nonUTF8 branch unexercised, writer recursion bounded on shipped parsing path. Reviewer mentions untracked work-packet schema; PM previously committed it before import—check status once, do not blindly delete/recommit. Ignore generic “seal” wording: no new baseline seal needed.

## Implemented

- `crates/core/src/collection.rs`: frozen ports/types/errors/limits.
- `crates/testkit/src/collection.rs`: fakes, functional `TextFixtureAdapter`, conformance.
- `crates/loader-jsonl`, `crates/adapter-claude`, `crates/output-otlp`: real independent implementations.
- SDK `Collector`/`collect_batch`; core `CollectionApi` injected into CLI.
- `crates/cli/src/sessions.rs`: list/export, explicit file output, limits/content policy.
- `crates/app/src/adapters.rs`: static registration; second adapter tested through one entry.
- `crates/testkit/src/bin/proof/collection.rs`: real external SDK/installed CLI proof.

Content defaults metadata-only, **not anonymity**. Pure mapper has no FS/env/clock/network. Loader Unix-only, nonrecursive, bounded LF reads. CLI restarts at zero each invocation; no persisted resume, `--harness`/session-ID lookup or record-ordinal from/to. EOF is observed boundary, not finality.

`docs/fidelity.md`: requested six-axis matrix plus CLI UX gaps/follow-ons. No lossless claim; no raw archive/revision implementation added; current explicit-content policy unchanged.

## Actual proof

123 workspace tests; six quality gates; real external SDK/installed CLI parity, content policy, hostile environment, partial tail, errors and non-overwrite. Operator binary built; synthetic export: three records/three batches/byte897.

Loader runner: 18 pass, but nonUTF8 **directory candidate** branch explicitly NOT EXERCISED (filesystem EILSEQ); direct invalid-path rejection exercised. Adapter 15; writer nine plus actual encode example. Full nested proof is in composition receipt—do not rerun to recover context.

## Peers and ownership

All fresh OMP Astra/high coders delivered/compacted; clone paths below have prefix `/Users/jordanknight/substrate/unisphere/`:

- `tk-0002` / `pij-young-tran` / `unisphere-plan005-loader`: `7b6b2417b2714c9365441de7302d4f2e24691fb4`
- `tk-0003` / `pij-living-anteater` / `unisphere-plan005-claude`: `58cb09add80c91d077e901c55391be3691aa9c23`
- `tk-0004` / `pij-bad-butterfly` / `unisphere-plan005-output`: `323877bcc240d7515a9a1866f4a54a97e2c9bed8`

Current Builder packets grant work directly: **no legacy ack/release dance**. Import/verify succeeded; ownership deviations recorded as warnings. Main oversight `pij-female-varl`; harness owner `pij-varied-alpaca`.

## Newly requested adapter catalog

Implement at the next appropriate boundary, without silently changing the reviewed candidate. Proposed command: `unisphere adapters list --json`.

- Versioned machine-readable envelope; registered production adapters only.
- Stable ID, application/display name, concise description.
- Structured usual-location hints: platform, base/home, relative root, session glob, storage format.
- Truthful capabilities/limitations: export, SDK cursor, no persisted CLI resume, delayed-update gaps.
- Listing performs no private-store scan, hint expansion or arbitrary execution. Hints do not assert local installation/store existence; actual discovery remains explicit and caller-overridable.
- Extend the existing `crates/app/src/adapters.rs` registration; no parallel catalog or outward core dependency.
- This does **not** authorize the six researched adapters; that standing implementation question remains unanswered.

## Resume

1. Verify incoming report/receipt hashes; run `harness builder review <plan> --receipt <receipt>` and retain genuine approval.
2. Finish factual AC/task evidence. Tasks: PLAN `/assets/tasks/phase-1/tasks.dd.json`; log: PLAN `/assets/execution-log.dd.json`, latest verified entry `lg-0005`. Product ACs still open; executed task assertions mostly checked, final PM assertion includes review/closeout.
3. Preserve observations; post-flight/harvest; archive complete plan; no retirement. Then prepare main landing for approval.
4. Preserve Main research commit `452e0f3cbf4c045a9d5bb10f16f5dc96134a8e98` at convergence; no clone rebase needed for it.

Uncommitted plan evidence: composition receipt/view, verification summary, observation custody, optional Main rerun observation, task/log updates and this handover. No known product changes after candidate.

Commands: cwd ROOT; `PIJ_SESSION_ID=pij-right-kotallo`; local `node_modules/.bin/ddocs`; `harness commit` plus fresh `pij-rs commit-trailers`. Rust binaries `/Users/jordanknight/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin`; command-local toolchain only. Kernel variables may not survive; don't assume them.

## Additional context

Observations: PLAN `/assets/observation-custody.json`; peer buckets untouched pending durable retention. Existing experiments remain `/Users/jordanknight/substrate/unisphere/unisphere-sdk-cli-foundation/scratch/native-readers`.

Optional multi-client research:
`/Users/jordanknight/.omp/agent/sessions/-substrate-unisphere-unishpere-main/2026-09-06T23-30-10-147Z_01a0790e-d463-7000-b541-3f4d2cc9e98b/local/multi-harness-adapter-research.json`

Harness code is inspiration, not authority; SQLite/snapshots/patch journals need native revision cursors, not fake LF offsets. No additional adapter release authorized.
