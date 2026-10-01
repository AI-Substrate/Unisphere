# Reviewer packet — Plan 029 composition review, round 1 (phases 1 + 2)

**From** PM `pij-specific-kiwi` · **Reviewer** `pij-easy-seahorse` (Sonnet 5.5, same independent seat as decomposition) · C10.

**You own (write only):** `docs/plans/029-session-status/assets/reviews/composition-r1.md` and `docs/plans/029-session-status/assets/reviews/composition-r1.receipt.dd.json` (id `rv-029-composition-r1`, scope `composition`, same shape as decomposition-r2; findings disposition ∈ open|fixed|accepted; every non-low finding needs a non-empty `evidence` string).

**Subject:** the commit adding this file (`git log -1 --format=%H -- docs/plans/029-session-status/assets/team/reviewer-composition-r1.md`); its parent `ef47ccce189b2542f56bc92ba15e11ef99e5f98f` is the verified code.

**Your job:** decide whether the composed session-status feature is fit to land (PR to main is a later, separate operator decision). Read-only; do not fix.

## What changed since your decomposition approval (read `plan.dd.md` § Implementation Summary + `assets/phase-2-plan.md`)
1. Phase 1 Claude: tk-0002 SDK StatusService (+ Pij dogfood fix: warm path O(appended), single-use cursor 'spent' reset), tk-0003 CLI + pij/tmux resolver, PM composition (crates/app/src/status.rs), proof mode `unisphere-proof status`, real-machine script.
2. Rulings: pane budget "about 50 ms bounded by one ps call"; Windows requested then dropped; macOS + Linux only (Linux ETXTBSY test fix + ubuntu container proof in assets/proof/linux).
3. Plan 028 final merged (phase-2 folds for OMP/Pi/Codex/Copilot CLI/VS Code/Cursor) — 028 closed, 029 now owns that code until landing.
4. Phase 2 gap-fixes: OMP fold v2, Codex native context_window + post-compaction tokens, Copilot CLI tokenLimit window + native cache TTL; SDK: SessionFacts.context_window / cache_ttl_seconds / cache_expires_ms, provider/dot-normalised window table, TTL only from a native split, post-compaction context, direct <id>/events.jsonl probe, 64 MiB status record limit.
5. Checks envelope bounded (CI-pinned harness 0.13.0 64 KiB parse bug).

## Evidence at `ef47ccce189b2542f56bc92ba15e11ef99e5f98f`
- CI green ubuntu-latest + macos-latest (run for ef47ccc); local `harness checks` ok; `harness boot` ready, all 8 proofs incl. status.
- Real machine: `docs/plans/029-session-status/assets/proof/status-real-evidence.json` (failures []).
- Linux: `docs/plans/029-session-status/assets/proof/linux/linux-evidence.json`.
- Builder `compose --verify` refuses E475 (plan gained phase 2 after seal) — guide checks were run directly instead; judge whether that is acceptable.

## Assess (keep it short)
1. Correctness of fact definitions per harness (unknown never 0; basis honest) — spot-check against real sessions if useful.
2. SDK cursor/incremental semantics and the harness-neutral contract (external embedders: pij Plan 157).
3. Resolver safety (pij/tmux/ps, liveness, conflicts) and the macOS/Linux split.
4. Anything that must be fixed before landing vs acceptable follow-ups (e.g. Copilot context used unknown; GPT windows unknown).

Return: `pij send pij-specific-kiwi "<receipt path>"`. No edits outside your two paths; no push.
