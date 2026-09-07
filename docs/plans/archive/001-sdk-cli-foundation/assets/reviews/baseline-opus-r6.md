# Baseline review r6 — independent, source-bound

- Reviewer: `pij-xenacious-yarpen` (omp, github-copilot/claude-opus-5, effort high), native root `/Users/jordanknight/substrate/unisphere/unishpere-main`
- Subject: `1ec7213488044ba083da3db7f45a958024a86c86` on `builder/001-sdk-cli-foundation`, clean tree at open and close
- Plan `bc208a54522fe9d5d26c87d25270806ad72b6b0a53d9155ae4cff1b2871fe3cc` · Guide v7 `9a2977c85b3c025eafb434ef1306fc71bf3c35fcaa3bc39fd7784a97a868899d`
- Recorded: 2026-09-07T05:58:45Z
- Verdict: **approved** — zero open findings; four accepted guardrails carried forward undischarged

## The change is three leaves, again

Structural leaf comparison of guide v6 against v7 yields **exactly three changed leaves**:
`meta.version` 6→7, `meta.updated`, and `baseline.receipt` `team/baseline-v6.dd.json` →
`team/baseline-v7.dd.json`. Ten of twelve sections compare equal object-for-object — architecture,
fan_out, capabilities, units, isolation, roles, checks, composition, review, risks. Within
`baseline` the only differing key is `receipt`; `files` is list-identical at 18 entries and `proof`
still resolves to `impl-guide.dd.json#checks/vd-0001`. Contract count holds at 15; no check argv
moved. The diff `412842e..1ec7213` touches nothing under `crates/`, no `Cargo.toml`, `Cargo.lock` or
`rust-toolchain.toml`, and **all 18 frozen files are byte-identical** across the range.

## The v6 seal was real, and it is preserved

`team/baseline-v6.dd.json` (`6502b02b1060653436bed536fc229d796fa8e80ac4064a4fb464d2c015b1d242`)
records 18 file entries with **zero digest mismatches against current committed bytes**, binds
`source_sha` `412842e` and guide `096ef3c0` — the exact commit and guide I approved at r5 — and
carries `vd-0001` at exit 0 with real captured output: 3 core tests and 10 testkit tests, 13 total.
`baseline-v6-seal-command.json` shows the seal was invoked `--review` against my canonical r5
receipt and exited 0. `baseline-v6-ready.json` shows tk-0002, tk-0003 and tk-0005 all `ready` with
empty `issues`. The superseded 17-file seal `team/baseline.dd.json` remains byte-identical at
`a665d697…`, and `baseline-history-8eafef5.json` is untouched. Nothing was rewritten to fit.

`team/baseline-v7.dd.json` does not exist at this SHA. Correct — forward identity, not a
hand-authored receipt.

## The E473 defect is recorded accurately

`dispatch-native-refusal.json` preserves all three clone dispatches, each exit 1 with
`E473 "Native Git root or HEAD does not match the frozen workspace baseline."` The details payload
returns HEAD `4a6fdfe1427fa59f8bd530b037e6b636b6ae708e` — the post-seal evidence commit — against
`sealed_source` `412842e`. The same three units had returned `ready` exit 0 minutes earlier. So the
PM's characterisation is exactly what the evidence shows: **readiness tolerates a later evidence
commit, dispatch requires PM HEAD equality with the sealed source.** Committing the seal evidence is
what broke dispatch, and `lg-0007` names the defect surface, the prime who source-verified the
supported ordering, and the ordering itself without claiming a fix.

## On the ordering, and a correction to my own start message

I opened this round warning that an uncommitted review receipt would weaken what my approval binds.
The PM corrected me and the correction is right, verified against timestamps: the v6 seal ran at
05:46:39Z and my canonical r5 receipt was committed at 05:47:13Z — **34 seconds later**. Review
receipts were already ingested from disk at r3 and r5. That property is not new and my r5 approval
already lived with it. The only delta here is delaying the post-seal evidence commit until after
dispatch. I withdraw the framing.

The property that actually matters held empirically: the committed canonical r5 receipt is
byte-identical to what I wrote to `.harness/temp` (`9889c2f3…`), as is the r5 report
(`81f4d574…`). Reviewed bytes have survived the disk-to-commit transition unaltered once already,
and that is checkable by digest every time.

## Findings

| ID | Severity | Disposition | Note |
|----|----------|-------------|------|
| F-0001 | medium | fixed | Toolchain evidence; bytes unchanged at this SHA |
| F-0002 | medium | fixed | Folded into F-0001; unchanged |
| F-0003 | medium | accepted | Guardrail, undischarged |
| F-0004 | low | accepted | Guardrail, undischarged |
| F-0005 | medium | fixed | Negative fixtures; `fixtures.rs` unchanged, digest equals sealed entry |
| F-0006 | low | accepted | Guardrail, undischarged |
| F-0007 | medium | fixed | Chain lg-0002 → lg-0004 → lg-0005 → lg-0006/lg-0007 intact; no rerun claimed |
| F-0008 | low | accepted | Guardrail, undischarged |

No new finding. Two non-blocking observations:

1. Coders dispatched at HEAD `1ec7213` fork from a commit whose `baseline.receipt` names
   `team/baseline-v7.dd.json`, a file absent from that commit and, by design, still uncommitted at
   dispatch. Named closure: after the evidence commit, verify `baseline-v7.dd.json` and the r6
   receipt/report land byte-identical to the ingested on-disk digests recorded below, and ensure
   coder worktrees see that commit before any lane relies on the guide pointer.
2. `dw-0004`'s `proven_by` now names `lg-0006`, the v6 seal, while the current pointer expects a v7
   seal. `dw-0004` is unchecked so nothing false is claimed; repoint it at the v7 record once that
   seal exists.

## Scope, unchanged

5 units all unchecked; 20 assertions with 3 checked and 17 unchecked; `dw-0004` and `tk-0001`
unchecked; 11 plan acceptance criteria with the plan byte-identical to every prior round; 11
backpressure rows. Task movement is 6 leaves — three `proven_by` repoints to `lg-0007`, `dw-0004` to
`lg-0006`, plus the dependency note and receipt path — none added or removed, no state flipped.
Every prior evidence asset and the r2–r5 reports are byte-identical across the range.

## What this approval does and does not permit

Permits: `harness builder contracts --seal` against guide v7, and the three dispatches at unchanged
PM HEAD under the prime's ordering.

Does not permit or claim: checking `tk-0001` or `dw-0004` without Builder actually re-executing
`vd-0001` at the v7 seal; any product acceptance criterion — all eleven remain unchecked;
`vd-0002`..`vd-000f`; the runtime collector seal probe, which stays PM-owned; the composition review
`dw-0011`. Per the PM's standing correction, **bp-0008 is discharged by independent core/SDK
source-surface judgement plus sealed hostile-environment behaviour**; no syscall trace or executed
network-denial run is required by the approved guide and I do not treat its absence as a gap.

## Method and limits

Read-only: `git rev-parse`, `status`, `log`, `diff --name-status`, `show`, SHA256 recomputation from
committed blobs, and in-process structural leaf comparison of guide, tasks, plan, seal receipts and
imported review receipts. Per the packet I executed **no** build, test, linter, formatter, `ddocs`
or `harness` verb, and I committed nothing. The E473 payloads, the ready results and the seal stdout
are read as committed records; what I verified independently is that their bound digests equal
current committed bytes and that their conclusions reproduce from those bytes. I did not observe the
seal, the dispatches or the defect diagnosis. Writes were confined to the two r6-authorized paths,
both new; all r1–r5 artifacts are untouched.
