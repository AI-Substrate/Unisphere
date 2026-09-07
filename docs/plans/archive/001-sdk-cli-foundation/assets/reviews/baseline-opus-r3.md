# Plan001 baseline review r3 — focused F-0007 verification

- **Subject SHA:** `8eafef5ef7dd5e798d8c869f3fb367c884789948` (`docs(plan001): bind baseline assertions to current verified source`)
- **Prior subjects:** `633b9bf` (r2, changes-requested), `18ee5165` (r1)
- **Scope:** decomposition — focused re-review of the single open finding F-0007
- **Reviewer:** `pij-xenacious-yarpen` — OMP, `github-copilot/claude-opus-5`, effort `high`
- **Reviewer native root:** `/Users/jordanknight/substrate/unisphere/unishpere-main`
- **Plan:** sha256 `bc208a54522fe9d5d26c87d25270806ad72b6b0a53d9155ae4cff1b2871fe3cc` (unchanged since r1)
- **Guide v4:** sha256 `370eaee98d6122eb2581b158ca606236b3318db66d7b694ac16273746c4a3d9e` (unchanged since r2)
- **Backpressure:** sha256 `5e700e9aa22288422c94ea943c7407a0ddf7ba13f4dad50a5d113697718fc6fc` (unchanged)

## Verdict

**approved.** F-0007 is fixed. No finding is open. Four residual findings remain `accepted` with their guardrails intact and unchanged.

The repair is exactly the one requested and nothing more. I verified that claim structurally rather than taking it: the leaf-level diff of `tasks.dd.json` from `633b9bf` to this SHA is **three leaves, all three `proven_by` addresses**, with zero leaves added and zero removed. No assertion state flipped, no note was reworded, no unit was touched. `crates/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `plan.dd.json`, `impl-guide.dd.json` and `backpressure.dd.json` are all byte-identical to the r2-reviewed bytes — I recomputed each digest rather than trusting the diff summary. The PM's "no code or guide change" statement holds under inspection.

## F-0007 — fixed

`lg-0002` now exists and the three `tk-0001` assertions point at it:

```
dw-0001 checked → ../../execution-log.dd.json#entries/lg-0002
dw-0002 checked → ../../execution-log.dd.json#entries/lg-0002
dw-0003 checked → ../../execution-log.dd.json#entries/lg-0002
```

`lg-0002` names the current baseline source `633b9bf`, cites `assets/baseline-r2-final-checks.json`, and records the correct run: `cargo test … --lib` exit 0 with **3 core + 10 testkit**, `cargo clippy … --lib --tests -- -D warnings` exit 0, `cargo fmt --all --check` exit 0. Every element that was wrong in the r2 record is now right: the commit is the current one, the count is 13 rather than 11, and the clippy scope is the widened one that actually lints the new test code. `dw-0002`'s "**all** shared config fixtures are usable independently" is now backed by a run that executed the five constants and the generator, which was the precise defect.

Three things about the repair are better than the minimum I asked for.

**It binds by digest, not by commit.** `lg-0002` carries `fixtures SHA256 aa915f10…` and `guide SHA256 370eaee9…` in its own text. That detail is what makes the entry survive this very commit: `lg-0002` names source `633b9bf` while the subject is now `8eafef5`, and the link is nonetheless sound, because both bound digests are still the current committed bytes at `8eafef5` — I recomputed both from the committed blobs to confirm. A commit-sha-only citation would already have gone stale one commit after being written. This is the correct pattern and it should be the house rule.

**It supersedes without erasing.** `lg-0001` is retained verbatim as the second entry rather than being edited to look correct in hindsight, and `lg-0002` states plainly that it "supersedes lg-0001 for current baseline assertion links". The 11-test history stays legible.

**It refuses to overclaim.** `lg-0002` states "no fresh test execution is claimed", and `baseline-proof-link-repair.json` records `code_changed: false` and `tests_rerun: false` as explicit fields. A link repair that asserted a fresh green run would have been a worse outcome than the stale pointer it replaced; this one says exactly what it did.

The E432 warnings I reported in r2 are gone as a consequence, not as a separate patch: `baseline-proof-link-repair.json` records `ddocs validate` on `tasks.dd.json` at exit 0 with `error 0, warn 0, issues []`. I did not re-run `ddocs`, but the mechanism is verifiable from the commit without doing so — the warning class was `address-target-untracked` against an address that had no matching entry, and `lg-0002` now exists at the cited address.

## Canonical record fidelity

I checked whether my rejecting r2 review survived import intact, because a sanitized review record would be a worse defect than the one it replaced. It did, exactly:

- `assets/reviews/baseline-opus-r2.md` at this SHA is **byte-identical** to what I emitted — sha256 `7729f83b84a903de730fb2c131b03a8725b6dd0d09f99c60de19d18e1a857cb0`.
- `assets/team/review-decomposition-review-decomposition-633b9bf-pij-xenacious-yarpen-r2.dd.json` is **field-for-field identical** to my emitted receipt: I compared every top-level key and found no differences. Verdict `changes-requested` is preserved, and `F-0007` is preserved at `disposition: open`.

The plan therefore carries an honest record of having been sent back, rather than only its final approval.

## Carried forward

Every r2 judgement rests on bytes unchanged at this SHA, verified by recomputed digest rather than assumed:

- **F-0001, F-0002, F-0005 — fixed**, on `impl-guide.dd.json` and `crates/testkit/src/fixtures.rs`, both byte-identical to the r2-reviewed bytes.
- **F-0003, F-0004, F-0006, F-0008 — accepted**, guardrails preserved verbatim in `assets/baseline-review-dispositions.json` and unmodified by this commit. These remain commitments about future dispatch, import and tk-0005 implementation; approval of this baseline does not discharge them.
- **Scope intact:** 5 units, 20 assertions (3 checked, 17 unchecked), 11 acceptance criteria all `unchecked`, 11 backpressure rows. `dw-0004` — independent review and seal — is correctly still `unchecked` at this SHA, so this receipt does not pre-claim its own conclusion.

## Observation, not a finding

`lg-0002` cites source `633b9bf` rather than the current `8eafef5`. This is correct as written — the entry is about the *source* state, the doc-only commit changed no source, and the digest binding proves it. Worth stating in the closeout so nobody later mistakes the sha difference for drift.

## Boundaries

Executed, read-only: `git rev-parse`/`status`/`log`/`diff`/`ls-tree`/`show`, `shasum -a 256`, and in-process structural leaf comparison of `tasks.dd.json` and of the imported r2 receipt.

Not executed and not claimed: any build, test, linter, formatter, `ddocs` or harness verb. The 3 + 10 test result, the clippy and fmt legs, and the `ddocs validate` exit 0 rest on the PM's committed receipts; what I verified independently is that those receipts' bound digests equal the current committed bytes, that `lg-0002` exists at the cited address, and that the three assertions resolve to it. No product acceptance criterion, no `vd-0002`…`vd-000f`, no no-network execution test, no runtime collector seal probe, and no composition review (`dw-0011`) is judged here.

Working tree was clean at both the opening and closing check of this review, at `8eafef5ef7dd5e798d8c869f3fb367c884789948`.
