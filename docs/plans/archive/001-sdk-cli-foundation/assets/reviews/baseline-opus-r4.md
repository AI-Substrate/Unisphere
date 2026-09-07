# Baseline review r4 — independent, source-bound

- Reviewer: `pij-xenacious-yarpen` (omp, github-copilot/claude-opus-5, effort high), native root `/Users/jordanknight/substrate/unisphere/unishpere-main`
- Subject: `8b43aa9ea5a253aaf004abe2506c7bb949a898ab` on `builder/001-sdk-cli-foundation`, clean tree at open and close
- Plan `bc208a54522fe9d5d26c87d25270806ad72b6b0a53d9155ae4cff1b2871fe3cc` · Guide v5 `2bd056c99c8d6bfb9ba8ddc4fac3daf65e679f9d4663bea20d08fdde042060fe`
- Recorded: 2026-09-07T05:37:59Z
- Verdict: **approved** — zero open findings; four accepted guardrails carried forward undischarged

## Scope of this round

Round 3 approved `8eafef5`, Builder sealed it, and then `harness builder ready` refused
E471 for tk-0002, tk-0003 and tk-0005 alike with `Dependencies lack committed proof: tk-0001`.
The r4 subject is the corrective: guide v5 adds the already-existing, unchanged `.gitignore`
to the frozen baseline set. I reviewed the current subject whole, but the only new decision
is whether that expansion is the right, minimal and honestly-recorded fix.

## The refusal predicate, reproduced independently

I did not take the diagnosis on trust. Recomputing the coverage predicate myself against the
committed bytes:

- `impl-guide.dd.json#units/tk-0001/paths` lists ten fence entries, including `.gitignore`, and
  is **byte-identical between v4 and v5** — the whole `units` section is unchanged. The fence
  always claimed `.gitignore`; only the freeze set omitted it.
- Matching that fence against the sealed `assets/team/baseline.dd.json#baseline/files` (17 files),
  the set of fence paths with no covering frozen file is exactly `['.gitignore']`.
- Matching the same fence against guide v5's 18-file set, that set is empty.

So the refusal is a genuine coverage gap in the guide, not a harness defect and not a missing
test. The E471 payload itself only says `tk-0001`; it never names a path. The attribution to
`.gitignore` therefore rests on the recorded `ddocs get` diagnosis plus the harness owner's
confirmation — and it survives independent recomputation, which is the part that matters.

## The fix is exactly minimal

- Guide v4 → v5 leaf diff is **`version` 4→5, `updated`, and the `baseline.files` array only**.
  Every other section — `architecture`, `fan_out`, `capabilities`, `units`, `isolation`, `roles`,
  `checks`, `composition`, `review`, `risks` — compares **equal object-for-object**. Contract count
  stays 15. No check argv moved, so `vd-0001` still runs the same command.
- The 18-file set equals **the sealed 17 plus `.gitignore`, exactly** — nothing else crept in.
  `Cargo.lock` remains excluded, so the O1 composition-owned exception is preserved unchanged.
- The name-only diff `8eafef5..8b43aa9` touches **no path under `crates/`**, no `Cargo.toml`,
  no `Cargo.lock`, no `rust-toolchain.toml`. All 18 now-frozen files are byte-identical across
  that range; `.gitignore` itself is unchanged since the initial commit `a7747e0`.

## The carried proof is legitimate, not recycled

`lg-0004` re-binds dw-0001/2/3 across a guide change without claiming a rerun, and its logic holds:

- It asserts the 17 prior frozen source digests still equal the approved seal. I recomputed all
  of them and they do, including fixtures `aa915f10…`.
- The added file is `.gitignore`, which compiles nothing and is consumed by no crate — `git grep`
  finds no reference to `scratch` anywhere under `crates/`. Expanding the freeze set by it cannot
  invalidate a test result over the other 17 files.
- It states plainly that no fresh test execution is claimed, and it links `lg-0002` and `lg-0003`
  rather than restating their results as new.

Independently, the sealed `baseline.dd.json` carries **Builder's own `vd-0001` stdout** at exit 0 —
3 core plus 10 testkit tests, named individually. dw-0001/2/3 are thus backed by a harness-executed
run over these exact bytes, not only by the PM's own receipt.

## Honest bookkeeping under a failed gate

This is the part I would have failed the round on had it gone the other way. Under an E471 refusal,
the cheap moves are to check `tk-0001` anyway, to hand-write a composition receipt, or to quietly
drop `.gitignore` from the fence so the predicate passes. None happened:

- `tk-0001` and `dw-0004` were checked at the successful 17-file seal (`6c3d292`) and are **reset to
  unchecked** at `8b43aa9`, because the guide they were sealed against has been superseded. Counts
  at the subject: 3 of 20 assertions checked, 5 units unchecked, 11 ACs unchecked.
- `dw-0004` still cites `lg-0003` while unchecked — correct, since `lg-0003` is real partial evidence
  of an approved review and a successful seal, just not of the 18-file one.
- The refusal is preserved verbatim, all three attempts with full stdout, alongside the PM disposition
  "Do not invent composition evidence or bypass readiness". The superseded seal and `lg-0003` are
  retained as immutable history, matching the supersede-without-erase pattern established at r2→r3.
- My r3 artifacts are imported **byte-identical**: `baseline-opus-r3.md` `3bf88ef9…` and the team
  receipt `55f2ada2…`, the latter field-for-field equal to what I emitted, with all eight dispositions
  intact. The r1 and r2 reports are likewise unchanged. `lg-0003` cites that receipt digest correctly.

## Findings

| ID | Severity | Disposition | Note |
|----|----------|-------------|------|
| F-0001 | medium | fixed | Toolchain evidence; bytes unchanged at this SHA |
| F-0002 | medium | fixed | Folded into F-0001; unchanged |
| F-0003 | medium | accepted | Guardrail, undischarged |
| F-0004 | low | accepted | Guardrail, undischarged |
| F-0005 | medium | fixed | Negative fixtures; `fixtures.rs` `aa915f10…` unchanged |
| F-0006 | low | accepted | Guardrail, undischarged |
| F-0007 | medium | fixed | Proof-link repair; carried forward correctly to `lg-0004` |
| F-0008 | low | accepted | Guardrail, undischarged |

No new finding. One forward recommendation, non-blocking: the 18-file reseal will overwrite
`assets/team/baseline.dd.json`, after which `lg-0003`'s citation of seal digest `a665d697…` describes
a file that no longer exists at that digest. `lg-0004` already frames `lg-0003` as historical, so this
is sound as written — but record the new seal as `lg-0005` superseding `lg-0003` rather than editing
`lg-0003`, keeping the same pattern that made F-0007's repair trustworthy.

## What this approval does and does not permit

Permits: reseal of the 18-file baseline against guide v5 citing the r4 receipt, and readiness
re-attempt afterwards.

Does not permit and does not claim: checking `tk-0001` or `dw-0004` without Builder actually
re-executing `vd-0001` at the reseal; any product acceptance criterion — all eleven remain
unchecked; `vd-0002`..`vd-000f`; an executed network-denial proof for bp-0008 beyond the committed
source-surface inspection; the runtime collector seal probe, which stays PM-owned; or the
composition review dw-0011.

## Method and limits

Read-only: `git rev-parse`, `status`, `log`, `diff --name-status`, `show`, `grep -l`, SHA256
recomputation from committed blobs, and in-process structural leaf comparison of the guide, tasks
and imported receipts. Per the packet I executed **no** build, test, linter, formatter, `ddocs` or
`harness` verb; the `ddocs get` outputs, the E471 payloads, the seal stdout and the r2 check results
are read as committed records, and what I verified independently is that their bound digests equal
current committed bytes and that their conclusions reproduce from those bytes. Writes were confined
to the two r4-authorized paths, both new; all r1, r2 and r3 artifacts are untouched.
