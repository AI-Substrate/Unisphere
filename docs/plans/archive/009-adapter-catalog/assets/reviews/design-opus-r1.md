# Plan009 adapter catalog — independent design review (r1)

- Scope: decomposition. Subject `2e3109c1273c38df76c9db5fb2f082c28a48eb5b`.
- Plan `docs/plans/009-adapter-catalog/plan.dd.json` sha256 `31e3ccf47289285ba10f64bce57d74952cce8dd740f5f162939975bbaf18fb5c`.
- Guide `docs/plans/009-adapter-catalog/assets/impl-guide.dd.json` sha256 `d114a779c29efbd2bfccd530e03749c9e41b829f3e78070c65a89f3e7984a565`.
- Reviewer: `pij-huge-nigel`, omp, github-copilot/claude-opus-5, effort high.
- Verdict: **changes-requested**. Three contract items should be settled in the guide before code encodes them; two further findings are bounded and accepted for candidate review.

No implementation exists yet, and I did not treat its absence as a defect. I ran no build, test, linter or formatter, and edited no plan, guide or product source. Everything below is read from committed bytes at the pinned subject.

## What the design gets right

**The fan-out decision is correct.** Six ACs, one coupled contract, one wave, one unit. Splitting typed metadata from executable dispatch would manufacture exactly the drift `rk-0002` names, because the two halves would then be separately editable. Solo-pm here is not a shortcut, it is the shape that makes the single-registration invariant enforceable at all.

**The registration collapse is the right mechanism.** Today `AdapterRegistration` in `crates/app/src/adapters.rs:8` carries `name` plus `run`, and `dispatch` selects on that name at line 45. Folding the descriptor into the same struct and selecting on `descriptor.id` means a new adapter cannot appear in one surface and not the other, because both surfaces read one const array. That is a structural guarantee rather than a documented convention, and it is what ac-0005 actually needs.

**Pure metadata in core is enforceable, not merely asserted.** This is the strongest thing in the design and I want to be explicit about why. `crates/testkit/src/bin/unisphere-arch-check.rs:243` scans `crates/core/src` recursively, and `check_source` at line 137 rejects production `std::env`, `std::fs`, `std::process`, `std::net`, `std::thread`, clock calls and `include_str!`/`include_bytes!`, including the grouped-import form. A new `crates/core/src/catalog.rs` is swept by that scan the moment it exists, with no fixture or allowlist edit. So ac-0004's "no home/config environment expansion or command execution" is structurally true for the descriptor data itself, and `harness checks --json` already runs the gate. That is a real proof, and the design earned it by putting the DTO in core.

**No architecture-policy change is required, and I checked rather than assumed.** `allowed()` at line 13 already permits `unisphere-core` → `serde`/`serde_json` and `unisphere-cli` → `serde_json`. The catalog adds no crate and no dependency edge, so the allowlist is untouched. This is the same class of gap I raised as B1 on Plan005; here it does not recur.

**The capability booleans are truthful against the shipped code, with one exception I raise below.** `export_platforms=[unix]` matches a loader that is `#[cfg(unix)]`-gated and returns `Unsupported` elsewhere. `output_formats=[otlp-jsonl]` matches the only writer. `cli_persisted_resume=false` matches `docs/fidelity.md:56`, "the SDK returns a caller-owned cursor, but the CLI persists no checkpoint". `delayed_revision_reconciliation=false` and `lossless_archive=false` match the fidelity table's Unsupported-behavior classifications. Nothing here overclaims by omission.

**Symbolic `base=home` is the correct primitive.** Emitting `base:"home"` with a relative `.claude/projects` and a `*/*.jsonl` glob makes "this is where such stores usually live" structurally distinguishable from "this store exists here", which is exactly what ac-0002 asks for. An expanded absolute path could not carry that distinction no matter how it were labelled.

## Findings

### D1 — one registration, but still two identity strings (medium, open)

The plan's invariant is that metadata and dispatch derive from one registration, and PM has confirmed `descriptor.id` is the sole dispatch and catalog name. But the identity a consumer ultimately cares about is the one stamped on exported telemetry, and that comes from a different place. `SessionAdapter::name()` is declared at `crates/core/src/collection.rs:211`; `crates/adapter-claude/src/lib.rs:27` returns `"claude-code"` from it and line 86 independently writes the literal `"claude-code"` into `unisphere.source.adapter`. The shared conformance helper binds those two together — `crates/testkit/src/collection.rs:286` asserts the emitted attribute equals `adapter.name()` — so that pair is proven for any adapter run through it.

`descriptor.id` would be a third string with no binding to either. Nothing in the guide requires `descriptor.id == adapter.name()`, and no check compares them. The failure it permits is quiet and directly contrary to the product's purpose: `adapters list --json` advertises id X, `sessions export --adapter X` dispatches correctly, and the exported records are attributed to Y. The catalog would then be misreporting which adapter produced the telemetry. Today the strings coincide by literal accident, so no gate would notice, and the ac-0005 fixture test would not notice either, because it would register its own descriptor id next to `TextFixtureAdapter::name()` and both would read `fixture-text` by hand.

Smallest fix: one behavioral test that iterates the production registry, exports a fixture through each entry's `run`, and asserts the emitted `unisphere.source.adapter` equals that entry's `descriptor.id`. That scales to every future registration instead of pinning one pair. A one-line contract sentence stating the equality is the minimum.

### D2 — `limitations` is declared but undefined, and duplicates the false booleans (medium, open)

The architecture contract lists `AdapterDescriptor {id, application, description, locations, capabilities, limitations}`. `limitations` appears exactly once in the guide and nowhere else: no type, no content rule, no AC binding, no check. Meanwhile `AdapterCapabilities` already carries the same three negatives as `cli_persisted_resume`, `delayed_revision_reconciliation` and `lossless_archive`, all false.

So the same facts have two representations with no single source of truth and no mechanism keeping them consistent. Prose limitations saying one thing while a boolean says another is precisely the metadata drift `rk-0002` exists to prevent, and it would ship inside a v1 machine envelope where consumers can depend on both.

Smallest fix: either drop `limitations` and let the false booleans carry the negatives, or define it as a static slice of stable machine tokens with the invariant that every false capability boolean has exactly one corresponding token and no token exists without one. The second option is worth the extra line because it also gives D3 somewhere to live; either way the choice should be made before the field is serialized into v1.

### D3 — `sdk_cursor_resume: true` is the only positive claim and it overstates the cursor (medium, open)

Every other capability boolean is false, so this one field carries the design's entire affirmative resume claim, and ac-0003 explicitly asks for truthful separation of supported from unsupported. `docs/fidelity.md:23` is precise about what the cursor does and does not do: it binds path/device/inode/offset, supports appended complete lines, defers partial tails and reports observed replacement or truncation below the checkpoint — and there is **no same-inode rewrite/regrowth or truncation-above-checkpoint detection**.

An unqualified `sdk_cursor_resume: true` in a machine-readable catalog invites a consumer to assume resume is safe across source rewrites, which is the case the implementation explicitly does not detect. The name is doing the overclaiming: the honest fact is that the SDK hands the caller a cursor, not that resume is sound under arbitrary source mutation. This is cheap to fix now and expensive later, because renaming or requalifying a field in a published v1 envelope breaks the consumers the plan is being built for.

Smallest fix: rename to something that describes the mechanism rather than a guarantee, for example `sdk_caller_owned_cursor`, and/or pair it with an explicit limitation token naming undetected same-inode rewrite, regrowth and truncation above the checkpoint. Resolving D2 first gives this a natural home.

### D4 — the discriminating proofs live only in backpressure prose (low, accepted)

`assets/backpressure.dd.json` names genuinely good proofs: bp-0002 repeats the real command under hostile `HOME`/`CLAUDE_CONFIG_DIR` and compares bytes; bp-0004 runs the catalog under inaccessible synthetic store roots and confirms no collector call. Neither is bound by anything in the guide. Every capability row points at the same two checks, and neither discriminates: vd-0002 is `cargo run … adapters list --json` whose only signal is exit 0, and vd-0003 is `harness checks --json`, which runs whatever tests happen to exist. A board can therefore go green with the hostile-environment test never written.

This matters more for the renderer than the DTO. Core is covered structurally by the arch-check source scan, but `crates/cli/src` cannot join that scan: `crates/cli/src/sessions.rs:5` legitimately imports `std::fs::OpenOptions` for explicit `--output`, so a crate-wide lexical sweep would fail on it. The CLI catalog renderer is exactly where a friendly `$HOME` expansion would plausibly creep in, and only a behavioral test would catch it.

Accepted rather than blocking: the proofs are already specified at the row level, the same agent writes both the rows and the code, and the candidate review sits downstream of real evidence. I will check for a hostile-environment byte-identity test and an inaccessible-store-root test by name at candidate review, and their absence will be a finding then. Naming them in tk-0001's acceptance now would make that unnecessary.

### D5 — the failure envelope label and the human rendering are unspecified (low, accepted)

The contract pins the success envelope as `{ok:true,command:"adapters.list",v:1,data:{adapters:[...]}}` and otherwise says to reuse existing conventions. There are two incompatible existing conventions. `crates/cli/src/output.rs` is mode-aware and hardcodes `"command":"config.check"` in both the report arm at line 47 and the failure arm at line 65; `crates/cli/src/sessions.rs:93` is mode-unaware and emits `"command":"sessions"` as a JSON line on stderr with help going to stdout as plain text. A literal reuse of the first family yields a machine error envelope for `adapters list` labelled `config.check`.

Separately, `args::mode` at `crates/cli/src/args.rs:106` returns `Human` whenever stdout is a terminal, so `unisphere adapters list` with no flags is the default interactive path — and no AC or contract line describes what it prints. ac-0001 specifies only `--json`; the human path appears solely in bp-0006's proof prose.

Accepted: these are unmade decisions rather than wrong ones, `crates/cli/src/output.rs` and `args.rs` are already in tk-0001's paths, and `after_help` in `args.rs:12` sets the precedent of announcing an intercepted top-level command. Smallest fix is one contract sentence naming the failure command label and the human shape. I will read the rendered output at candidate review.

## Not in scope

I did not revisit Plan005 findings or its accepted dispositions, and I did not treat the missing implementation, the absent `assets/team/baseline.dd.json` receipt or the untracked `node_modules` ignore issue as design defects. One observation on the baseline for the record, raised as no finding: `vd-0001` is `cargo test -p unisphere-core --lib`, which exercises inherited behavior but does not prove the two frozen files are unchanged — the digest manifest in the baseline receipt is what does that. The freeze is otherwise well chosen, since neither `crates/core/src/collection.rs` nor `crates/core/src/ports.rs` appears in tk-0001's paths.

## What would make this approved

Settle D1, D2 and D3 in the guide — one binding sentence plus a named test for D1, a defined or deleted `limitations` field for D2, and an honest name or paired limitation token for D3. No decomposition change is required: the unit split, wave, ownership, baseline and composition root are all correct as written, and the implementation can proceed on the same shape once the three contract lines are settled.
