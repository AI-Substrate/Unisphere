# Continuity checkpoint

## Authority

Jordan controls compaction; do not compact/reload/restart this PM. Main landing needs explicit approval. No remote publication, private-store export, global changes or retirement. Six additional source adapters remain research-only. Native tool root is `/Users/jordanknight/substrate/unisphere/unishpere-main`; use explicit workspace paths/cwd.

## Plan005: product delivered, archive completed

Workspace: `/Users/jordanknight/substrate/unisphere/unisphere-claude-session-pipeline`.
Branch: `builder/005-claude-session-pipeline`.
Archive: `docs/plans/archive/005-claude-session-pipeline/plan.dd.json`.
Tested/reviewed artifact: `093a89811296efb681386047b1da912348c63f10`.

Actual composition verification passed all eight checks; independent Opus approved with zero open findings. Original R2 intake succeeded after the confirmed upstream PR204 fix excluded closeout retro records from code-change detection. No settled product proof/review was repeated.

- Compose receipt SHA256: `fef0a9a5d8be8a2e66f8e749279156dc5a7726ba3dc319bb13d181da0553b6ed`.
- Accepted review: `assets/team/review-composition-rv-plan005-composition-opus-r2.dd.json`, SHA256 `d3db8ed01868e6c514b114e607d29bfbd34db756931c52db2ec77d8dc2a0f31c`.
- Archive commit: `5c64d21b33cd6516f639346ceed6a5e1de6b10f0`.
- Strict archived validation: zero errors/warnings/open items; `assets/archive-completion.json`.
- Initial external preservation: `/Users/jordanknight/substrate/unisphere/unisphere-plan005-preserved/fa75faee-d644-4835-bd40-3fbf963dd1c9/receipt/preservation.dd.json`. Canonical flow comments contain any later refresh locator.

No main merge or workspace retirement occurred. Preserve Main research commit `452e0f3cbf4c045a9d5bb10f16f5dc96134a8e98` at convergence; a read-only merge-tree preview was conflict-free before final archive changes.

`docs/fidelity.md` remains the accepted report: explicit Unix Claude JSONL projection, not lossless archival or final session history. Metadata-only is not anonymity. CLI restarts at byte zero; SDK provides caller-owned cursor; no persisted CLI resume or revision reconciliation. The nonUTF8 directory-entry branch is explicitly NOT EXERCISED on this filesystem.

PM observations were committed and then only own buckets cleared; worker custody remains in `assets/observation-custody.json`. Local telemetry lookups returned E100/ref_unavailable: no global-absence or flush claim. Original experiments remain `/Users/jordanknight/substrate/unisphere/unisphere-sdk-cli-foundation/scratch/native-readers`.

## Plan009: authorized catalog, design approved, implementation next

Workspace: `/Users/jordanknight/substrate/unisphere/unisphere-adapter-catalog`.
Branch: `builder/009-adapter-catalog`.
Plan: `docs/plans/009-adapter-catalog/plan.dd.json`.
Allocation: `al-009-1ed1bab9-c1d6-42f4-8c83-b4242c50b6d5`.
Reviewed design: `09c0605f00b21594494be1660bca74fb977e09fa`.

Opus R2 approved design; intake and implementation remain next. Receipt: `.harness/temp/plan009-design-review-r2.dd.json`; report: `assets/reviews/design-opus-r2.md` under this plan. Preserve R1/R2; no repeated source audit.

Implement `unisphere adapters list --json` from the existing registration, not a parallel catalog. Core gets pure serializable static `AdapterDescriptor`, `LocationHint`, `AdapterCapabilities`; CLI renders injected references; app registration contains descriptor plus runner and selects via `descriptor.id`.

Contract corrections are authoritative in the guide:
- Production registry export test binds each descriptor ID to emitted `unisphere.source.adapter`.
- No duplicate generic `limitations` field.
- `sdk_caller_owned_cursor=true`, `cursor_source_assumption=append_only`; CLI persistence/revision reconciliation/lossless archive remain false.
- Mandatory real-binary `adapter_catalog` integration target exercises hostile environment, inaccessible stores and output modes.
- JSON success/failure label is `adapters.list`; human output labels symbolic hints as not detected installations. Document deliberate stream difference: catalog JSON errors on stdout, session errors on stderr.

No product code has yet changed for Plan009. Design report approval is not runtime proof. Do not add the six researched adapters.

## Peers and tools

PM `pij-right-kotallo`; coordinator `pij-female-varl`; prime relay `pij-minor-unicorn`; harness owner `pij-varied-alpaca`; independent reviewer `pij-huge-nigel` (OMP Opus5/high, session `01a07e75-cb05-70b6-a10d-db61a6ed87d8`, PID71450 last observed). Original Plan005 coders: loader `pij-young-tran`, Claude `pij-living-anteater`, output `pij-bad-butterfly`.

OMP swallowed queued peer turns despite empty `pij inbox`; prime is hand-delivering while the fix is in flight. Treat those as normal peer messages; never replay transport files.

Use repo-local `node_modules/.bin/ddocs`, `harness commit`, fresh native commit trailers, command-local Rust1.95.0 PATH/toolchain, `PIJ_SESSION_ID=pij-right-kotallo`, and `HARNESS_NO_TELEMETRY_AUTOSYNC=1`. Plan009 uses a real ignored node_modules directory with a .bin symlink, not an untracked directory symlink.
