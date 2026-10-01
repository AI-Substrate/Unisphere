# Review — Plan 028 phase-2 decomposition (guide v3), round 2 — delta review

**Reviewer** `pij-panicky-anteater` (OMP, `github-copilot/claude-sonnet-5.5`, native session `01a0eb71-0bdc-74bd-97d4-6f85f7527e72`, pid 53353) · **Scope** decomposition · **Subject SHA** `cfd716b8dda71884f0b2838953f324f988cd0eff` · **Date** 2026-09-29

## Verdict: **approved** — guide v3 rev 2 may be re-sealed at this subject

## What changed, verified directly against the JSON (not just the packet's prose)

- All seven phase-2 lanes' `depends_on` narrowed from `[tk-0001, tk-0002/3/4/5, tk-0006]` to `[tk-0001]` only. Confirmed by reading `units[tk-0007..tk-000d].depends_on` directly: every one is now exactly `["tk-0001"]`.
- `reads` arrays are byte-identical to round 1 (still name `tk-0002` (adapter-claude), `tk-0003` (loader-jsonl), `tk-0004` (output-prep), `tk-0005` (cli) as reference-code owners) — these are now flagged as **advisory `read-owner` warnings** (9, exactly as claimed) instead of silently matching a `depends_on` entry, since the guide checker treats "read a file whose owner isn't in your `depends_on`" as worth flagging, not blocking.
- `wave` numbers unchanged: all seven lanes still `wave: 3`; `tk-000e` (PM composition) still `wave: 4` with `depends_on` unchanged at all seven lane ids — this is the one dependency edge that's real forward dispatch ordering (composition genuinely can't start before the lanes deliver), and it was correctly left alone.
- `baseline.receipt` moved from `team/baseline-p2.dd.json` to `team/baseline-p2r2.dd.json`. Confirmed on disk: `baseline-p2.dd.json` (the round-1 seal) is still present and untouched — consistent with "seal receipts are immutable, a re-seal needs a new filename," not an overwrite of prior history.
- `plan.dd.json` unchanged (hash matches round 1 exactly). `impl-guide.dd.json` hash matches the new binding. Structural check reproduced: `status: ok`, **26 write-overlap + 9 read-owner = 35 warnings**, matching the packet's claim exactly.
- `cargo check --locked --workspace` still clean — no code changed, only guide metadata.

## Is the change architecturally correct?

Yes. `depends_on` is a **dispatch-readiness graph edge** — "which other units in *this* dispatch cycle must complete before this one can be verified" — not a transitive-closure record of everything the code historically descends from. `tk-0002`…`tk-0006` are not in-flight siblings of the phase-2 lanes: they are retired, PM-owned history, already verified and composed (`rv-028-composition-p1-r1`, approved), and their code is already sitting in the sealed source tree every phase-2 clone starts from. Declaring them as `depends_on` told the dispatcher "wait for these to be verified in this run," which they never will be (they're not being re-run), producing exactly the `E471` deadlock the PM hit. `tk-0001` is the actual, correct seal boundary — the one unit phase-2 packets are sourced *from*. The `reads` field correctly remains as pure documentation ("here's where the analogous pattern lives for reference"), now surfaced as an advisory warning rather than incorrectly satisfying/conflicting with a dependency-graph entry.

This is a pure metadata fix: the underlying file/read/wave graph — which I verified line-by-line in round 1 to prove zero cross-lane coupling — is unchanged. Narrowing `depends_on` doesn't loosen any guarantee I relied on for the round-1 approval (lane independence was established from `reads`+`paths` non-overlap, not from the `depends_on` list), and it doesn't grant any lane access to another lane's in-progress work. `tk-000e`'s real dependency on all seven lanes (the one edge that matters for composition ordering) is untouched.

## Findings

None. Zero findings this round.
