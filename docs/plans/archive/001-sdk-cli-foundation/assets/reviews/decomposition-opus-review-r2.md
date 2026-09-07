# Independent decomposition re-review (r2) — Plan 001 SDK/CLI foundation

**Reviewer:** `pij-angry-hekarro` (same independent reviewer as the accepted r1 canary; not PM, not an implementation identity)
**Scope:** decomposition, focused on the F1–F9 dispositions and their decomposition consequences
**Subject SHA:** `d325a8f60a3c97739b7c3b16e612110b5db84530`
**Prior:** r1 receipt `review-decomposition-1902f26-pij-angry-hekarro-v2` (`changes-requested`, subject `1902f26…50b5d`) — history, not approval of v3.

## Inputs re-derived, not trusted from the packet

| Path | SHA-256 | Status |
|---|---|---|
| `docs/plans/001-sdk-cli-foundation/plan.dd.json` | `bc208a54522fe9d5d26c87d25270806ad72b6b0a53d9155ae4cff1b2871fe3cc` | matches packet; unchanged from r1; worktree blob == committed blob |
| `docs/plans/001-sdk-cli-foundation/assets/impl-guide.dd.json` | `95da8bf6c3d65077ae18085a379c9c4f780571a7f2ef62d5ac1a65644ae6cdb0` | matches packet; guide v3; worktree blob == committed blob |
| `docs/plans/001-sdk-cli-foundation/assets/backpressure.dd.json` | `5e700e9aa22288422c94ea943c7407a0ddf7ba13f4dad50a5d113697718fc6fc` | matches packet |

`git rev-parse HEAD` returns the subject SHA and `git status --porcelain` is clean, so the reviewed bytes are the committed bytes with no untracked drift. The revision is a single commit, `d325a8f docs: close decomposition contract and proof gaps`, touching guide, backpressure, their generated `.md` views, and the r1 review/receipt/basis snapshots. The plan is untouched — the product promise was not edited to fit the evidence, which is the right direction of travel.

**Verdict: approved.** All five medium findings and all four low findings are fixed in text I verified line by line. No unresolved material finding remains. Two forward-looking observations are recorded as low/accepted below; neither is a defect in v3 and neither blocks guide exit.

---

## Method

I diffed guide v2 → v3 structurally rather than by eye: both documents were parsed and compared leaf by leaf, yielding exactly 33 changed leaves, each of which I read in full. I then re-ran the r1 structural cross-check against v3 from scratch. Results:

- Zero dangling references: every `#checks/vd-*` and `#acceptance_criteria/ac-*` in the guide resolves.
- All 11 ACs still carry exactly one capability owner, and every owner resolves to a declared unit.
- Zero write-fence overlaps across all five units.
- `Cargo.lock` now appears in exactly one fence (`tk-0004`) and is absent from `baseline.files`.
- Waves and dependency edges are unchanged: tk-0001 wave 0; tk-0002/tk-0003/tk-0005 wave 1 depending on tk-0001 alone; tk-0004 wave 2 depending on all four. No sibling read or import was introduced.
- No AC lost proof. The only `.len` decrease anywhere is `tk-0001.paths` 11 → 10, which is the intended `Cargo.lock` removal; `tk-0004.paths` 3 → 4 is its intended arrival.

Read-only throughout. No build, test, formatter, linter or gate was run.

## Dispositions

### F1 — Failure construction surface · **fixed**

`architecture.contracts[4]` now states private fields plus three public constructors — `invalid_configuration(Option<Location>)`, `configuration_read(ReadFailure, Option<Location>)`, `invalid_arguments(Option<Location>)` — and a full accessor set (`kind`, `location`, `read_failure`, `code`/`message`/`fix`, `retryable`). The third constructor is the one that mattered: it is what lets tk-0003 emit `UNI-ARGS-INVALID` from a clap rejection without reaching into a sealed core it does not own. The added clause "code/message/fix copy is core-owned and fixed, never caller-controlled" keeps SDK/CLI parity structural, and the diagnostic-safety rule now names clap Debug output explicitly alongside parser and UTF-8 errors. Both wave-1 lanes can now be dispatched without a pending contract negotiation.

### F2 — nested Cargo manifests under a workspace member · **fixed**

`contracts[2]` adds "No live nested Cargo.toml is stored under testkit fixtures; manifests are Cargo.toml.template/data materialized into isolated temporary roots", and `tk-0005.interface` repeats it operationally: commit templates or graph data, materialize only in an isolated temporary root, calculate SDK path dependencies at runtime from explicit repository context, and bake no machine-private absolute path into committed fixtures. That last clause closes the residual I raised when accepting the approach — a committed template carrying an absolute path back into the checkout would have failed ac-0009 on its own terms. The negative architecture fixtures are covered by the same rule, which they need, since a forbidden-edge fixture is itself a manifest.

### F3 — `Cargo.lock` fence · **fixed** (your resolution, not mine)

Single ownership, stated in four places: `contracts[2]` ("single-owner PM-generated state in tk-0004, not a frozen baseline file"), `tk-0001.interface` ("Lockfile materialization during baseline prep is explicitly authorized PM-generated state outside this unit frozen contract fence, under tk-0004 single ownership"), `tk-0004.interface` ("the sole Cargo.lock fence across the lifecycle, including initial generated lockfile prep before baseline sealing; root Cargo.toml remains tk-0001-owned"), and `composition.steps[1]`. Verified: `Cargo.lock` is in `tk-0004.paths` only, absent from `baseline.files`, and no fence pair overlaps.

I withdraw my counter-proposal. `guide-service.ts:261-269` rejects duplicate fences unconditionally, so listing the path in both PM units would fail the real validator; shipping a guide that fails its own checker to satisfy a reviewer's model of fence semantics would have been the wrong trade. The exception is now written down rather than inferred, which was the condition I said was sufficient. See O1 for the one residual, which is a tool-behavior unknown rather than a guide defect.

### F4 — ambient-read and no-network proof · **fixed**

Three coordinated changes, and the shape is right: the product promise in `plan.dd.json` is untouched while the evidence grade is stated honestly.

- ac-0003 now lists `vd-0002` **and** `vd-000a`, and `vd-000a` requires running the built consumer "under sealed hostile HOME/XDG_*/UNISPHERE_* values in separate subprocesses" asserting unchanged explicit configuration, with the explicit prohibition "never mutate process-global env in parallel tests". That directly answers the edition-2024 `set_var` hazard I raised — the hostile values are set on the child `Command` before spawn, per `contracts[10]`, not on the proof process.
- ac-0008's `path` now says in the artifact itself that no-network is "a restricted dependency/source-surface review claim, not an executed syscall-trace claim", and `bp-0008` drops from `computational` to `human-judgement` with a REVIEW leg naming ambient reads, networking/client calls, process launch, FFI and unsafe escapes.
- A new `contracts[14]` "Proof-grade boundary" states that dependency absence is not a syscall trace, and `review.proof` gains a fifth clause requiring the inspection to be reported separately from executed subprocess tests.

Downgrading a tier is the uncomfortable move and it is the correct one. See O2 on who actually discharges it.

### F5 — toolchain pin · **fixed**

`contracts[13]` now says plainly that `rust-toolchain.toml` is "only a rustup request, not proof of enforcement", records this host's observed state (Homebrew rustc/cargo 1.95.0, no rustup, newer clippy/rustfmt provenance), and requires tk-0005's `checks` to capture actual `rustc`/`cargo`/`clippy`/`rustfmt` version and provenance and enforce a coherent approved suite before any product gate, with a mismatch yielding "a named failed prerequisite, never a green pin claim". `tk-0005.notes` and `vd-000d`'s description carry the same obligation, and no automatic global install is authorized. ac-0009's documented toolchain is now evidence rather than a file.

### F6 — `#[path]` service isolation · **fixed**

`tk-0002.interface` adds the constraint verbatim: service.rs must be self-contained, using absolute `unisphere_core::`/`serde_json::` paths with no `crate::`/`super::` references to facade or sibling modules. `vd-0002`'s isolation test will now compile as specified.

### F7 — human render path never executed · **fixed**

ac-0005 gains `vd-000b`, and `vd-000b` now exercises "explicit --human success and failure as well as captured/default JSON, asserting stdout/stderr routing", while stating "Real TTY detection beyond injected/explicit modes is not claimed." Both halves are right: the reachable behavior is executed against the installed binary, and the unreachable claim is named rather than implied.

### F8 — sealed runtime environment · **fixed**

`contracts[10]` now gives the exact signature `sealed_command(binary:&Path, temp_root:&Path) -> std::io::Result<std::process::Command>`, requires an absolute executable, clears the environment, sets `PATH=""`, retains only explicitly required platform variables, and never inherits `HOME`/`XDG_*`/`UNISPHERE_*`/tokens. Critically it separates the two environments: the proof tool uses an explicit build environment for cargo/rustc, then runs the *built product* through `sealed_command` with Node/OMP/Builder inaccessible. Requiring an absolute binary is what makes `PATH=""` workable rather than self-defeating. "Node unavailable at product runtime" is now a mechanism, not an assertion.

### F9 — oversize inline payload · **fixed**

`contracts[5]`: oversized `Inline` returns `Failure::invalid_configuration(None)` / `UNI-CONFIG-INVALID` **without invoking `ConfigReader`**; oversized `File` is `ReadFailure::TooLarge` mapped to `configuration_read(...)` / `UNI-CONFIG-READ` with file location; neither path attempts an unbounded copy. The "without invoking ConfigReader" clause is the useful part — it makes the inline branch assertable against `FakeReader`'s call recorder rather than merely described.

---

## Observations (new, low, accepted — not defects in v3)

**O1 — the baseline lockfile write is fenced to a wave-2 unit.** `tk-0004` is wave 2 and depends on all four other units, yet its fence authorizes a write that happens during wave-0 baseline prep. The guide documents the exception in four places and this is the only shape that passes the duplicate-fence checker, so it is the right call. The residual is a tool-behavior unknown I did not test and could not test read-only: if `contracts --seal` or `compose --import` ever attributes fence writes with wave-ordering awareness, that one lockfile commit is where it will bite, and the exception will need to be honored by the tool rather than only by the guide. Worth a single dogfood probe at baseline sealing, and worth reporting to `pij-varied-alpaca` if the tool disagrees with the guide.

**O2 — ac-0008's accountable owner cannot discharge one of its own proof legs.** ac-0008 is owned by `tk-0004`, the PM, but its no-network leg is now an explicitly *independent* source-surface review (`bp-0008` REVIEW leg, `review.proof` clause 5). The PM is not independent, so this necessarily falls to the composition reviewer. The mechanism exists and is written down; the risk is purely that the composition review packet omits it and the leg is silently skipped. Smallest safeguard: name the bp-0008 source-surface inspection explicitly in the composition reviewer's packet, the same way this packet named the F1–F9 dispositions. At decomposition stage there is no source to inspect, so nothing is owed now.

**One wording note, no action required.** v3's rewrite of `contracts[2]` dropped the phrase "no empty SDK/CLI/app crates or placeholder behavior". The prohibition survives verbatim in `tk-0001.responsibility` ("No SDK/CLI stub behavior"), in `rk-0002`'s check and in the plan's own risk register, so nothing is lost — but the anti-stub rule now rests on the unit responsibility rather than the architecture contract, which is worth knowing if that text is edited again.

## Evidence and limits of this re-review

- Read-only. Executions were limited to: digest recomputation (`shasum -a 256`) on the three named inputs and on the committed blobs via `git show <sha>:<path>`; `git rev-parse`, `git status --porcelain`, `git log`, `git diff --stat`; in-process structural comparison and revalidation of the parsed guide/backpressure documents. No build, test, formatter, linter or project gate; no commit, push or coder launch.
- Scope was the focused re-review the packet authorized: F1–F9 dispositions plus decomposition consequences of the edits, against the full v2→v3 leaf diff. I did not restart the discovery pass. The r1 report's positive findings on lane independence, contract completeness, fence disjointness, AC ownership and Builder lifecycle fidelity were re-verified structurally against v3 but not re-argued here.
- Native launch root remains the main clone `/Users/jordanknight/substrate/unisphere/unishpere-main`; all subject artifacts were read by absolute path from `/Users/jordanknight/substrate/unisphere/unisphere-sdk-cli-foundation`. The native root is not claimed to be the subject worktree.
- I did not re-probe the toolchain for this pass; F5's disposition is verified as *guide text*, and the host observation it records is the one I made and reported at r1.
- Main's `harness builder guide --check`, DD validation and plan validation results were reported to me, not observed by me. They are structural checks and are not product execution proof; my approval rests on my own reading, not on theirs.
- Runtime honesty: harness `omp`, model selector `github-copilot/claude-opus-5` and effort `high` are observable only as requested configuration. **Provider-side model identity and reasoning effort remain unverified and are not claimed.**
- No product code, guide, plan, flow, backpressure, governance file, canonical `assets/team` receipt or peer file was modified. Only the two paths the r2 packet authorized were written, both new; the r1 report and both r1 receipts were left untouched.
- This approves the decomposition at `d325a8f60a3c97739b7c3b16e612110b5db84530` only. It is not baseline sealing, not dispatch authority, and not an implementation release; every check in the guide remains future work and none was executed.
