# Plan 005 — baseline-source conformance check (baseline r1)

**Verdict: changes-requested.** The frozen contract types are an accurate, high-quality
implementation of the approved C1/C3/C4/C5 surface — I found no divergence in any type, field,
default, enum member, error code or remedy string. But the same commit adds a contract-required
`unisphere-core -> serde_json` edge without updating the dependency allow-list that the repository's
own `architecture` gate enforces, so `harness checks` is red at the commit proposed for source-bound
seal, and the file that fixes it is fenced to wave 2. That is B1 below and it should be closed before
the seal, not after. B2 is a frozen-surface omission worth fixing in the same pass; B3–B5 are cheap.

| Field | Value |
| --- | --- |
| subject_sha | `3df2450324873eedb77aba645edfaa9472de3a56` |
| workspace | `/Users/jordanknight/substrate/unisphere/unisphere-claude-session-pipeline` |
| plan sha256 | `0c28b557a2def3dc075c70403e6768959057c007c428dcb9a4ac23683ac7f830` (unchanged, approved) |
| guide sha256 | `5520d59344dc2f664af3121df3184c344f659a4c1acceca9432ce06abe242777` (unchanged, approved) |
| scope | decomposition — baseline source vs approved contracts only |
| reviewer | `pij-huge-nigel`, omp, `github-copilot/claude-opus-5`, effort high |
| method | committed bytes only; no build, tests, clippy or formatters run |

All eight manifest digests recomputed and matching. During the review the PM began uncommitted SDK
work (`crates/sdk/src/collection.rs`, `crates/sdk/src/lib.rs`); I re-verified every baseline file
against the commit afterwards — all eight unchanged — and read nothing from the WIP. The three future
package directories are absent from both `git ls-tree HEAD crates/` and the working tree.

---

## Findings

### B1 — high, open — the sealed baseline turns the repository's architecture gate red

`crates/core/Cargo.toml` now declares `serde_json.workspace = true`. That dependency is *correct and
contract-required*: C4 specifies `TelemetryRecord.attributes: BTreeMap<String, serde_json::Value>`,
and C12 says "Core adds serde_json solely for structured portable values". The problem is the other
half of that same C12 sentence — "**Dependency checks updated to reflect actual approved graph**" —
which this commit does not do.

`crates/testkit/src/bin/unisphere-arch-check.rs:15`:

```rust
("unisphere-core", "normal") => dependency == "serde",
```

`check()` returns `Err` on the first disallowed edge and `main` maps that to `ExitCode::FAILURE`, so
against the real workspace graph the binary now fails with
`forbidden normal dependency: unisphere-core -> serde_json`. `.harness/extensions/checks/checks.mjs:85`
runs exactly that binary as the `architecture` gate:

```js
['architecture', cargo, ['run', '--locked', '-p', 'unisphere-testkit', '--bin', 'unisphere-arch-check']],
```

with no `--metadata` argument, so it shells `cargo metadata --format-version 1 --no-deps --locked` on
the actual workspace. `harness checks` is therefore red at `3df24503`.

**Why the recorded proof did not catch it.** `assets/baseline-proof.json` records two checks:
`cargo test -p unisphere-core -p unisphere-testkit --lib` and a scoped clippy. Neither runs the
arch-check binary against real metadata. The binary's own unit tests read
`crates/testkit/fixtures/architecture/allowed.json`, whose `unisphere-core` package lists only
`serde` — so the synthetic fixture agrees with the stale allow-list and the tests stay green while
the real graph fails. The sensor and its fixture drifted together, which is precisely the failure
mode C13 is trying to prevent elsewhere.

**Why it cannot be left to wave 1.** Unit ownership puts `crates/testkit/src/bin/**` and
`crates/testkit/fixtures/architecture/**` in `tk-0005`, role pm, **wave 2**. `tk-0002`, `tk-0003` and
`tk-0004` list those paths in neither `paths` nor `reads`. So all three wave-1 coder lanes would
start against a repository whose architecture gate is already failing on a file none of them owns and
none of them may edit — the exact cross-lane blockage the fence exists to prevent.

**Fix, before seal, by the PM who owns both `tk-0001` and `tk-0005`:**

```rust
("unisphere-core", "normal") => matches!(dependency, "serde" | "serde_json"),
```

and add the `serde_json` edge to `fixtures/architecture/allowed.json`'s `unisphere-core` package,
bumping the expected edge count in `accepts_only_declared_allowed_edges_in_a_partial_workspace` from
7 to 8 — otherwise the fixture continues to certify a graph the workspace no longer has. Re-running
the `architecture` gate (not just the scoped `--lib` tests) is the proof.

Note for wave 1 planning, not part of this fix: `check()` also rejects any workspace member not in
its hardcoded five-name list (`unisphere-arch-check.rs:54-61`), so the same file must gain
`unisphere-loader-jsonl`, `unisphere-adapter-claude` and `unisphere-output-otlp` plus their C12 edges
at the moment those crates appear. Under the current fence that is a wave-2 file being required by
wave-1 work. Worth resolving deliberately now rather than discovering it three lanes deep.

### B2 — medium, open — the frozen conformance helper asserts three of the four properties C13 names

C13: "testkit supplies reusable pure-adapter conformance helper asserting
**determinism/source-provenance/contentpolicy/outputbounds** on fixture inputs."

`assert_adapter_conformance` (`crates/testkit/src/collection.rs:252-316`) covers the first three well
— double-map equality, per-record `event_name`/`profile.version`/`source.adapter`/`source.path`/
`source.offset`/`source.kind`, `body.is_none()` under default options, and provenance stability across
content modes. It asserts nothing about output bounds, and `MAX_OUTPUT_BATCH_BYTES` appears nowhere in
the crate.

This matters more than a missing line usually would because of where it sits. The helper is frozen at
wave 0; `tk-0003` and `tk-0004` list `crates/testkit/src/collection.rs` under `reads`, not `paths`. If
the assertion is not added now, no lane can add it later without breaching the fence, and every
adapter that "passes conformance" will have passed a check that is silent about the one bound the
writer must later enforce. It is implementable at wave 0 with what the crate already has —
`TelemetryRecord` is `Serialize` and testkit already depends on `serde_json` — e.g. assert that the
serialized mapped batch stays within `MAX_OUTPUT_BATCH_BYTES` for both content modes.

While there: the fixtures deliberately carry `SENSITIVE-USER-CONTENT`, `SENSITIVE-REASONING`,
`DO-NOT-PROMOTE-SIGNATURE`, `SENSITIVE-ARGUMENT`, `SENSITIVE-TOOL-RESULT`, `SENSITIVE-UNKNOWN-PART`
and `SENSITIVE-UNKNOWN-RECORD` markers so that leakage is mechanically detectable, yet nothing in the
shared helper scans a metadata-only mapping for them. A generic helper cannot know the markers, so
leaving that to each lane's own tests is defensible — but it is a decision, and right now it is an
implicit one. Recording it here so `tk-0003` inherits it explicitly.

### B3 — low, open — "blank line" is defined twice and not identically

C3 says blank lines consume budget and cursor but are omitted from returned records.
`fixture_records` (`crates/testkit/src/collection.rs:23`) implements blank as
`line.iter().all(u8::is_ascii_whitespace)` — so `" \n"` and `"\t\n"` are dropped, and the test
`fixture_framing_preserves_physical_offsets_across_blanks_and_crlf` pins that reading with a
space-only first line. A `tk-0002` author reading C3 could just as reasonably implement blank as an
empty physical line, in which case the real loader returns a whitespace-only `NativeRecord` that the
fixture path never produces: adapter tests stay green while the live pipeline emits diagnostics for a
record shape it was never exercised on. The definition is cheap to pin in C3 or in the helper's doc
comment, and it should be pinned in the frozen artifact rather than discovered by divergence.

### B4 — low, open — `CollectionApi` object safety is claimed but never exercised

C1 states "Both methods are object-safe", and C11 has the CLI take `&dyn CollectionApi`. I verified
dyn-compatibility by inspection — both methods take `&self`, neither is generic, neither mentions
`Self` in return position, and `Send + Sync` supertraits are fine — so the risk is low. But nothing at
baseline coerces to `dyn CollectionApi`; `fake_collector_returns_checkpoint_only_when_output_is_accepted`
calls through the concrete `FakeCollector`. By contrast `SessionAdapter` *is* proven, because
`assert_adapter_conformance` takes `&dyn SessionAdapter`. Adding `let _: &dyn CollectionApi = &fake;`
to the existing test converts a stated property into a compile-time one at wave 0, inside a file
`tk-0001` already owns, instead of first testing it in wave 2 when the CLI depends on it.

### B5 — low, open — two pre-seeded false positives for the C13 source scan

Recording these now so `tk-0005` does not build a sensor that fails on correct code:

- `crates/core/src/lib.rs:4` is `#![forbid(unsafe_code)]`. A naive `unsafe` term in the denylist flags
  the exact attribute that establishes the property being checked.
- `crates/testkit/src/collection.rs:10-11` legitimately uses `include_bytes!` for the fixtures. C13
  scopes the scan to production core and adapter-claude sources, so the enumerated file list must
  exclude testkit rather than relying on a repo-wide sweep.

I scanned the five committed core production sources for the C13 denylist
(`std::{fs,env,process,net,thread}`, `SystemTime`/`Instant::now`, `unsafe`, `extern "`,
`include_str!`/`include_bytes!`): zero hits outside that one `forbid` attribute. Core is clean.

---

## Verified conforming

- **Manifest integrity.** All eight `baseline-source-manifest.json` digests recomputed and matching;
  no unmanifested source file changed in the commit.
- **C1 exactly.** `SourceScope`, `SessionRef`, `SourceIdentity`, `ReadCursor`, `ReadLimits` (defaults
  128 / 1_048_576 / 4_194_304), `NativeRecord`, `LoadedBatch`, core-owned `CollectionBatch`
  `{mapped, next_cursor, more, incomplete_tail}` and `CollectionApi` — every field name, type and
  ordering as specified. No sessionId invented from a path. `FakeCollector`, `FakeSessionLoader`,
  `FakeRecordWriter` all present with call recorders.
- **C3.** `ReadLimits::validate` rejects any zero and `max_batch_bytes < max_record_bytes` as
  `InvalidInput`; `SourceScope::validate` rejects `max_sessions == 0`. `NativeRecord.bytes` documented
  as excluding LF and retaining CR, matching the CRLF test. `LoadedBatch::validate` goes beyond the
  contract in a good direction — it re-checks limits at the service boundary even for injected
  loaders, binds `next_cursor.source` to `source.path`, and rejects overlapping or past-the-cursor
  offsets — with `checked_add` and `u64::try_from` throughout, so the "checked arithmetic" clause is
  real rather than aspirational.
- **C4.** `TelemetryRecord`, `MappingOptions` (`include_content` defaults false via `Default`),
  `MappingDiagnostic`, all five `MappingDiagnosticCode` variants, `MappedBatch`, and `SessionAdapter`
  with `name()` and `map() -> Result<MappedBatch, PipelineError>`.
- **C5.** All ten `PipelineErrorKind` variants; `new`/`kind`/`offset`/`code`/`message`/`fix`
  accessors; all ten codes spelled exactly as the contract lists them; `Display` and `Error`; private
  fields so no caller can inject text; every `message`/`fix` a fixed `&'static str` with no OS, serde
  or source bytes. The four limit remedies are genuinely distinct and name the right knobs, and
  `RecordLimit`'s says "no record was skipped" — the F2 guarantee, surfaced where a user reads it.
- **C9 constant.** `MAX_OUTPUT_BATCH_BYTES = 32 * 1024 * 1024` exported from core, and the
  `OutputLimit` remedy quotes `33554432` — consistent, and pinned by a test.
- **C13 fixtures.** Small but genuinely adversarial: shared assistant `message.id` across records,
  `isSidechain`, `thinking` with `signature`, `tool_use` / `tool_result` with nested and null and
  boolean content, `is_error`, an unknown *part* type, an unknown *record* type, records with and
  without `timestamp`, and a `toolUseResult.persistedOutputPath` pointing at
  `/do/not/open/private-spill` — so "the adapter never opens sidecars" is testable rather than
  asserted. `TextFixtureAdapter` is a working second adapter, not a stub.
- **No stubs.** `git grep` over `crates/core` and `crates/testkit` at the commit finds no `todo!`,
  `unimplemented!` or `TODO` — `tk-0001`'s "no loader/mapper/writer stubs" holds.
- **Test count.** 6 core + 14 testkit `#[test]` functions = the 20 the PM reports. The four core
  collection/limit tests assert observable behaviour (rejection kinds, offset identity, remedy
  strings), not plumbing.
- **F13 obligation discharged.** C12 required the actual baseline run to prove workspace independence
  with all three future package directories absent. `baseline-proof.json` records both checks at exit
  0 with those three absolute paths listed absent, and both necessarily resolved the workspace — so
  the uninherited path declarations for `unisphere-loader-jsonl`, `unisphere-adapter-claude` and
  `unisphere-output-otlp` are confirmed inert on real Cargo, not merely by Plan 001 analogy. `time`
  and `libc` are likewise declared once at root per C12.

## Scope and honesty

Committed bytes only. I ran no build, tests, clippy or formatter, so PM `vd-0001` (20 tests, scoped
clippy) is reported as the PM's recorded runtime evidence, not as something I reproduced. B1's failure
is derived statically from the allow-list, the manifest and the gate's argv — certain by construction,
but not executed here; running the `architecture` gate will confirm it in seconds. Loader, adapter and
writer behaviour is out of scope and unimplemented. No design contract was reopened and no profile
question revisited; the approved plan and guide are treated as fixed.
