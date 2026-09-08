# Plan 010 — native snapshot contract and 6+1 fan-out, focused review R2

- Reviewer: `pij-huge-nigel` (omp / github-copilot/claude-opus-5, effort high)
- Scope: `decomposition` — focused disposition of R1 findings C1–C5 only
- Subject: `3a635c17110521a3ed93d03daf5eface57dc0bfd`
- Plan: `docs/plans/010-native-adapters/plan.dd.json` sha256 `fdd88eb6925cc55243d76f6e5d4f682cf91ac8daa3e5eddc6cdc78915ccd6511`
- Guide: `docs/plans/010-native-adapters/assets/impl-guide.dd.json` sha256 `38d5a0d69c2b56da77e5f2f4648d0592586e4df4a76f0523a1c2a67c3a45c475`
- Prior round: `contracts-opus-r1.md` sha256 `1cd645df8d25cb4600d8452ff6a913b155dc680abe964e6d3e4f76423ef8fd31`, receipt `.harness/temp/plan010-contract-review-r1.dd.json` sha256 `4d1eed8bc8c666a6311498655d513f8f44b228a5f7b6164785aa24b393770c6e` — both byte-identical at this subject; R1 is superseded in disposition, not amended in content.

**Verdict: approved.** C1, C2, C3, C4 and C5 are each fixed at the text level they were raised at. All seven wave-1 lanes may dispatch.

## What this round did and did not re-examine

The correction commit is documentation only, and I verified that rather than accepting the claim. `git diff --name-only e959bc03..3a635c17 -- crates '*.toml' Cargo.lock` is empty; the diff is ten files, all under `docs/plans/010-native-adapters/assets/`, +221/−24. `crates/core/src/snapshot.rs` still hashes `f455b99bd430d1f14ca512fa3516f00a02b42f3dccb2fbdaf1c295de330a57d0` and `plan.dd.json` is unchanged, so the R1 contract analysis stands unmodified and is not repeated here. The Plan005 JSONL contracts and the Plan009 catalog remain untouched bytes and were not re-audited, per packet constraint one. I ran no build, test, formatter, lint or rustdoc, and changed no plan, guide, lane-contract or product source.

## Disposition of R1 findings

### C1 — snapshot provenance vocabulary — **fixed**

The exact attribute set is now pinned identically in `lane-contracts.json#snapshot_contract` and `impl-guide.dd.json#architecture/contracts`: `unisphere.source.adapter` = descriptor ID, `.path` = `source.path`, `.key` = native DB key or structural document/journal location, `.revision` = `snapshot.revision`, `.format` ∈ `json_document|json_journal|sqlite_key_value`, `.kind` = actual native kind, `unisphere.profile.version`, optional `.session.id` as a verified selected session ID.

Three things make this a real fix rather than a restatement. First, the `format` values are exactly the serde `rename_all = "snake_case"` tags of `SnapshotFormat`, so the wire value and the type cannot drift apart silently. Second, `.session.id` follows the established dotted-suffix convention already used by `unisphere.source.record.id` and `.parent.id`, so it will not read as a novel namespace at composition. Third, and the part that actually closed the finding: **`unisphere.source.offset` MUST be absent for snapshots**, with `PM tk-0009` named as owner of making the proof's required set conditional on registered source representation, JSONL keeping real byte offsets.

That last clause is what R1 was asking for. The conflict was never that the names were missing in isolation — it was that `crates/testkit/src/bin/proof/collection.rs` unconditionally returns `Err("missing unisphere.source.offset")` while lane text forbade faking it, and the resolution sat in a path no coder may touch. An owner is now named for the only file that can resolve it, and the three snapshot lanes have concrete keys to write into fixtures instead of inventing three vocabularies. Independent implementability restored.

### C2 — contradictory type, const and dialect identities — **fixed**

Every unit interface was rewritten and the propagating boilerplate deleted. `tk-0007` now reads `CursorAdapter implements SessionAdapter; pub const DESCRIPTOR has id cursor-transcript. CursorIdeAdapter implements SnapshotAdapter; pub const IDE_DESCRIPTOR has id cursor-ide` — the `CursorAdapterSnapshot` type name and the superseded `id cursor` clause are both gone, and it now agrees with `lane-contracts.json` verbatim. `tk-0005` names both exports explicitly, including `SNAPSHOT_DESCRIPTOR` id `copilot-cli-snapshot`, which previously existed only in the lane file. The irrelevant Cursor sentence is removed from `tk-0002`, `tk-0003`, `tk-0004`, `tk-0006`; each is now one line naming its own type, its own const and its own id, deferring provenance detail to `lane-contracts.json`.

I grepped the whole tree at the subject for the old identifiers. The only surviving hits for `CursorAdapterSnapshot` and `id cursor` are inside my own R1 report quoting the text being fixed. `CopilotCliAdapterSnapshot` survives because it is the intended type name and is consistent across both files. The `.md` mirror carries the same corrected strings, so a coder handed the rendered guide sees the same names as one handed the JSON — that mattered, since the mirror was how the duplication spread in the first place.

### C3 — no owner for the Plan009 provenance invariant — **fixed**

`lane-contracts.json` gains a `registration_contract`, mirrored into the guide's contracts block and into `tk-0009`'s interface: one descriptor+runner registry covering JSONL and snapshot variants, each registration carrying its source representation, and the production provenance regression creating a representation-appropriate real synthetic JSONL / JSON / journal / SQLite fixture per registration, running an actual export, and asserting every emitted record and snapshot manifest carries that descriptor id. It closes with the two prohibitions that matter: no second metadata-only registry, and no snapshot escape from provenance coverage.

This is the shape R1 asked for and slightly better than the minimum. The value of Plan009's D1 was that coverage extended automatically to the next registration; a single typed registry with a per-registration representation preserves that property across a heterogeneous adapter set instead of forking it into a parallel snapshot path that ages differently. Naming "real synthetic fixture per registration, actual export" also blocks the cheap degradation — a metadata-only assertion that never exercises a runner.

### C4 — unowned `validate`, key-byte asymmetry — **fixed**

The loader postcondition is now explicit and layered: the loader validates input before I/O, enforces limits *during* read, and returns only `NativeSnapshot` values for which `validate(limits)` is `Ok`; the PM snapshot collection service revalidates injected loader output against source and limits before mapping; pure mappers may assume validated bounded input. That gives `validate` two named callers and preserves the property under injection, which was the actual gap — a caller-supplied `SnapshotLoader` is a trust boundary, and revalidation at the service seam is the correct place to hold it.

The asymmetry is now declared deliberate with its reasoning stated: `max_record_bytes` bounds raw value bytes, `max_snapshot_bytes` bounds native UTF-8 key bytes plus raw value bytes, explicitly including a single large key. I raised it as a question, not a defect, and a deliberate documented answer closes it.

### C5 — proof no owner may execute — **fixed**

`vd-0002`'s description is now "PM-executed: scoped delivered-clone mapper/storage tests before import, then this coordinated workspace regression after lockfile update; workers supply unexecuted source/fixtures", and a seventh worker rule states the same from the owner's side, ending with "The no-worker-validation rule is deliberate and unchanged."

This is the resolution I preferred: the purity rule is not loosened, and the artifact stops declaring a proof the unit structurally cannot produce. Naming *scoped per-crate tests in each delivered clone before import* also removes the real risk behind the finding — seven crates meeting a compiler for the first time simultaneously at `tk-0009`, with failures attributable to nobody in particular.

## Residual, non-blocking

**D1 — `unisphere.profile.version` is pinned as `"1"` where production emits `1`.** The corrected text writes `unisphere.profile.version = "1"` with contrastive quotes, while every other value in the same sentence is unquoted. Production emits a JSON number: `crates/adapter-claude/src/lib.rs:85` is `("unisphere.profile.version".into(), json!(1))`, and the inherited conformance helper at `crates/testkit/src/collection.rs:283` asserts `record.attributes["unisphere.profile.version"] == serde_json::json!(1)` — an equality assertion, not a presence check, so a string `"1"` fails it for every adapter routed through that helper.

I am not holding the seal on this, for one reason: `crates/testkit/src/collection.rs` is item four of the mandatory `read_first` list, so any owner following the instructions sees `json!(1)` before writing a fixture, and the frozen approved emission wins over prose on sight. The exposure is an owner who reads the pinned attribute list and not the fake. Cost of removing it entirely is deleting two quote characters in `lane-contracts.json` and `impl-guide.dd.json` before packets go out; cost of leaving it is uniform fixture rework across six lanes. Worth the ten seconds, not worth a round trip.

Second, smaller, and genuinely PM-only: `registration_contract` requires each registration to carry its source representation, but `AdapterDescriptor` and `AdapterCapabilities` in `crates/core/src/catalog.rs` are frozen `Copy` structs with no representation field. Adding one is a core edit inside `tk-0001`/`tk-0009`'s own paths and affects the `adapters list --json` wire shape. No coder is blocked by it and no fan-out property depends on it; flagging it only so it is a decision at composition rather than a discovery.

## Basis for approval

The property under review is whether six mapper owners and one storage owner can implement correctly and independently against the frozen DTOs. R1 found that true for the five JSONL lanes and not yet true for the snapshot lanes, on three specific grounds: undefined attribute names with an unresolvable proof conflict, contradictory exported identities, and an unowned cross-cutting invariant. All three are now text-resolved with named owners, and the two lower findings are answered rather than deferred. The contract source is unchanged and its R1 assessment — separate types for cursors and revisions, public-field DTOs with no private constructor, `map_snapshot(&NativeSnapshot)` with no I/O in the signature, `checked_add` on the aggregate bound, closed `SnapshotFormat`, identifier-constrained SQLite table — still holds.

This approves a contract and a decomposition. It says nothing about whether any dialect is mapped correctly, which is what `vd-0002` through `vd-0004` and a composition review are for.
