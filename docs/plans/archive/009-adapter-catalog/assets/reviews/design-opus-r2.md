# Plan009 adapter catalog — independent design review (r2, focused re-verdict)

- Scope: decomposition. Subject `09c0605f00b21594494be1660bca74fb977e09fa`, "docs(plan): bind catalog identity and honest cursor metadata".
- Plan `docs/plans/009-adapter-catalog/plan.dd.json` sha256 `b5cfc5fb5d3d590589607010f963b522acd34b2c77154a08aabbebc0d24d68d8`.
- Guide `docs/plans/009-adapter-catalog/assets/impl-guide.dd.json` sha256 `a77126067be182fc04fb5a4fa2a1c0f35b43b0bbf43776dd2cf27f15f64fa790`.
- Reviewer: `pij-huge-nigel`, omp, github-copilot/claude-opus-5, effort high.
- Prior round: `design-opus-r1.md` sha256 `5ce2e35522ffc280e8ea63ac3e31d34e5182d09dd8f06e015e5577d06c0feaea`, verdict changes-requested, retained byte-identical and re-verified at this subject.
- Verdict: **approved**. D1, D2, D3, D4 and D5 are all fixed in the contracts. One low, accepted documentation note remains; nothing blocks implementation.

This is a focused re-verdict. I diffed plan, guide, backpressure and tasks between the R1 basis `2e3109c1` and this subject, read `assets/design-corrections.json`, and made two bounded source checks to confirm the new contracts are constructible against shipped code. I did not repeat the R1 source audit, did not revisit Plan005, and ran no build, test, linter or formatter. No plan, guide, flow, team receipt or product source was modified.

## D1 — fixed

The guide now carries an explicit equality contract: production `descriptor.id` must equal emitted `unisphere.source.adapter`, proven by a named test `production_catalog_ids_match_exported_provenance` that "iterates the actual production registry and exports its fixture through each runner, not a parallel catalog or handwritten identity-pair assertion".

That last clause is what makes this a real fix rather than a restatement. Both degenerate forms I was worried about are explicitly foreclosed: a second hand-maintained id list, and a test that asserts `descriptor.id == adapter.name()` on a pair the author typed twice. Iterating the real registry and reading back emitted provenance means the assertion is behavioral and scales to every future registration without an edit. The equality also propagated correctly out of the guide into `ac-0005`, `bp-0005` and `dw-0005`, so it is a plan claim under backpressure rather than a guide aside.

I checked that the named test is constructible today, since a mandatory check that cannot be written is worse than none. `crates/app/Cargo.toml` already declares dev-dependencies on `unisphere-testkit`, `tempfile` and `serde_json`, and the architecture allowlist's dev arm in `crates/testkit/src/bin/unisphere-arch-check.rs` already permits exactly `unisphere-testkit | tempfile | serde_json` for that edge kind. So `crates/app/tests/adapter_catalog.rs` needs no manifest change and no allowlist change, and the app crate can compose a real export through each registered runner. The seam that was unbound in R1 is now bound by a test that can actually exist.

## D2 — fixed

`limitations` is gone from `AdapterDescriptor`, and the contract states the reason rather than leaving it implicit: "No duplicate generic limitations field; explicit capability values carry unsupported behaviors." One representation, one source of truth, nothing to drift against. The v1 envelope no longer ships a field with no type, no rule and no check.

## D3 — fixed, and better than the minimum

`sdk_cursor_resume` became `sdk_caller_owned_cursor`, and a new `cursor_source_assumption` carries `append_only`. The contract adds the sentence that matters: "A cursor is a mechanism, not safety under rewrite/regrowth/truncation-above-checkpoint."

This is the right shape. The R1 objection was that a bare positive boolean named `..._resume` told a consumer that resume is *safe*, when `docs/fidelity.md:23` is explicit that same-inode rewrite, regrowth and truncation above the checkpoint are undetected. The rename removes the guarantee from the name, and `append_only` converts the previously implicit precondition into a declared, machine-readable one — a consumer can now test whether its sources satisfy the assumption instead of inferring safety. Publishing the precondition is strictly more useful in a v1 envelope than an enumerated list of undetected failure modes would have been, and `ac-0003`, `bp-0003` and `dw-0003` all carry the append-only wording, so the claim is under backpressure rather than buried in guide prose.

## D4 — fixed

`vd-0004` is a new mandatory check, `cargo test --test adapter_catalog`, described as covering "Real binary JSON, human, help/error and hostile-environment/inaccessible-store catalog scenarios". It is attached to all six capability rows, to `tk-0001`'s proof, and to the composition/preservation proof block, and `crates/app/tests/adapter_catalog.rs` is now listed in `tk-0001`'s paths.

This closes the exact hole R1 named. Previously every capability row pointed at `vd-0002`, whose only signal is exit 0, and `vd-0003`, which runs whatever tests happen to exist — so the board could go green with the hostile-environment and inaccessible-store proofs never written. `cargo test --test adapter_catalog` fails when no such target exists, so the check cannot pass vacuously: absence of the proof is now a red check rather than a silent gap. That is the property I asked for, and it is the property that makes the purity claim in `ac-0004` gated rather than asserted, which matters most for the CLI renderer, the one surface the existing lexical purity scan cannot cover.

## D5 — fixed

The new contract line pins all four previously unspecified behaviors: JSON failures are `{ok:false,command:adapters.list,v:1,error:...}` with existing safe core failure fields; JSON goes to stdout and human failures to stderr; output-write failures exit 1 without appending a second envelope; and human success lists id, application, description, symbolic location hints and capability values while explicitly stating that hints are not detected installations. Piped output defaults to JSON, terminal to human, with `--json`/`--human` overriding after `adapters`.

I verified this against `crates/cli/src/output.rs` rather than taking it on trust, and it is a faithful description of shipped `emit` behavior rather than an invented convention: `Mode::Json` writes every response including `Response::Failure` to stdout, `Mode::Human` routes only failures to stderr, and the write-failure path already emits a bare `unisphere: could not write output.` to stderr and returns 1 with a comment forbidding a second machine envelope. So the contract pins the existing convention and simply substitutes `adapters.list` for the hardcoded `config.check` label — which was the actual defect.

The human-success clause is worth calling out as an improvement over what I asked for. Requiring the human rendering to state that hints are not detected installations carries `ac-0002`'s non-assertion property into the default interactive path, which previously had no specified behavior at all. The strongest misreading risk for this feature was always a human seeing a plausible-looking path and believing the tool had found something; that is now closed on both output modes rather than only the machine one.

## E1 — two JSON-error stream conventions across sibling commands (low, accepted)

Choosing the `output.rs` family for `adapters` is correct, but it leaves the CLI with two different streams for the same class of event. `crates/cli/src/sessions.rs:93` emits its JSON error envelope on **stderr**; `adapters list --json` will emit its failure envelope on **stdout**. A machine consumer that drives both commands must read errors from different streams depending on which command it called.

Stdout is the better choice and `sessions` is the anomaly, so this is not a request to change the new command — and `ac-0005` requires existing session behavior to stay compatible, so `sessions` should not move here either. The divergence is now deliberate rather than accidental, which is the important difference from R1. `docs/cli.md` is already in `tk-0001`'s paths; one sentence there recording which commands emit machine errors on which stream would keep the inconsistency documented instead of discovered. Accepted, not blocking, and I will look for it at candidate review.

## Consistency of the correction itself

The corrections propagated cleanly. `ac-0003` and `ac-0005` were strengthened in `plan.dd.json`, and the identical wording appears in `bp-0003`/`bp-0005` and in `dw-0003`/`dw-0005` in `assets/tasks/phase-1/tasks.dd.json`, so no surface still carries the superseded claim. `assets/backpressure.dd.json` refreshed `meta.basis_sha` to the new plan digest `b5cfc5fb…`, so the survey is not stale against the plan it measures. The R1 report was committed to the plan's own `assets/reviews/` and the R1 receipt ingested as a team review record, so this round's history is inspectable rather than replaced. No decomposition change was made: the unit split, single wave, solo-pm ownership, baseline, composition root and check set are unchanged from the shape R1 affirmed.

## Standing scope of this approval

This approves the design and the decomposition. It is not evidence that the catalog works, because no implementation exists at this subject. The properties above are now contracted and gated rather than merely intended, which is precisely the difference between the R1 and R2 verdicts. A separate composition review against real evidence is still required, and at that review I will read the `adapter_catalog` integration test for genuine hostile-`HOME` byte-identity and inaccessible-store-root scenarios, confirm `production_catalog_ids_match_exported_provenance` iterates the real registry rather than a fixture list, and read the rendered human and failure output for both the `adapters.list` label and the not-detected-installation statement.
