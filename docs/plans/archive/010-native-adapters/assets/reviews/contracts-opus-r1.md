# Plan010 contract review — r1

**Subject** `418a62452af3e766c8a939e6c2a4d9822d559afa` (parent `1f15eea4`, the Plan009 composition commit)
**Scope** decomposition — the new snapshot contract and the 6+1 wave-1 fan-out only
**Verdict** `changes-requested`, scoped: five JSONL lanes are clear to dispatch now; three snapshot-emitting lanes and the storage lane need one contract decision first.

## What I checked and what I did not

Digests recomputed before reading and matching the packet: plan `fdd88eb6…6511`, guide `4f33e554…de2ca`. The diff `1f15eea4..418a6245` is 18 files, purely additive: `crates/core/src/lib.rs` +2 lines, the new `crates/core/src/snapshot.rs`, and the 010 plan assets. No existing crate was modified, so the Plan005 JSONL contracts and the Plan009 catalog are unchanged bytes and I did not re-audit them.

I folded the evidence-only commit `e959bc03ac35c8a0e7bae3699fd874dd70f63226`. Verified evidence-only: `git diff --name-only 418a6245..e959bc03` over `crates`, all `Cargo.toml`, `Cargo.lock`, the plan, the guide and `lane-contracts.json` is empty, and `plan.dd.json`, `impl-guide.dd.json` and `crates/core/src/snapshot.rs` hash identically at both commits (`snapshot.rs` = `f455b99b…a57d0`). The reviewed product subject stays `418a6245`.

No build, test, formatter, lint or rustdoc was run by me. `contract-proof.json` (6 core + 14 testkit, exit 0) and `snapshot-contract-smoke.json` are read as your execution.

## The contract itself is sound

`crates/core/src/snapshot.rs` does the thing it set out to do. Byte cursors and whole-source revisions are now different types rather than one type with a lie in it: `SnapshotRecord.key` is a native key or a structural ordinal, the doc comment says outright that these are "never pretend file byte offsets", and `NativeSnapshot.revision` is a loader-owned digest of the complete bounded representation rather than a position. That is the honest separation Plan009's D3 obligation was pointing at, carried into the input side.

The parallel-safety property that actually matters is present and I confirmed it structurally: `NativeSnapshot`, `SnapshotRecord` and `SnapshotRef` are plain structs with all-public fields and no private constructor, so every mapper owner can build arbitrary inputs in their own tests without the storage lane existing. `SnapshotAdapter::map_snapshot` takes `&NativeSnapshot` and returns `Result<MappedSnapshot, PipelineError>` with no I/O in the signature. The three snapshot lanes genuinely do not wait on tk-0008, which is the load-bearing claim in ac-0009 and the fan_out rationale.

Bounds are typed rather than ambient. `SnapshotLimits::validate` rejects zero limits and the incoherent `max_snapshot_bytes < max_record_bytes` case before use, and `NativeSnapshot::validate` distinguishes `RecordLimit` from `BatchLimit` and uses `checked_add` for the aggregate, so the accumulator cannot wrap into a false pass. `SnapshotRef::validate` requires an absolute path, rejects non-UTF-8 paths and empty session ids, and constrains a SQLite table identifier to ASCII alphanumerics and underscore — which closes identifier injection at the type boundary rather than trusting the storage owner to quote correctly. Your smoke exercises exactly the discriminating cases: the aggregate boundary at 9 versus 10 bytes for an 8-byte key plus 2-byte value, the per-record limit, and table rejection.

`SnapshotFormat` is a closed tagged enum, so adding a fourth storage shape is a deliberate PM edit rather than a string that any lane can invent. Good.

## Findings

### C1 — snapshot provenance attribute vocabulary is undefined, and the sealed proof requires an attribute snapshots cannot honestly emit (medium, open)

This is the one that should move before three of the seven lanes start.

`crates/testkit/src/bin/proof/collection.rs` hard-requires a fixed attribute set on **every** record it validates: `unisphere.source.adapter`, `unisphere.source.path`, `unisphere.source.offset`, `unisphere.source.kind`, `unisphere.profile.version`, returning `Err("missing unisphere.source.offset")` otherwise. `lane-contracts.json` tells snapshot lanes the opposite: "Include source key/session/revision provenance, not fake `unisphere.source.offset`." Both cannot hold for a snapshot-derived record. One of them has to give, and the resolution lives in `crates/testkit/**`, which is tk-0009's path — no coder can touch it.

Worse for parallelism, the replacement names do not exist anywhere. I grepped the whole tree at the subject: the only provenance vocabulary in the repository is `unisphere.source.{adapter,path,offset,kind,record.id,parent.id,is_sidechain}` plus `unisphere.profile.version`. There is no `unisphere.source.revision`, no `unisphere.source.key`, no snapshot equivalent in `snapshot.rs`, in the guide, or in `lane-contracts.json`. So tk-0005 (snapshot half), tk-0006 and tk-0007 (IDE half) will each independently invent attribute names and then hard-code them into their per-dialect fixtures and regressions. Three lanes, three vocabularies for one concept, reconciled by PM after the fact — with the fixture assertions rewritten in three crates.

This directly contradicts ac-0009's claim that all six owners can implement against frozen types without sibling dependencies. It is true for the five JSONL lanes. It is not yet true for the snapshot lanes, because the frozen types stop short of the provenance surface those lanes must emit, and ac-0008 makes provenance an acceptance requirement.

The fix is one sentence in `lane-contracts.json` before dispatch: name the exact keys snapshot-derived records carry (for example `unisphere.source.key`, `unisphere.source.revision`, optional `unisphere.source.session`), and state explicitly whether `unisphere.source.offset` is absent for snapshot records — plus that the proof's required set becomes conditional on the source kind, owned by tk-0009. Cheap now, three-crate rework later.

### C2 — tk-0007's declared interface contradicts itself and `lane-contracts.json` on names the PM must import (medium, open)

Two authoritative documents disagree on a type name, and PM alone writes the registration:

- Guide unit tk-0007 interface: "a separate **CursorAdapterSnapshot** implementing SnapshotAdapter".
- `lane-contracts.json` tk-0007: "**CursorIdeAdapter** implements SnapshotAdapter".
- The guide's own boilerplate sentence, repeated in every unit, also says `CursorIdeAdapter`.

The same unit also says "pub const DESCRIPTOR: AdapterDescriptor with id **cursor**" and then, two sentences later, "DESCRIPTOR.id=**cursor-transcript**; IDE_DESCRIPTOR.id=cursor-ide. These supersede generic cursor slug." A coder following the first half ships `id = "cursor"`. That id is not an internal detail: Plan009's integration test asserts exact ids on the `adapters list --json` wire, so a wrong id is a published wire value, and its D1 test asserts the same string appears as emitted provenance.

Related omission: tk-0005's guide interface names `CopilotCliAdapterSnapshot` and one `DESCRIPTOR` with id `copilot-cli`, and never mentions the second descriptor. Only `lane-contracts.json` carries `SNAPSHOT_DESCRIPTOR id copilot-cli-snapshot`. A coder reading their unit packet literally ships one descriptor and PM discovers the missing one at composition.

Fix: make the guide unit interfaces state each exported const name and its exact id, delete the superseded "id cursor" clause, and settle `CursorIdeAdapter` versus `CursorAdapterSnapshot` in both files. Also worth deleting the Cursor boilerplate sentence from the five units where it is irrelevant — it currently appears verbatim in tk-0002 through tk-0008, which is how the contradiction propagated.

### C3 — Plan009's D1 provenance invariant has no owner for snapshot registrations (medium, open)

The production provenance test I cleared in the Plan009 composition review iterates `&ADAPTERS` and, for each entry, invokes `(registration.run)` and asserts the emitted `unisphere.source.adapter` equals `registration.descriptor.id`. Its value was that coverage extends automatically to the next registration. `AdapterRegistration` holds `{descriptor, run}` and the test drives the real runner with no source.

`SnapshotAdapter` has no runner, and running one meaningfully requires a real snapshot source. So at composition either snapshot descriptors join `ADAPTERS` with runners the D1 test will invoke without a source, or they are registered somewhere else and D1 silently stops covering three of the new identities while still passing. Neither the guide's tk-0009 interface ("App composes each mapper/loader/writer and catalog once") nor `lane-contracts.json` says which, and no check in the guide names the invariant.

This is not blocking for any coder. It is blocking for the seal being worth what it was worth last plan, and it needs a named owner and an explicit decision in the tk-0009 interface before composition, not after.

### C4 — nobody is contractually required to call `validate` (low, open)

`SnapshotLoader::read_snapshot` returns `Result<NativeSnapshot, PipelineError>` and takes `SnapshotLimits`, but neither the trait doc nor `lane-contracts.json` says the loader must return only snapshots satisfying `validate(limits)`, and no mapper is told it may assume that. `validate` is also inherently post-hoc — it measures a `NativeSnapshot` that has already been materialized, so it cannot itself be the allocation guard. tk-0008's lane text does say "Enforce record and aggregate limits before unbounded allocation", which is the right instruction, but that leaves `validate` as a method with no named caller.

Secondary asymmetry worth deciding while you are there: `max_snapshot_bytes` counts `key.len() + bytes.len()`, while `max_record_bytes` bounds only `bytes`. A single native key of arbitrary length passes the per-record check and is caught only in aggregate. For SQLite the keys come from the store, so they are source-controlled.

Fix: state in `lane-contracts.json` that `read_snapshot` MUST return only snapshots for which `validate(limits)` is `Ok`, that limits are enforced during read rather than after, and that mappers may assume a validated snapshot.

### C5 — every wave-1 unit's declared proof is a command its owner is forbidden to run (low, open)

`impl-guide.dd.json#checks/vd-0002` is `cargo test --workspace --all-targets --locked`, and it is the sole declared proof for all seven wave-1 units, tk-0002 through tk-0008. `lane-contracts.json` worker rule two says: "No builds, tests, formatters, lint, rustdoc, Cargo.lock changes or root Cargo/app/core edits. PM validates and integrates."

So no wave-1 owner can execute their own declared proof, and the root workspace uses `members = ["crates/*"]`, meaning all seven new crates first compile together at tk-0009 — with `--locked` against a lockfile that cannot yet contain tk-0008's `rusqlite` and `sha2`. The seven-crate first compile is a real integration event, not a formality, and every regression those owners write is unexecuted code until then.

I am not asking you to loosen the purity rule; there is a good reason coders do not run workspace-wide commands. But the guide should either say plainly that vd-0002 is PM-executed and wave-1 acceptance is source-only, or give each owner a scoped `cargo test -p <own crate>` inside their own clone. Right now the artifact claims a proof that the unit structurally cannot produce.

## Why changes-requested rather than approved, and what is not blocked

C1 and C2 are decisions, not implementations: two edits to `lane-contracts.json` and one to the guide's unit interfaces. Both are strictly cheaper now than after three crates hard-code divergent attribute names and one crate ships a wrong public descriptor id.

Nothing here blocks the five JSONL lanes. tk-0002 (codex), tk-0003 (oh-my-pi), tk-0004 (pi), the `SessionAdapter` half of tk-0005 (copilot-cli) and the transcript half of tk-0007 (cursor-transcript) all implement the Plan005 `SessionAdapter` contract against approved unchanged bytes, with an established provenance vocabulary and a working fake in `crates/testkit/src/collection.rs`. They should dispatch now. C2's naming fix touches tk-0005 and tk-0007 packets, so settle it before those two go out; it does not affect the other three.

The gated set is tk-0006, the snapshot halves of tk-0005 and tk-0007, and tk-0008 — the lanes whose output has to carry snapshot provenance. Unblocking them is a paragraph of contract text, not a wave.

## Scope of this verdict

This is a review of the frozen contract and the decomposition, not of behavior that does not exist yet. I read committed bytes at `418a6245` plus the evidence committed at `e959bc03`, recomputed the digests named in the packet, and ran nothing. I did not re-audit the Plan005 JSONL contracts or the Plan009 catalog; those are unchanged bytes and already approved. Native dialect facts in `native-structural-research.json`, `vscode-journal-schema.json` and `cursor-snapshot-schema.json` were read as structural schema evidence; I did not verify them against any real installation, and no private payload appears in them.
