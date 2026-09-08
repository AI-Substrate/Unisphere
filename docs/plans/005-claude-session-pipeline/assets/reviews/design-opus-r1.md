# Plan 005 — independent design (decomposition) review r1

**Verdict: changes-requested.** Three high findings are open contract decisions that must land
*before* `tk-0001` freezes `crates/core/src/collection.rs`, because all three are baseline types or
baseline-frozen semantics that the three coder lanes cannot renegotiate afterwards.

| Field | Value |
| --- | --- |
| subject_sha | `21a9f98787ad8f3de4b00e0f8b715c8600db643e` |
| workspace | `/Users/jordanknight/substrate/unisphere/unisphere-claude-session-pipeline` |
| plan sha256 | `ff6bcc7faa386dc47145da1f678afeef48b4c4b2e9af5bcf0783d91988a3218b` |
| guide sha256 | `f2155e8adbe9e2ce3ba440c6dc790cbff25ad2c918a6547f05b4bf7ae2b81a4d` |
| reviewer | `pij-huge-nigel`, omp, `github-copilot/claude-opus-5`, effort high |
| scope | decomposition (design before implementation) |
| method | read-only; committed bytes at HEAD == subject_sha; no builds, tests, linters or formatters run |

Working tree at review time was clean for every reviewed path (`git status --porcelain` showed only
untracked `.dd/schemas/builder/work-packet/`), so the bytes read are the committed bytes. Independently
recomputed plan/guide digests equal those recorded in `assets/design-validation.json#basis`, so the
PM's structural receipts are honest about their subject.

## What is right

Worth stating, because these are the parts a later reviewer should not relitigate:

- The user's mandate is actually encoded, not merely diagrammed. `SessionAdapter::map(&self, source,
  records, options)` (guide C4) takes bytes and returns `MappedBatch`; it has no loader, reader,
  writer, clock or path handle. `SourceScope`/`SessionRef`/`ReadCursor` live behind the loader.
- The lane cut is real. `tk-0002`/`tk-0003`/`tk-0004` each read only `tk-0001`'s frozen paths, and none
  imports a sibling. That is a genuine three-way fan-out, not three serialized tasks.
- Honesty discipline is unusually good and should be preserved verbatim: no `gen_ai.usage` totals until
  provider counter-inclusion semantics are proven (C8), `event_name=unisphere.session.record` instead of
  a fake `gen_ai.client.inference.operation.details` (C6), repeated `message.id` records kept distinct by
  provenance rather than deduped or summed, no exactly-once promise (C3/C9), and metadata-only default
  with `ContentOmitted` + `unisphere.content.omitted=true` (C7).
- Deliberately named limits — non-recursive listing, no in-place-rewrite detection, `Unavailable`
  identity, partial bytes on write failure — are documented as limits rather than dressed up as
  guarantees.

The findings below are all about places where a contract is *silent* at a seam two owners share, not
about style, formatting, or additional process gates.

---

## F1 — high — CLI has no reachable path to the collection service (open)

**Location:** guide C10 (SDK `collect_batch` free function), C11 ("CLIfrontend uses injected collection
functions/port"), C1/C4 (core type list), `tk-0001` frozen paths.

**Evidence:**
- `crates/testkit/src/bin/unisphere-arch-check.rs:19-21` — `("unisphere-cli", "normal")` permits only
  `unisphere-core`, `clap`, `serde_json`. `unisphere-cli` may **not** depend on `unisphere-sdk`.
- `crates/cli/src/lib.rs:35-41` — the existing seam is a *core-owned port trait*: `run(..., inspector:
  &dyn InspectionApi, ...)`, with `InspectionApi` declared in `crates/core/src/ports.rs:12-14` and faked
  in `crates/testkit/src/fakes.rs:85`.
- Guide C10 declares only a free `unisphere_sdk::collect_batch(...)` and places `CollectionBatch` in the
  SDK service description; `MappedBatch` is core (C4) but `CollectionBatch` is never assigned a home.

**Why it is material:** in wave 2 `tk-0005` will find no legal way to wire `sessions export`. The three
available moves are all bad: (a) edit frozen core, contradicting `tk-0001`'s "frozen sourceinterfaces";
(b) widen `allowed()` to permit `cli -> sdk`, inverting the dependency direction the repo currently
enforces and weakening `ac-000b`'s own dependency check; or (c) import the concrete adapter/loader crates
into the CLI, which C11 explicitly forbids. The CLI also cannot *name* `CollectionBatch` in a signature
if that type lives in the SDK.

**Smallest fix:** add to C1 (so it is inside `tk-0001`'s baseline) a core-owned port plus the batch type,
mirroring the existing `InspectionApi` shape exactly:

```rust
pub struct CollectionBatch { pub mapped: MappedBatch, pub next_cursor: ReadCursor,
                             pub more: bool, pub incomplete_tail: bool }

pub trait CollectionApi: Send + Sync {
    fn collect_batch(&self, session: &SessionRef, cursor: Option<&ReadCursor>,
                     limits: ReadLimits, options: MappingOptions,
                     destination: &mut dyn std::io::Write) -> Result<CollectionBatch, PipelineError>;
}
```

The SDK's `collect_batch` becomes the implementation behind it (keep the free function too if the
external consumer wants it), `crates/testkit/src/collection.rs` gains a `FakeCollector` for CLI tests,
and `crates/cli` keeps depending on core only. No arch-check weakening, and `ac-000a`'s "one registration
entry" story gets a real seam to register into.

## F2 — high — an oversize physical record has no defined behaviour (open)

**Location:** guide C1 (`ReadLimits.max_record_bytes` default `1048576`), C3 (loader semantics), C5
(`PipelineErrorKind::Limit`), `ac-0003`.

**Why it is material:** `LoadedBatch` has no diagnostics channel — only `MappedBatch` does — so the loader
can express "this record exceeds `max_record_bytes`" only by erroring or by silently skipping. Skipping
violates `ac-0003` ("no silent data loss"); erroring, as written, is permanent: the cursor can never
advance past that record, so everything after it in the session becomes unreachable and `sessions export`
dies mid-file with a fixed `UNI-LIMIT` string that names no remedy. This is not hypothetical at the 1 MiB
default — pasted files, large `tool_result` payloads and image blocks in Claude Code JSONL routinely
exceed it. Two lanes (`tk-0002` producing, `tk-0005` looping in the CLI) must agree, and neither is told
what to do.

**Smallest fix:** one sentence in C3 — an oversize record returns `PipelineError` kind `Limit` carrying
the record's **start** offset; `next_cursor` is not advanced; the same cursor retried with a larger
`max_record_bytes` must succeed. Add the retry-succeeds case to `vd-0002`, and require the CLI to print
the raise-the-limit remedy on stderr before exit 1.

## F3 — high — `SourceIdentity::Unavailable` silently makes the product Unix-only (open)

**Location:** guide C1 (`SourceIdentity = Unix { device, inode } | Unavailable`), C3 ("unsupported resume
identity is a typed error, not automatic reset"), C12 (`unisphere-loader-jsonl -> core + targetunix
libc`), plan non-goals, `ac-0009`, `ac-000c`.

**Why it is material:** on any non-Unix target the loader can only ever produce `Unavailable`. Read the
two contract clauses together and the *second* `read_batch` call — the one that passes back the cursor the
first call returned — is a typed error. That means every session larger than one batch is unexportable on
Windows, while `ac-0009`/`ac-000c` claim installed-CLI export proof and the plan's non-goals never say the
slice is Unix-only. The repo's own harness doc already warns that "configured Linux/macOS CI is not itself
observed platform evidence", so this will not be caught by CI either.

**Smallest fix:** pick one and write it into C3 *and* the plan non-goals.
(a) Declare the file loader Unix-only for this slice; the CLI fails fast with `Unsupported` naming the
platform, and `sessions export` never half-works. (b) Permit `Unavailable -> Unavailable` resume on
path + boundary offset alone, documented as a strictly weaker guarantee with no rotation/truncation
detection. Either is defensible; silence is not. Prove the chosen one in `vd-0002`.

## F4 — medium — the purity proof is four words with no deny-list (open)

**Location:** `vd-0006` ("Approvedkind-awaredependencies **and pureadapter/core source-surface
constraints**"), `bp-0001` ("no I/O available to mapper by dependency/source check"), `ac-0001`.

**Evidence:** the existing `unisphere-arch-check` (`crates/testkit/src/bin/unisphere-arch-check.rs:33-109`)
is purely a `cargo metadata` graph checker. It inspects package names, dependency kinds, `target`,
`optional`, `rename` and path-ness. It contains no source inspection whatsoever.

**Why it is material:** `std::fs`, `std::env`, `std::process`, `std::net` and `SystemTime::now` are not
dependency edges — they arrive free with `std`. A graph check therefore **cannot** prove the single
property the user actually asked for. The new source-surface half is specified in four words, so
whichever shape `tk-0005` invents will be unfalsifiable: a scan with no stated token list and no negative
fixture passes silently whether or not it works.

**Smallest fix:** enumerate the denied source tokens in the guide (`std::fs`, `std::env`, `std::process`,
`std::net`, `std::time::SystemTime`, `include_str!`/`include_bytes!`, `unsafe`; `std::io` permitted only in
the writer crate), require a negative fixture under the already-planned
`crates/testkit/fixtures/architecture/` that the check **must** reject, and state the known false-negative
bound (import aliasing, macro expansion, indirect calls) in `docs/adapters.md` rather than implying the
scan is airtight. Keep `#![forbid(unsafe_code)]` per crate as it is today.

## F5 — medium — the profile attribute registry is undefined at a frozen seam (open)

**Location:** guide C6 (`unisphere.source.kind` required on every record, value domain never given), C8
(`unisphere.usage.*` component counters, names never given), C13 (conformance helper asserts
"source-provenance"), `tk-0001` owning `crates/testkit/src/collection.rs`.

**Why it is material:** `tk-0001` writes the shared conformance helper in **wave 0** and freezes it; that
helper must assert attribute keys that `tk-0003` will not write until wave 1. With the key set undefined,
the mismatch surfaces only at wave-2 integration — exactly the coupling the fan-out was designed to avoid.
Separately, one attribute namespace currently has two documentation owners: `docs/telemetry-profile.md`
(`tk-0004`) and `docs/claude-adapter.md` (`tk-0003`).

**Smallest fix:** put the closed attribute list into C6/C8 — key, value type, emitting lane,
required/optional — including the value domain of `unisphere.source.kind` (in particular what an unknown
native record kind emits, given C7 promises a provenance-only record) and the exact `unisphere.usage.*`
component names carried from the native record. Name `docs/telemetry-profile.md` as the single normative
registry and have `docs/claude-adapter.md` reference it.

## F6 — medium — the OTLP proto revision is unpinned (open)

**Location:** guide C9, `ac-0006` ("valid ... OTLP LogsData JSONL"), `docs/telemetry-profile.md`.

**Why it is material:** C9 emits `eventName` on `logRecords`. `LogRecord.event_name` is a comparatively
recent addition to `opentelemetry-proto`; consumers built against an earlier revision drop the field
entirely, which silently discards the one attribute carrying the record's identity. C9 also omits
`resource` from `resourceLogs[]`, and omits `severityNumber`/`severityText`/`observedTimeUnixNano` — each
legal, each changing how a real collector attributes the data — and a record whose native timestamp is
absent emits no `timeUnixNano` at all.

**Smallest fix:** pin the exact `opentelemetry-proto` version the profile targets in C9 and in
`docs/telemetry-profile.md`; state that `resource`, `severity*` and `observedTimeUnixNano` are
intentionally absent and what a consumer should therefore expect; add one `vd-0004` case asserting the
exact top-level key set so a future edit cannot quietly add a nonstandard key.

## F7 — medium — `Limit` is overloaded three ways behind one fixed message (open)

**Location:** guide C5 (`PipelineError::new(kind, offset)`, "fixed code/message/fix", `UNI-LIMIT`), C1
(`SourceScope.max_sessions`), C2 ("reject oversize result rather than truncate silently").

**Why it is material:** one code and one fixed message must serve record-too-large (F2), batch-budget
exhausted, and listing-oversize — and for listing, `offset` is meaningless, so the error carries no
discriminating information at all. The caller cannot tell which of three different limits to raise, which
contradicts the "actionable error context" `ac-0007` asks for. `PipelineError` is frozen at baseline, so
this cannot be fixed later without unfreezing core.

**Smallest fix:** carry a closed reason discriminator so `code()` yields `UNI-LIMIT-RECORD`,
`UNI-LIMIT-BATCH` or `UNI-LIMIT-LISTING`, each with its own fixed `fix()` string. No new kind, no free-form
text, no source bytes.

## F8 — low — `ReadLimits`/`SourceScope` invariants have no named enforcement point (open)

**Location:** guide C1 — "positive and `max_batch_bytes>=max_record_bytes`".

The contract states the invariant but not whether the type has public fields or a checked constructor, and
`SourceScope.max_sessions == 0` is undefined (reject, or list nothing?). As written, `tk-0002` and
`tk-0005` will each invent their own validation and disagree about the error kind. **Fix:** specify
`ReadLimits::new(...) -> Result<Self, PipelineError>` with private fields plus accessors — or state that
fields are public and the loader returns `UNI-INPUT` — and define `max_sessions == 0`. Baseline decision.

## F9 — low — non-UTF-8 session paths have no owner (open)

**Location:** guide C6 (`unisphere.source.path` is the "explicit supplied UTF8path"), C1 (`SessionRef {
path: PathBuf }`).

A `PathBuf` on Unix need not be UTF-8, and nothing says who rejects the mismatch. If the pure adapter
does, one badly-named file fails an entire export from inside the mapper. **Fix:** one sentence in C2 —
the loader rejects non-UTF-8 session paths at `list_sessions`/`read_batch` with `UNI-INPUT`, so the adapter
may assume UTF-8.

## F10 — low — "bounded batch buffer" is bounded by nothing (open)

**Location:** guide C9 — "Complete serialization into a bounded batch buffer before writing";
`ac-000c` ("bounded batches").

`write_batch(&self, records, destination)` takes no limit. The buffer is bounded only transitively by the
caller's `ReadLimits`, and OTLP AnyValue wrapping plus JSON escaping expands the input several-fold, so a
4 MiB native batch is a materially larger allocation. The wording implies a guarantee the signature cannot
give. **Fix:** reword C9 to say the buffer is proportional to the supplied records and bounded only
transitively by `ReadLimits`, or give the writer an explicit maximum. Wording, but on a resource claim.

## F11 — low — non-recursive listing returns an empty success on the natural input (open)

**Location:** guide C2 (deliberately non-recursive), C11 (`sessions list --root ABS_PATH`).

Claude Code stores sessions as `<projects>/<project-slug>/<uuid>.jsonl`. A user pointing `--root` at the
parent gets `{"sessions":[]}` and exit 0 — indistinguishable from "this tool found nothing here". The
non-recursive choice itself is sound for a first slice; the silent empty success is the problem.
**Fix:** include an explicit `"recursive": false` field in the versioned list response (it is already
versioned JSON) and name the leaf-directory requirement in `--help` and `docs/session-loader.md`.

## F12 — low — `ac-0001`'s proof list omits the check that actually proves it (open)

**Location:** guide `capabilities/ac-0001` — proof `vd-0003`, `vd-0005`.

Both are test runs. Unit tests can show the adapter *behaves* deterministically; they cannot show it has
no filesystem dependency. The check that does is `vd-0006` — and `bp-0001` says so itself ("no I/O
available to mapper by dependency/source check"). **Fix:** add `vd-0006` to `ac-0001`'s proof list. One
array entry; without it the traceability claim for the user's headline requirement is wrong.

## F13 — low — baseline root-manifest ordering (open, unverified by me)

**Location:** guide C12 ("Newtime/libc direct deps declared once in root atbaseline"), `tk-0001` owning
root `Cargo.toml`, `Cargo.toml:2` (`members = ["crates/*"]`).

`time` and `libc` are external and land cleanly at baseline. But if `[workspace.dependencies]` also gains
path entries for `unisphere-loader-jsonl`/`-adapter-claude`/`-output-otlp` at baseline, those directories
do not exist until wave 1. I did not run a build — validation was out of scope — so I flag this as a
cheap thing to confirm while executing `vd-0001` rather than as an established fact. **Fix:** declare
external deps only at baseline and add the internal path entries in wave 2 with the arch-check allow-list
update, which `tk-0005` already owns.

---

## Answers to the packet's focus questions

- **Three-lane independence:** genuine, with one exception. F1 is a hidden dependency of the *composition*
  on a type that must exist in the frozen baseline. F5 is a hidden dependency of the wave-0 conformance
  helper on wave-1 attribute names. Both are fixed by adding to `tk-0001`'s contract, not by reordering.
- **Contract completeness / resource limits:** cursor, blank-line, partial-tail, `more`/`incomplete_tail`
  and source-change semantics are precise and internally consistent — I found no contradiction among C1,
  C3 and C10. The gaps are the oversize record (F2), non-Unix resume (F3) and limit-error granularity (F7).
- **Minimal honest Claude mapping:** correct as specified. Physical fragments, repeated `message.id`,
  usage-as-snapshot and the refusal to emit `gen_ai.usage` totals are all right, and I recommend no
  loosening. The only gap is the undefined `unisphere.source.kind` / `unisphere.usage.*` key set (F5).
- **OTLP encoding:** AnyValue recursion, `intValue`/`timeUnixNano` decimal strings, i64 rejection and
  null → `{}` are correct against the JSON Protobuf encoding rules. Unpinned proto revision and the
  undocumented absent fields are the gap (F6).
- **Failure/checkpoint semantics:** `ac-0007` and C9/C10 agree — checkpoint only after write success,
  partial bytes possible, no rollback, no exactly-once. Honest and consistent.
- **SDK/CLI consumer and extension affordance:** blocked on F1. Once a core port exists, `ac-000a`'s
  "one registration entry" claim is credible: the pattern already works for `InspectionApi` +
  `FakeInspector` + app-level composition.
- **Existing configuration API:** preserved. Nothing in the guide alters `ConfigReader`/`InspectionApi`/
  `Failure`, and C5 explicitly declines to touch the configuration failure taxonomy. Note that adding a
  `sessions` boundary to `unisphere_cli::run` changes that function's signature; `ac-000b` says
  *behavior* stays compatible, which remains satisfiable, and the only in-repo callers are
  `crates/app/src/main.rs:31` and `crates/cli/tests/frontend.rs`.
- **Twelve ACs, proof and ownership:** every AC has a named owner and at least one check. Ownership is
  clean. One traceability defect (F12). Task assertions are correctly deferred: `assets/tasks/phase-1/
  tasks.dd.json` is an empty scaffold, which is the right state before design acceptance, and the twelve
  task-accounting orphans the PM flagged are the expected consequence.

## Honest gaps in this review

- No build, test, lint or format was run — the packet forbade it. Everything above is a reading of
  committed bytes; F13 in particular is explicitly unverified.
- I did not read any private Claude store, so my statement that real records exceed the 1 MiB default
  (F2) rests on the format's known content shapes, not on measurement in this session.
- I did not verify the exact `opentelemetry-proto` release that introduced `LogRecord.event_name` against
  the upstream repository in this session; F6 asks the PM to pin the version precisely for that reason.
- `assets/design-validation.json` records only structural `builder guide --check` receipts and a boot
  capture. It contains no semantic approval, and I treated it as none.

## Recommendation

Land F1, F2 and F3 into the guide contracts before `tk-0001` seals `crates/core/src/collection.rs` — all
three are baseline types or baseline semantics, and all three are cheap now and expensive after the fan-out.
F4–F7 are worth folding into the same edit since they touch the same frozen surfaces. F8–F13 can ride
along or be accepted explicitly. No new proof run and no additional review cycle is needed before guide
exit; a re-read of the amended contracts is sufficient.
