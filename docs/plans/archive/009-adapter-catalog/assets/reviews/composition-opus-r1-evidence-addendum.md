# Composition r1 — evidence addendum

Recorded after the r1 report was committed. It records what changed in the record, not in the product.

- R1 report `assets/reviews/composition-opus-r1.md` sha256 `03c5de42f625d175dea03209532028fefc86c8e3b15d14aa4491b77cda95a18f` and receipt `.harness/temp/plan009-composition-review-r1.dd.json` sha256 `191391f01a9d39a5850d1b1b92ba591528fcde9f908e8904c3157cb581ff10db` stand byte-identical. The r1 report was committed unchanged at `1f15eea4` and still hashes to the value its receipt cites.
- Reviewed product subject remains `17829db8a427d1a6e69681044d58d7cb7c837263`.
- Verdict remains **approved**. No finding changes severity or disposition.

## What the evidence-only commit closed

`1f15eea471b464ab2735caba9429c63d9a2fe268`, "docs(plan): retain exact catalog composition and consumer proof context", touches eight files, all under `docs/plans/009-adapter-catalog/assets/`. `git diff --name-only 17829db8..1f15eea4 -- crates '*.toml' Cargo.lock docs/cli.md docs/adapters.md docs/fidelity.md` is empty, so no product source, manifest, lockfile or public doc moved. The plan and guide still hash to `b5cfc5fb…68d8` and `a77126067…a790`, and the working tree is clean at this commit. Nothing I reviewed was invalidated.

Two provenance gaps named in r1 are now closed:

**The composition receipt is committed.** `assets/team/composition.dd.{json,md}` were untracked when I reviewed them. They are committed at `1f15eea4`, and `composition.dd.json` still hashes to `f2b7f95f7c3acdbd02e89363c6a2e4ccc9488fb78b040774065aaa42b842245f` — the same bytes I verified by hand, now durable rather than working-tree-only. `assets/composition-verification.json` additionally records `harness builder compose --verify 17829db8` at exit 0 returning that identical digest, so the tool's verification and my independent recomputation agree on the same artifact. `assets/composition-verify-before-init.json` retains the prior `E470` missing-receipt error, which makes the gap and its closure both inspectable rather than only the closure.

**The io-smoke provenance delta is committed and confirms my reading.** The committed `catalog-io-smoke.json` at `1f15eea4` now carries `recorded_at`, `argv`, `cwd` and `temporary_consumer_removed: true` alongside the unchanged `manifest`, `consumer_source` and `execution`. The result is byte-identical to what I assessed: exit 0, `"Actual embedded catalog: write and flush failures return exit 1; private I/O detail is not emitted."`, empty stderr. The delta is provenance only, exactly as r1 stated, and the external consumer proof is unchanged. The added `argv` also shows the consumer was built out-of-workspace via `--manifest-path .harness/temp/catalog-io-smoke/Cargo.toml`, which supports rather than weakens the claim that it exercised `unisphere-cli` as an external dependent.

`assets/candidate-commit.json` additionally records the candidate's own `harness commit` as `mode: direct-verified`, `probe: connected`, sha `17829db8` — so the reviewed commit's attribution was confirmed at creation rather than buffered.

## What is unchanged

**C2 still stands as accepted.** `composition.dd.json` hashes to the same `f2b7f95f` at `1f15eea4`, so its checks array is still exactly `vd-0002`, `vd-0003`, `vd-0004`; `vd-0001` still has no row. Committing the receipt made it durable, not more complete. The r1 grading is unaffected: coverage is established by superset, since `.harness/extensions/checks/checks.mjs` runs `cargo test --workspace --all-targets --locked` and `vd-0003` is recorded green. Recording the `vd-0001` row, or annotating that `vd-0003` subsumes it, remains the close.

**C1 still stands as accepted.** No test was added, so the human surface remains unprotected by any declared check. Unchanged by an evidence-only commit, and still the item I would encode before the next adapter lands.

No new code review, proof run, build or test was performed for this addendum. Every statement above is a read of committed bytes at `1f15eea4` and a recomputation of the digests named in it.
