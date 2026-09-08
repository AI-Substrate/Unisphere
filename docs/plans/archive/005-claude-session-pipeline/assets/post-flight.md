# Plan005 closeout

## Delivered and independently reviewed

- Tested/reviewed product artifact: `093a89811296efb681386047b1da912348c63f10`.
- Shared injectable loader, pure Claude mapping, bounded OTLP writer, SDK collector, explicit CLI list/export and reusable adapter registration are implemented.
- Actual composition receipt: `team/composition.dd.json`, SHA256 `fef0a9a5d8be8a2e66f8e749279156dc5a7726ba3dc319bb13d181da0553b6ed`; all eight selected checks exited 0, including actual external SDK/installed CLI and boot.
- Independent Opus review: `reviews/composition-opus-r1.md`, approved with four accepted advisories and zero open findings. R2 is a schema-only receipt successor, not a repeated review.
- `docs/fidelity.md` is the accepted delivery report. Current CLI starts at byte zero; no persisted CLI resume, revision reconciliation, raw archive or session-finality claim. The non-UTF8 directory-entry branch is explicitly NOT EXERCISED on this host.

## Preservation and resolved blocker

- Original review R1 retained as `reviews/composition-opus-r1-rejected.json`; actual intake failures and R2 are retained rather than replaced by invented success.
- PM observations were retained in `.harness/records/retro/2026-09-08/001-plan005-delivery.md` and `002-plan005-review-intake.md`, initially committed at `98bb1cdd34e7b65233c7db5cf755f7ec420c832d`. Formatting successors end at `fe6838fd78dbb38c577fe34e784df476b78ec4f5`; the actual harvest now reads two records/two entries with zero malformed skips. Attribution landed for each commit. Only PM's two captured buckets were cleared; worker buckets remain untouched and retained in `observation-custody.json`.
- The confirmed Builder classification defect was fixed upstream in PR204. Original R2 intake succeeded after activation at 03:29:52Z for the unchanged product artifact; canonical receipt `team/review-composition-rv-plan005-composition-opus-r2.dd.json` has SHA256 `d3db8ed01868e6c514b114e607d29bfbd34db756931c52db2ec77d8dc2a0f31c`. No product test or review was repeated.
- Local telemetry lookups from both plan and native main worktrees returned E100/ref_unavailable. `closeout-telemetry.json` and `closeout-telemetry-main.json` retain the exact observations. This is not proof of global absence or successful flushing; no remote fetch/push was performed, and autosync remains disabled for these commands.
- Original experiments remain outside this plan at `/Users/jordanknight/substrate/unisphere/unisphere-sdk-cli-foundation/scratch/native-readers`; no workspace retirement is authorized.

## Post-flight exit obligations

The archive conjunct formerly attached to phase assertion `dw-000d` belongs here, after review. It was rescheduled, not removed.

- [x] Formal composition receipt intake succeeds without reclassifying unchanged source.
- [x] Final factual task/AC proof links and harness seam receipts are recorded.
- [x] `harness builder close` returned this archive and external preservation receipt for all four allocations; no workspace was retired.
- [x] Archived `harness plan validate --complete` returned zero errors, warnings, open items, contradictions and orphans.

Initial preservation receipt: `/Users/jordanknight/substrate/unisphere/unisphere-plan005-preserved/fa75faee-d644-4835-bd40-3fbf963dd1c9/receipt/preservation.dd.json`, SHA256 `d93ed4f94bc05a9074569704bffcd7d2ead6a2474b2cc19df21dbe7ef6609a8f`. The canonical post-flight flow comments carry any subsequent refresh locator. Archive commit: `5c64d21b33cd6516f639346ceed6a5e1de6b10f0`; strict completion evidence is retained in `archive-completion.json`.

The generated 16.4 MB preservation receipt initially exceeded the harness reader's 4 MiB limit. Upstream PR205 raised the preservation-reader bound to 64 MiB without changing the format or freshness/confinement rules. Actual archive departure then succeeded using the flow's recorded latest locator, `/Users/jordanknight/substrate/unisphere/unisphere-plan005-preserved/4a7ff460-7bb1-4f8f-8c1c-f574c3292957/receipt/preservation.dd.json` (SHA256 `948f500c770809028bb06b32604b16b28ab059aabe6331096166ae55f251469d`). No preserved root was modified, inventory reduced or close replayed for the fix; see `departure-validation.json`.

## Main landing and separate follow-on

`main-landing-preview.json` records a conflict-free merge-tree calculation at current refs; no merge occurred. `main-landing-intent.json` names approval and closeout prerequisites and preservation of Main's research commit `452e0f3cbf4c045a9d5bb10f16f5dc96134a8e98`. No push, PR or retirement is authorized by this note.

The newly requested adapter catalog is isolated in Plan009, not silently added to this reviewed candidate. Six additional adapters remain research-only.

## Encodable lesson

The closeout source classifier was repaired upstream and the original review intake passed. Another concrete improvement is validating the ReviewReceipt schema at delivery before PM intake; that would have caught the missing `description` fields without another handoff. Neither improvement changes the delivered product scope.
