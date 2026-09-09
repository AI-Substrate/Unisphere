# Plan 010 — composition review R2: formal binding addendum

- Reviewer: `pij-huge-nigel` (omp / github-copilot/claude-opus-5, effort high)
- Scope: `composition` — **binding addendum only**, not a repeated product review
- Formal artifact (subject of this receipt): `6a38efaf4db192a6e35fafa903db7c06cdbe371e`
- Product bytes originally approved at: `c6f3636b394f0e17445eb2e143f6ac2c6d1ec998`
- Plan: `docs/plans/010-native-adapters/plan.dd.json` sha256 `2160c94a738ec3cb49c482d8099c4a86ee5dfe36b545ffdaaedd106693eab3ac` *(changed since R1 — completion bookkeeping, see below)*
- Guide: `assets/impl-guide.dd.json` sha256 `38d5a0d69c2b56da77e5f2f4648d0592586e4df4a76f0523a1c2a67c3a45c475` *(unchanged blob since R1)*
- Prior report: `assets/reviews/composition-opus-r1.md` sha256 `f72935cf…b12c213` — **immutable, re-verified**
- Prior receipt: `assets/reviews/composition-opus-r1-receipt.json` sha256 `8d07fa2c…3de6ba` — **immutable, re-verified**

**Verdict: approved.** The R1 approval of `c6f3636b` carries forward to `6a38efaf` unchanged, because the product bytes are the same bytes — proven by tree-object identity, not by assertion. The formal artifact now additionally carries a genuine composition receipt whose integration proof I independently reproduced.

## What this round is, and what it is not

This is a reviewer-owned successor that binds an already-approved product to the formal artifact that now records it. It is **not** a second review of the implementation. I ran no build, test, clippy, rustdoc, formatter or linter; I read no adapter, loader, core, SDK or CLI source this round; I re-audited nothing that R1 settled. I edited no code, plan, guide, flow, team or canonical asset — only the two deliverables this packet declares.

The R1 approval is not relabelled, not backdated and not restated as if it had been issued against `6a38efaf`. It was issued against `c6f3636b` on the evidence available then, and it stands exactly as written. What follows is the additional, newly observed fact that makes it transferable.

## The binding: product bytes are identical

I did not accept "diff empty" as a claim. Byte equality between `c6f3636b` and `6a38efaf` is established here at the Git object level, which is stronger than a textual diff because it compares content-addressed trees rather than rendered output:

| Object | `c6f3636b` | `6a38efaf` |
|---|---|---|
| `crates` tree | `98d59964ac0005166bb39596ed0761b20fd80760` | identical |
| `Cargo.lock` blob | `04ad7e25009ab9dc296f7d4524493b5bd9faffa2` | identical |
| root `Cargo.toml` blob | `0d985e25005dcb8a6bdf800c085f2ef6445f39e9` | identical |
| `docs/fidelity.md` blob | `7aaaa9671421dd0378be0416ae93f75af8e0c4c8` | identical |

The `crates` tree OID covers every crate manifest and every source and fixture file transitively, so a single matching hash forecloses any change anywhere beneath it, including one that a path-filtered diff could have missed.

The five commits between the two shas total 81 files, +3290/−138. Every changed path is under `docs/plans/010-native-adapters/` except six `.harness/records/retro/2026-09-08/*.md` worker retrospectives, which are additive records, not product. `c6f3636b` is a verified ancestor of `6a38efaf`.

**Consequence:** every behavioral statement in `composition-opus-r1.md` — the representation-aware provenance proof, the registry test that builds a real SQLite database per registration, three-way byte-identical parity, mutation-proven revisions, checkpoint-after-output with 11 partial bytes and no checkpoint, the `SQLITE_OPEN_NOFOLLOW` and `immutable=1` reasoning, mapper purity enforced by the dependency sensor — describes `6a38efaf` verbatim. Nothing needs re-deriving, because nothing moved.

## Plan and guide: intent unchanged

The guide blob is bit-identical to the one I sealed at R1, so the architecture I reviewed is the architecture bound here.

The plan digest changed and I read the whole diff rather than trusting the "factual completion additions" summary. It is bookkeeping only:

- Ten acceptance criteria `ac-0001`–`ac-000a` flip `state` from `unchecked` to `checked` and gain `proven_by: assets/execution-log.dd.json#entries/lg-0005`. **Every `claim` string is unchanged** — the diff shows them as context lines, so no criterion was weakened, rewritten or narrowed to fit what was built.
- Phase `ph-cce1` flips to `checked`.
- `implementation_summary` fills from empty.

No AC was added, removed or retitled. The bar I reviewed against is the bar that is now marked met, which is the property that matters for transferring an approval.

One staleness note, non-blocking: that new `implementation_summary` still asserts that "historical native-binding and baseline-check defects prevent formal Builder receipt/archive only." As of the receipt described below, that sentence is out of date. It was accurate when committed and it under-claims rather than over-claims, so it is safe in the wrong direction — but a reader citing the plan alone would conclude no formal receipt exists. PM-owned text, not mine to edit.

## The formal receipt, verified rather than assumed

`assets/team/composition.dd.json` is sha256 `70fee1b212963f7a26bcf0c52850637e5431dcb7fa2d447dba2b186d78a0a5ef` — matching the packet — with `artifact_sha` `6a38efaf`, `integration_sha` `114e1535`, `integration_method: already-integrated`, seven units on baseline `3a635c17`, and 473 file digests.

Its three checks are real and green: `cargo test --workspace --all-targets --locked` at exit 0, `harness boot --json` at exit 0, `harness checks --json` at exit 0. I parsed the captured stdout rather than reading the exit codes alone. The test check sums to **236 passed** across 36 binaries with `0 failed` in every result line — reconciling exactly with the retained count. The boot payload is `status: ok`, `ready: true`, scope `configuration-and-native-session-projections`, six gates and five proof modes all at 0.

**I reproduced the integration claim independently.** `integration_method: already-integrated` is the shape that most deserves scepticism, because it asserts that seven workspaces' work arrived intact without a merge to inspect. So I compared each unit's owned crate tree between its delivery commit and the integration commit:

| Unit | Peer | Owned tree | Delivery → `114e1535` |
|---|---|---|---|
| tk-0002 | pij-ideological-koala | `crates/adapter-codex` | identical (`6b678d09`) |
| tk-0003 | pij-financial-elle | `crates/adapter-omp` | identical (`e9257ed2`) |
| tk-0004 | pij-remarkable-woodpecker | `crates/adapter-pi` | identical (`d2156504`) |
| tk-0005 | pij-homeless-dudley | `crates/adapter-copilot-cli` | identical (`9f2d4858`) |
| tk-0006 | pij-adorable-bat | `crates/adapter-vscode-copilot` | identical (`a7b8552c`) |
| tk-0007 | pij-mechanical-tsarina | `crates/adapter-cursor` | identical (`01b703f9`) |
| tk-0008 | pij-kind-turkey | `crates/loader-snapshot` | identical (`171e783b`) |

Seven for seven. No worker's delivered tree was rewritten, replayed or reconstructed on the way in. The unit commits are correctly *not* ancestors of `6a38efaf` — they live in separate workspaces — which is precisely why the tree comparison, rather than an ancestry check, is the honest proof, and it holds.

`114e1535` is a verified ancestor of `c6f3636b`. Those crate trees then legitimately differ at the candidate: that difference *is* the composition commit — registration, provenance vocabulary, proof updates — which is the delta I reviewed and approved at R1.

## R1 findings: carried forward unchanged

K1 (journal hole budget counted in `Value` slots), K2 (`vscode-copilot` publishes two storage formats but registers one), K3 (nested `checks` scope string still says Claude-only) all remain exactly as recorded: low, accepted, non-blocking, with no product bytes changed to address them and none required. K3 is directly re-observable in this receipt's `vd-0004` payload, whose scope is still `configuration-and-claude-jsonl` while boot's is `configuration-and-native-session-projections`.

Zero open material issues stand against the product.

## Custody caveat, stated plainly

The canonical receipt is **not committed**. `assets/team/composition.dd.json`, its `.dd.md` sibling, `formal-verification-summary.json`, `activated-historical-import.json` and `self-handover.md` are all untracked at `6a38efaf`. Everything above is verified against the bytes on disk right now, and their digests are recorded in my receipt so any later change is detectable — but "durable in canonical" is true of the *path*, not yet of the *history*. A closeout that lands the plan without committing them would ship a plan whose own text says no formal receipt exists while the receipt proving otherwise survives only in a working tree. PM-owned and trivially closed by a commit; I flag it rather than treat it as satisfied.

By contrast, my R1 report and receipt **are** committed, at byte-identical digests to what I authored. Immutability holds.

## Basis and boundaries

What this round establishes: the approved bytes and the formally recorded bytes are the same bytes; the plan's criteria were not altered to fit the delivery; the composition receipt's integration claim is independently reproducible and reproduces; its checks are genuinely green with 236 tests and 0 failures.

What it does not: I ran nothing this round, so every runtime outcome cited is the receipt's own capture, read and reconciled, never reproduced. R1's boundaries stand undiminished — synthetic fixtures only, single-host Unix, no live vendor installation, hostile ancestor directories out of the threat model. `output_capture` in the formal summary states the raw verifier stdout was recovered after an eval reset rather than re-run; I take that at its word and did not re-run the verifier. Formal Builder machinery being satisfied is a process fact, not additional evidence about product behavior, and I have not treated it as such.
