# Baseline review r5 — independent, source-bound

- Reviewer: `pij-xenacious-yarpen` (omp, github-copilot/claude-opus-5, effort high), native root `/Users/jordanknight/substrate/unisphere/unishpere-main`
- Subject: `412842e36204b42d579553429a2e6a80f5feaecc` on `builder/001-sdk-cli-foundation`, clean tree at open and close
- Plan `bc208a54522fe9d5d26c87d25270806ad72b6b0a53d9155ae4cff1b2871fe3cc` · Guide v6 `096ef3c06daff0c337a80768e2aa83c3bef2ca45803a2c0e152b91ff662b4191`
- Recorded: 2026-09-07T05:44:12Z
- Verdict: **approved** — zero open findings; four accepted guardrails carried forward undischarged

## Scope of this round

Round 4 approved the 18-file freeze set at `8b43aa9`. The reseal against that approval was
refused **E472 — "Existing baseline receipt is immutable and cannot be reused for this attempt"**,
details "The requested review does not match the existing seal", with next_action:
*"Preserve any existing seal. Select a fresh `guide.baseline.receipt` identity before committing
revised guide/inputs, obtaining their review and running `contracts --seal`."*

The r5 subject is that next_action executed literally. The only new decision is whether the
receipt-identity change is genuinely identity-only, and whether the old seal survived intact.

## The change is three leaves

Structural leaf comparison of guide v5 against v6 — not a diffstat reading — yields **exactly three
changed leaves and no others**:

| Leaf | v5 | v6 |
|------|----|----|
| `meta.version` | 5 | 6 |
| `meta.updated` | 05:35:00 | 05:41:51 |
| `baseline.receipt` | `team/baseline.dd.json` | `team/baseline-v6.dd.json` |

Every other section compares equal object-for-object: `architecture`, `fan_out`, `capabilities`,
`units`, `isolation`, `roles`, `checks`, `composition`, `review`, `risks`. Within `baseline` the only
differing key is `receipt` — `files` is list-identical at 18 entries and `proof` still resolves to
`impl-guide.dd.json#checks/vd-0001`. Contract count holds at 15. No check argv moved.

The name-status diff `8b43aa9..412842e` touches **no path under `crates/`**, no `Cargo.toml`, no
`Cargo.lock`, no `rust-toolchain.toml`. All 18 frozen files are byte-identical across that range,
so the freeze set I approved at r4 is the same bytes here.

## The old seal was preserved, not overwritten

This is the load-bearing check, because the failure mode E472 exists to prevent is exactly the one
that would be tempting here — quietly rewriting `team/baseline.dd.json` so the requested review
matches it.

- `assets/team/baseline.dd.json` is **byte-identical** across `8b43aa9..412842e`, still
  `a665d6971e0aaad60008a7cb9ba6fcbcdaf238f511e8175afdf0f59f459cff46` — the digest `lg-0003`
  cites, so that citation still resolves.
- The new `assets/baseline-history-8eafef5.json` is a byte-identical copy of that seal; its own
  content digest **is** `a665d697…`. The 17-file seal now exists in two places, neither edited.
- `assets/team/baseline-v6.dd.json` **does not exist** at this SHA. Correct: it is a forward
  identity for Builder to write at seal time, not a hand-authored receipt. No composition evidence
  was fabricated to satisfy the refusal.

This also settles the forward recommendation I raised at r4. I flagged that a reseal might overwrite
`baseline.dd.json` and strand `lg-0003`'s digest citation. The installed runtime does not overwrite —
it refuses — and the PM followed the refusal's own next_action rather than my speculative guardrail.
The runtime's answer was better than my recommendation, and the record says so plainly.

## The refusal and the carried proof are recorded honestly

- `assets/baseline-reseal-refusal.json` preserves the attempt verbatim: full argv naming my r4
  receipt, cwd, exit code 1, and the complete E472 payload including next_action. Nothing sanitized.
- `lg-0005` states that no 18-file seal and no new baseline-test execution is claimed, names the
  refusal asset, records the preserved seal digest, and carries applicability forward by reasoning
  rather than restatement: only receipt identity, version and timestamp changed from the approved v5,
  so `lg-0004`'s source-digest applicability and `lg-0002`'s executed 3+10 tests, scoped clippy and
  fmt results carry unchanged. It links `lg-0004`, `lg-0002` and `bp-0007` instead of copying them.
- `dw-0001/2/3` repoint from `lg-0004` to `lg-0005` — **exactly three changed task leaves, none
  added or removed**. `dw-0004` and `tk-0001` remain unchecked pending fresh review and actual seal.
- Scope intact: 5 units all unchecked, 20 assertions with 3 checked and 17 unchecked, 11 plan
  acceptance criteria (plan byte-identical), 11 backpressure rows.
- Every prior evidence record — dispositions, r2 final checks, ready refusal, path diagnosis,
  proof-link repair, backpressure — is byte-identical across the range.

## Prior-round fidelity

- `assets/reviews/baseline-opus-r4.md` imported byte-identical to my emission,
  `578ff9ab615fddb648941a3fb5814c03bc84227160d0b76c1cd59ac8a67cc519`.
- The imported r4 team receipt is field-for-field identical to mine,
  `3c6e60f3879d4e20ad49243629d3ecb6965ea2b3e32f13b5f13904bc8b0d47e0`.
- `assets/team/basis-2bd056c9….json` is a byte-identical snapshot of guide v5 and its content
  digest equals its own filename — a correct harness-emitted basis for the reviewed guide.
- r1, r2 and r3 reports unchanged.

## Findings

| ID | Severity | Disposition | Note |
|----|----------|-------------|------|
| F-0001 | medium | fixed | Toolchain evidence; bytes unchanged at this SHA |
| F-0002 | medium | fixed | Folded into F-0001; unchanged |
| F-0003 | medium | accepted | Guardrail, undischarged |
| F-0004 | low | accepted | Guardrail, undischarged |
| F-0005 | medium | fixed | Negative fixtures; `fixtures.rs` `aa915f10…` unchanged |
| F-0006 | low | accepted | Guardrail, undischarged |
| F-0007 | medium | fixed | Proof-link chain lg-0002 → lg-0004 → lg-0005 intact, no rerun claimed |
| F-0008 | low | accepted | Guardrail, undischarged |

No new finding. One non-blocking observation: between this commit and the seal, `baseline.receipt`
names a file that does not yet exist, so the guide's own pointer is unresolvable in the interval.
That is inherent to the prescribed sequence, and `lg-0005` plus `baseline-history-8eafef5.json`
keep the superseded 17-file seal reachable by path and digest meanwhile. Confirm `baseline-v6.dd.json`
is committed immediately after the seal so the pointer does not stay dangling.

## What this approval does and does not permit

Permits: `harness builder contracts --seal` against guide v6 citing the r5 receipt, and readiness
re-attempt afterwards.

Does not permit and does not claim: checking `tk-0001` or `dw-0004` without Builder actually
re-executing `vd-0001` at the seal; any product acceptance criterion — all eleven remain unchecked;
`vd-0002`..`vd-000f`; an executed network-denial proof for bp-0008 beyond the committed
source-surface inspection; the runtime collector seal probe, which stays PM-owned; or the
composition review dw-0011.

## Method and limits

Read-only: `git rev-parse`, `status`, `log`, `diff --name-status`, `show`, SHA256 recomputation from
committed blobs, and in-process structural leaf comparison of the guide, tasks, plan and imported
receipts. Per the packet I executed **no** build, test, linter, formatter, `ddocs` or `harness`
verb; the E472 payload, the earlier E471 payloads, the seal stdout and the r2 check results are read
as committed records, and what I verified independently is that their bound digests equal current
committed bytes and that their conclusions reproduce from those bytes. Writes were confined to the
two r5-authorized paths, both new; all r1–r4 artifacts are untouched.
