# Plan 005 — corrected-contract reread (design review r2)

**Verdict: approved.** All thirteen r1 findings are disposed: twelve fixed in the amended contracts,
one (F13) accepted on a rationale I verified independently rather than took on assertion. Zero open
material issues. No further design cycle is needed before guide exit.

| Field | Value |
| --- | --- |
| subject_sha | `006600d85a6ad5eb38c832dc950c022d5e58aa2d` |
| prior subject | `21a9f98787ad8f3de4b00e0f8b715c8600db643e` (r1 report retained, unmodified) |
| workspace | `/Users/jordanknight/substrate/unisphere/unisphere-claude-session-pipeline` |
| plan sha256 | `0c28b557a2def3dc075c70403e6768959057c007c428dcb9a4ac23683ac7f830` |
| guide sha256 | `5520d59344dc2f664af3121df3184c344f659a4c1acceca9432ce06abe242777` |
| reviewer | `pij-huge-nigel`, omp, `github-copilot/claude-opus-5`, effort high |
| scope | decomposition — focused reread of changed design only |
| method | read-only; committed bytes at HEAD == subject_sha; no builds, tests, lint or formatters run |

Both packet digests were recomputed here and match the live files. Working tree clean for every
reviewed path (only untracked `.dd/schemas/builder/work-packet/`). The delta from r1 is docs-only:
15 files, +2467/−54, with `source_changes: false` in the dispositions record and no crate touched.

I re-read only what changed — guide `meta`/`architecture`/`capabilities`/`units`/`checks` and plan
`non_goals`/`acceptance_criteria` — plus the unchanged contracts each amendment now depends on (C0,
C4, C7), because an amendment can only be judged against the surface it lands on. Everything r1
already established as sound is preserved and not relitigated.

---

## Disposition of F1–F13

### F1 — CLI has no reachable path to the collection service → **fixed**

C1 now makes both the port and the batch type core-owned:

```
CollectionBatch { mapped, next_cursor, more, incomplete_tail }
CollectionApi: Send+Sync
  list_sessions(&self, &SourceScope) -> Result<Vec<SessionRef>, PipelineError>
  collect_batch(&self, &SessionRef, Option<&ReadCursor>, ReadLimits, MappingOptions,
                &mut dyn std::io::Write) -> Result<CollectionBatch, PipelineError>
```

Both methods are object-safe as written — `&self` receivers, no generic parameters, no `Self` return
— so `&dyn CollectionApi` is well-formed, and the guide says so explicitly rather than leaving it to
be discovered. C10 has the SDK implement it (`Collector<L,A,W>` with `new(loader,adapter,writer)`),
C11 adds `run_sessions(args, context, collector: &dyn CollectionApi, stdout, stderr) -> u8`, and the
existing `run` keeps its signature and behaviour as a distinct operation rather than a compatibility
alias — which is the cleaner of the two options and removes the incidental signature churn r1 noted.

The fix stays legal against the check that motivated the finding:
`crates/testkit/src/bin/unisphere-arch-check.rs:19-21` permits `unisphere-cli -> core | clap |
serde_json`, and the CLI now imports only core. `FakeCollector` lands in `crates/testkit/src/
collection.rs`, which `tk-0001` already owns, and CLI→testkit is already an allowed dev edge. No
allow-list weakening, no core edit at composition time, no dependency inversion. C1's baseline and
`tk-0001`'s interface line both name `CollectionApi`/`CollectionBatch`/`MAX_OUTPUT_BATCH_BYTES` and
`FakeCollector` as part of the frozen module, so wave 2 inherits a complete surface.

### F2 — oversize record undefined → **fixed**

C3: `RecordLimit` at the record **start** offset, no batch and no next cursor returned, caller
retains its previous cursor, and retrying the **same** cursor with a larger `max_record_bytes` and
compatible `max_batch_bytes` recovers all data. C11 has the CLI remedy name `--max-record-bytes` and
`--max-batch-bytes`; the record is never skipped. `vd-0002` gains the retry-succeeds case, and
`ac-0003` now states the guarantee.

The recovery path is genuinely closed: the error surfaces in the loader before any mapping or
writing, so nothing was emitted, the cursor did not move, and the retry re-reads the same window
without duplicating output. C3 also settles the adjacent case — batch capacity with prior complete
records returns that prior batch with `more=true` and does not consume the next record — so the two
limit outcomes cannot be confused.

### F3 — non-Unix silently unsupported → **fixed**

C2: the file loader is explicitly Unix-only for this release, and **both** `list_sessions` and
`read_batch` fail fast with `Unsupported` before any I/O — the phrase "no misleading first-batch-only
success" closes exactly the failure mode r1 described. C5 has `Unsupported` explain that boundary
with no raw OS text. The plan gains a non-goal naming macOS/Linux and stating that broader platform
support is separate scope, "not implicit partial support", and `ac-0003` carries it. Purity of the
mapper and writer is preserved: C2 states they do not inherit the restriction, so the portable half
stays portable. This is option (a) from r1, cleanly taken.

### F4 — purity proof was graph-only → **fixed**

C13 now specifies two layers. The metadata check is extended with a source scan over non-test
production core and adapter-claude sources denying `std::{fs,env,process,net,thread}`,
`SystemTime::now`/`Instant::now`, `unsafe`/extern FFI, and `include_str!`/`include_bytes!`. Negative
synthetic fixtures under `crates/testkit/fixtures/architecture/` must exercise **fully-qualified and
grouped imports**, a clock call and include macros, and must fail — so the sensor is falsifiable
rather than vacuously green. The false-negative bound (aliases, macro expansion, indirect calls) is
documented, independent source review is named as the complementary judgement, and the guide states
outright that no formal no-I/O proof is claimed from a regex.

Two details show the amendment was made with the actual code in view rather than in the abstract:
core is explicitly permitted to *name* `std::io::Write` as a port type while forbidden from invoking
filesystem/env/network/process/clock APIs — without which the F1 fix would have tripped the F4 check
— and the scan must distinguish `#[cfg(test)]` modules or enumerate production files, since tests
legitimately use fixtures and threads. `#![forbid(unsafe_code)]` remains.

### F5 — undefined attribute registry at a frozen seam → **fixed**

C6 closes the registry: every required key with its type — `unisphere.profile.version` integer 1,
`unisphere.source.adapter` from `SessionAdapter::name`, `unisphere.source.path` string,
`unisphere.source.offset` non-negative integer, `unisphere.source.kind` raw native type string when
present and the literal `unknown` otherwise — plus the optional id/model/conversation strings and the
`is_sidechain`/`content.omitted` booleans. Missing or non-string native type yields `unknown` with an
`InvalidField` diagnostic; unknown kinds keep their native type string with `UnsupportedRecord` and
no raw payload. C8 closes the usage side to exactly four component keys copied only from the
identically-spelled native `message.usage` component, each optional integer `0..=i64::MAX`, plus
`unisphere.usage.scope = native_record_snapshot` when any is retained — with "no other usage keys are
emitted in v1" stated explicitly.

Every diagnostic code these amendments invoke (`UnsupportedRecord`, `InvalidField`, `ContentOmitted`)
already exists in C4's closed `MappingDiagnosticCode` enum, and `SessionAdapter::name()` already
exists in C4, so C6/C8 introduce no undeclared surface. `docs/telemetry-profile.md` is named sole
normative owner and belongs to `tk-0004`; `docs/claude-adapter.md` (`tk-0003`) references it. One
owner, no divergent namespace. `tk-0001` can now write the wave-0 conformance helper against a key
set that is frozen in the guide rather than guessed from a wave-1 sibling.

### F6 — unpinned OTLP revision → **fixed, and independently verified**

C9 pins `opentelemetry-proto@bb8796bff67cf6e1c7f218e21de6eaec0841871e`,
`opentelemetry/proto/logs/v1/logs.proto`. I fetched that exact blob and read it: line 227 is
`string event_name = 12;`, annotated `[Since v1.5.0]`, with the comment "A unique identifier of event
category/type" — so the field number, the version claim and C6's "event_name is category, not
instance identity" reading are all correct against the pinned bytes, not merely asserted.

The message-part reference `semantic-conventions-genai@94f432d7126f5884d30a2cdde6f4e89908ebb6fd`,
`model/gen-ai/gen-ai-input-messages.json`, also resolves at that exact SHA, and its vocabulary
carries `role`, `parts`, `text`, `reasoning`, `tool_call` (with `arguments`), `tool_call_response`
(with `response`) and `id` — matching C7's part mapping term for term.

C9 additionally documents what is deliberately absent — `resource` and `schemaUrl` (unknown
attribution), `severityNumber`/`severityText`/`observedTimeUnixNano`/`flags`/`traceId`/`spanId` (no
observed values or correlation) — states that an absent native timestamp omits `timeUnixNano`, and
requires tests to assert the legal key set without demanding unsupported optional fields. It also
says plainly that this is source-derived `LogsData` conversion, not an instrumented GenAI inference
span. That is the honest framing.

### F7 — one overloaded `Limit` code → **fixed**

C5 splits the kind into `RecordLimit | BatchLimit | ListingLimit | OutputLimit` with codes
`UNI-LIMIT-RECORD`/`-BATCH`/`-LISTING`/`-OUTPUT`, each with its own fixed remediation (raise record
plus compatible batch; reduce batch size or raise the batch budget; raise `max_sessions` or narrow
scope; reduce output batch or content size). Offset semantics are now defined per kind — record start
for `RecordLimit`, optional for batch/output, `None` for listing — which was the specific
information-loss r1 flagged. Errors never return a committed cursor. Still no free-form text and no
source bytes in diagnostics.

### F8 — unenforced `ReadLimits` invariant → **fixed**

C3 gives `ReadLimits` public fields plus `validate() -> Result<(), PipelineError>`, called by both
`FileSessionLoader` and SDK `collect_batch` **before any I/O or port invocation**; non-positive values
or `max_batch_bytes < max_record_bytes` are `InvalidInput`. C2 defines `SourceScope.max_sessions == 0`
as `InvalidInput`. Validating in both places is deliberate defence in depth against SDK callers that
bypass the loader, and it is deterministic because both call the same function. C3 also nails byte
accounting — physical LF/CR bytes counted, checked arithmetic, returned slices omit LF only.

### F9 — non-UTF-8 paths had no owner → **fixed**

C2 makes `FileSessionLoader` reject non-UTF-8 root and session paths at the listing and read
boundaries with `InvalidInput` *before* opening or returning a bad path, and has pure adapters
defensively reject a caller-supplied non-UTF-8 `SessionRef` as `InvalidInput` rather than coerce
lossily — justified because SDK callers can bypass the loader. The defensive half has a channel:
C4's `map` returns `Result<MappedBatch, PipelineError>`, so this does not require an infallible
signature to grow one.

### F10 — "bounded" output buffer bounded by nothing → **fixed**

C9 exports `MAX_OUTPUT_BATCH_BYTES = 33_554_432` (32 MiB) from core and requires serialization
through a capped `std::io::Write` buffer that refuses growth past the cap, with the explicit
instruction "do not serialize unbounded then measure". A breach returns `OutputLimit` **before**
touching the destination, so no partial bytes and no cursor advance — which composes correctly with
C10's checkpoint-only-on-success rule. The pre-existing honest limit (write failure may leave partial
bytes, no rollback) is retained. `vd-0004` gains the cap case.

### F11 — silent empty listing → **fixed**

C2 and C11 put `recursive:false` alongside the explicit root in the versioned list response, require
`--help` and the docs to state the leaf project directory requirement, and require an explanatory
stderr diagnostic on an empty or non-`.jsonl` leaf directory — with "never claims recursive search"
stated outright. `vd-0002` covers the empty-leaf case. The non-recursive choice itself is unchanged,
which is right for a first slice.

### F12 — `ac-0001` did not cite its own proof → **fixed**

`ac-0001` proof is now `vd-0003`, `vd-0005`, `vd-0006`. The traceability for the user's headline
requirement now points at the check that actually tests it.

### F13 — inert root path declarations → **accepted, rationale verified**

The PM accepted r1's flag rather than changing the design, on the rationale that uninherited
`[workspace.dependencies]` path entries for not-yet-existing packages are an already-exercised
pattern from Plan 001. I verified that claim directly instead of taking it:

- At `18ee516` (the Plan 001 baseline commit) root `Cargo.toml` declared
  `unisphere-sdk = { path = "crates/sdk" }` and `unisphere-cli = { path = "crates/cli" }`, while
  `git ls-tree 18ee516 crates/` shows only `crates/core` and `crates/testkit` present.
- That same commit contains a `Cargo.lock` naming exactly `unisphere-core` and `unisphere-testkit`.
  Cargo therefore resolved that workspace successfully with two path declarations pointing at absent
  directories.
- `members = ["crates/*"]` is a glob, so absent directories never appear as members — matching C12's
  "no missing directories appear as explicit workspace members".

So the pattern is proven by repository evidence, not assertion, and my r1 concern is answered.
C12 also binds the acceptance to a real obligation — the actual `vd-0001` baseline run must prove
independence with all three future package directories absent before seal, and if it fails the fix is
to correct the root declarations, explicitly **not** to add stubs and **not** to mutate a frozen root
manifest in wave 2. That is the right escalation: a fail-loud, pre-seal check with a named remedy,
rather than a silent assumption. I record this as accepted with a live obligation, and I did not run
`vd-0001` because the packet excludes execution.

---

## Verification performed for this reread

- Recomputed plan and guide sha256; both equal the packet values.
- `git rev-parse HEAD` == `006600d85a6ad5eb38c832dc950c022d5e58aa2d`, clean tree for reviewed paths.
- Structural diff of guide and plan sections against `21a9f987` to bound the reread to real changes:
  guide `meta`/`architecture`/`capabilities`/`units`/`checks` changed; `fan_out`, `baseline`,
  `isolation`, `roles`, `composition`, `review`, `risks` unchanged. Contracts C1, C2, C3, C5, C6, C8,
  C9, C10, C11, C12, C13 changed; C0, C4, C7 unchanged and re-read as landing surfaces.
- Cross-checked each amendment against the frozen surface it depends on: `MappingDiagnosticCode`
  membership, `SessionAdapter::name()`, `map` returning `Result`, `CollectionApi` object safety,
  arch-check package allow-list, unit path ownership for the single-owner registry claim.
- Fetched and read both pinned upstream schema files at their exact SHAs (see F6). This is the only
  network read; it was necessary because an unverifiable pin is not a pin.
- Read `assets/design-review-dispositions.json` and `assets/design-v2-validation.json`: four checks,
  all exit 0 — `builder guide --check`, `plan validate` (0 error, 0 warn, 0 contradictions, 12
  orphans as expected pre-tasks), `ddocs validate` (0/0), `ddocs build --check` (`drift: false`).

## Implementation notes for `tk-0005` (guidance, not findings)

Neither of these is a contract gap; both sit inside one owner and one wave, and neither touches a
frozen type. Recording them so they are decided rather than discovered:

- `run_sessions` receives an already-constructed `&dyn CollectionApi`, so `--adapter` selection
  necessarily happens in the app's static registry before the collector is built. C11's existing rule
  makes an unknown `--adapter` value an invalid argument, so exit 2 is the consistent reading. Worth
  deciding once, in one place, rather than twice.
- The rule that a pure adapter defensively rejects a non-UTF-8 `SessionRef` lives in C2, the loader
  contract, but binds `tk-0003`. Repeating it in the adapter documentation would prevent it from being
  missed by a reader who works from C0/C4/C6/C7/C8.

## What this review did not do

No build, test, lint or formatter was run — the packet excludes them, so nothing here is an execution
claim. `vd-0001` through `vd-0006` remain unexecuted, and the dispositions record says so itself
(`proof_not_claimed`). No source, plan, guide, task, flow or canonical receipt was written; the r1
report is untouched. This is design approval only: the contracts are now specific enough that the
three lanes can proceed independently, which is exactly the question this scope asks.
