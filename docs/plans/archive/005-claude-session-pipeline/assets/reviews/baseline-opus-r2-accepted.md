# Plan 005 — baseline r2, findings disposition addendum

Bookkeeping only. **No new review cycle, no re-audit, no code checks.** This addendum records the
owner's disposition of the three non-blocking findings from
[`baseline-opus-r2.md`](baseline-opus-r2.md) so the seal contract sees resolved findings rather than
open ones. The subject, the digests, the evidence and the verdict are unchanged.

| Field | Value |
| --- | --- |
| subject_sha | `ac4d28f682739c36f08bb5b8f3dd1c605a7e1a86` — unchanged |
| verdict | approved — unchanged |
| prior report | `baseline-opus-r2.md`, sha256 `cca83a8eb7825091c6eab8220bc22c9397e686eaeac063fa7b0cd176ad678a77` — preserved byte-identical |
| prior receipt | `.harness/temp/plan005-baseline-review-r2.dd.json`, sha256 `d0b89c9c056c29c73203d905972a4723da57441a1fac0b44e78c71fabe683b32` — preserved byte-identical |
| basis | `assets/baseline-seal-refusal.json` — `E475`, "Independent decomposition review is missing, red, unbound or stale", next action "resolved findings" |

`open` is not a resolved disposition, so `E475` fires on C1–C3 even though all three were reported as
non-blocking and the verdict was already approved. `accepted` is the correct resolved state for a
finding the owner has knowingly absorbed. This is a disposition change by the owner, not a re-grading
of severity or a re-description of the finding: **each finding's text, severity and evidence are
carried across verbatim**, and no material issue is relabelled. Nothing that was blocking is being
made non-blocking here, because nothing here was ever blocking.

I re-confirmed before restating the digests, since I am asserting them a second time: `HEAD` is still
`ac4d28f6`, all 8 frozen files still recompute to their manifest digests, and all 8 still report SAME
against `HEAD`. The working tree now carries further PM changes to `crates/cli/src/args.rs`,
`crates/cli/src/sessions.rs` and `crates/cli/tests/sessions.rs`; those are outside the frozen fence,
outside review scope, and unread by me. They do not touch the reviewed bytes.

## Dispositions

**C1 — accepted.** Owner's rationale: the numeric fixture edge count is incidental; the negative-edge
rejection tests and the real-graph gate are retained. I concur that this is a defensible trade — the
detection I described as lost is the vacuous-fixture case only, and rejection coverage
(`forbidden_graph_fixtures_are_rejected_with_the_edge_kind`, `malformed_or_incomplete_graphs_never_pass`)
plus the live 16-edge gate remain intact. The residual exposure is unchanged from what r2 recorded and
is now knowingly held.

**C2 — accepted.** Owner's rationale: the zero-core guard is PM-owned and will land as an integration
fix, not a baseline blocker. I concur; r2 already recorded the exposure as theoretical because
`checks.mjs` runs from the repository root, which is the only invocation path in the gate today.

**C3 — accepted.** Owner's rationale: the shared dev-only arm (`unisphere-testkit`, `tempfile`,
`serde_json`) is a deliberate allowance for controlled tests, and production edges remain strict. I
concur on the substance — the arm is dev-only and production edges were verified strict and exactly
C12 — and note the boundary observation from r2 stands unamended as recorded reasoning, not as an
open ask.

No finding was withdrawn, no severity altered, no evidence removed. r2 remains the substantive record;
this addendum only moves three dispositions from `open` to `accepted` on the owner's decision.
