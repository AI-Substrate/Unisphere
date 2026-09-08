# Telemetry fidelity: implemented boundaries and remaining gaps

**This release is a source-derived Claude JSONL projection, not a lossless session
archive or a complete telemetry history.** A successful read/export means the
selected bytes were handled under the chosen policy; EOF is only an observed read
boundary, never evidence that the producer has finished emitting or revising data.

Full fidelity means preserving the telemetry the source actually makes available,
including native facts not yet understood by our mapping, and identifying what was
missing, deliberately excluded, unsupported or not verified. It cannot reconstruct
content the source never stored or has already irreversibly redacted.

Jordan requested this assessment at delivery, not an expansion of this release
into raw archival, revision tracking or additional native adapters. The existing
explicit-content policy is unchanged.

| Dimension | Full-fidelity meaning | Implemented behavior | Actual proof | Remaining gap and classification |
| --- | --- | --- | --- | --- |
| Native bytes and unknown fields | Retain original records and unfamiliar fields so future interpretation does not depend on today's mapper. | Loader provides bounded native bytes excluding LF and omits ASCII-whitespace-only lines; mapper parses JSON and exports selected fields. Unknown records retain provenance, not their raw payload. | Loader tests preserve fixture bytes/offsets and reject malformed/bounded inputs; adapter tests cover unknown/non-object records and safe malformed-data failures. | No native-byte archive, JSON lexical/key-order/duplicate-key preservation, complete unknown-field retention or raw reconstruction. **Unsupported behavior**; LF/blank filtering is an **intentional framing policy**. |
| Semantic events, IDs and usage | Preserve physical identity, logical message identity and relationships without conflating fragments with model operations; usage must retain its real scope. | Physical records and repeated logical IDs remain distinct, native parent/session/model facts are retained when valid, and four usage components remain independent native-record snapshots. No inferred operation totals, provider or trace/span IDs. | Fifteen adapter tests include repeated IDs, split-batch/replay equivalence, sidechain semantics, missing values and usage bounds; real SDK/CLI output parity uses the same fixtures. | No full message/session reconstruction, branch/rewind interpretation, new usage fields, arbitrary invalid-value retention or consumer deduplication/aggregation. **Unsupported behavior**; absent source fields are a **source limitation**, never zero. |
| Content inclusion | Retain prompts, responses, reasoning and tool I/O under explicit policy, including opaque native blocks where permitted. | Metadata-only is the default; opt-in maps supported text/reasoning/tool arguments/results as structured values. Paths, IDs, model and kind remain metadata, so metadata-only is not anonymity. | Adapter content-marker tests, writer structured-value tests and actual metadata/opt-in SDK/CLI proof. | Default omission is **intentional policy**. Thinking signatures, unknown/redacted block payloads and malformed parts are not retained even with opt-in: **unsupported behavior**. Source-redacted original content is a **source limitation**. |
| Referenced artifacts | Capture referenced sidecars/attachments/spills, or name each as unavailable, denied or still pending. | No attachment/spill path is opened implicitly; tool-result values may contain references, but outer sidecar metadata is not interpreted. | Loader symlink/FIFO and explicit-source tests; adapter fixture includes a do-not-open spill path; source/port separation prevents mapper I/O. | Referenced bytes and a reference-resolution/availability ledger are absent. **Unsupported behavior**, with deliberate safe non-dereference policy. Missing/unavailable source artifacts would be a **source limitation** only after explicit resolution evidence. |
| Delayed arrivals, updates and deletes | Reconcile late appends, backfilled inserts, in-place revisions and disappearance without silently losing history or double-counting current state. | Caller-owned file cursor binds path/device/inode/offset, supports appended complete lines, defers partial tails and reports observed replacement or truncation below the checkpoint. Repeated source records are not deduplicated. | Real loader append/partial-tail/source-change/budget tests and actual partial-tail CLI proof. | No same-inode rewrite/regrowth or truncation-above-checkpoint detection, logical revision model, delete inventory, delayed SQLite/snapshot reconciliation, durable scheduler or idempotent sink transaction. **Unsupported behavior**. Cursor/other IDE delay is expected, not proof of absence; only Claude file loading is implemented. |
| Timing, provenance and completeness | Distinguish native occurrence time, collection/arrival time, revision ordering and source completeness. | Valid representable RFC3339 native timestamps retain nanoseconds/offset meaning; source path/byte offset/native IDs identify origin. `more` and `incomplete_tail` describe the current read, not the lifetime of the source. | Adapter timestamp tests and loader EOF/no-spin tests; OTLP example emits decimal-string timestamps and integers; CLI reports partial-tail state. | Observation/arrival timestamps, revision sequence and explicit provisional/final/completeness-unknown fields are not emitted. Invalid timestamps are diagnosed and omitted. **Unsupported behavior**, not implicit finality. Broader live-client/dialect coverage remains **unverified coverage**. |

## Evidence boundaries

Plan005 evidence is kept with its plan under `docs/plans/005-claude-session-pipeline`
(or the same folder under `docs/plans/archive/` after closeout):

- `assets/tk-0002-validation-corrected.json`: 18 loader tests passed on this Unix
  host; the non-UTF-8 **directory candidate** branch explicitly reports NOT
  EXERCISED because this filesystem rejects such names with `EILSEQ`; direct
  non-UTF-8 root/input rejection was exercised before I/O.
- `assets/tk-0003-validation-initial.json`: 15 in-memory Claude mapping/conformance
  tests passed, using synthetic fixtures rather than private session stores.
- `assets/tk-0004-validation-corrected.json` and `tk-0004-example.json`: nine writer
  tests plus actual OTLP output, covering 64-bit values, structured content,
  exact encoded byte cap, partial/short writes and flush failure.
- `assets/collection-smoke-initial.json`: actual external SDK and temporary
  installed CLI parity, explicit metadata/content policy, partial tail, output
  collision protection and error-stream isolation.
- `assets/composed-tests-initial.json` and `composed-quality-initial.json`: actual
  assembled tests, formatting, clippy, architecture/purity sensor and wrapper
  regressions; final committed-candidate receipts supersede these where named.

A lexical source-purity sensor has documented alias/macro/indirect-call limits;
it is not a formal effect system. Unit tests over sanitized/synthetic data do not
prove every private or future Claude dialect. No network-denial, complete-session,
all-client or Linux/Windows live-runtime claim is inferred from these results.

## Current CLI experience versus resumed ingestion

Current selection is `--adapter claude-code --input <path>`, not `--harness` plus
session ID. Each invocation starts at byte zero; `--output` creates a new file and
does not reopen an existing destination as a resume operation. The SDK returns a
caller-owned cursor, but the CLI persists no checkpoint.

The summary reports mapped record count, batches, aggregate diagnostic count,
incomplete-tail state and the final byte offset. It does not report starting
cursor, native physical-line count, per-code diagnostic breakdown or a source
record-ordinal from/to range. SDK callers retain the individual diagnostic codes
and offsets. These are **unsupported CLI behaviors**, not idempotence guarantees.

Future checkpointed ingestion needs an unambiguous source/harness namespace and
backend-native revision token, plus bindings to destination, projection version
and content policy. It must label byte ranges separately from logical record IDs
and ordinals, and publish progress only after destination acceptance; crash/replay
deduplication needs an explicit contract rather than an assumed atomic two-file
write. None of that work is silently included in this first export command.

## Follow-on changes needed for full fidelity

1. Separate raw evidence retention from the current semantic/export projection,
   with explicit consent, unknown-field/opaque-payload handling and source digests.
2. Introduce backend-native source and revision tokens rather than inventing LF
   offsets for SQLite databases, snapshot files or patch journals.
3. Reconcile stable native item identity plus revision/content identity, retain
   update/delete history, and make derived usage views idempotent rather than
   summing repeated snapshots.
4. Add an explicit, scope-limited reference loader that returns captured,
   unavailable, denied and pending artifacts without giving I/O to pure adapters.
5. Supply observation/arrival time from the outer service and report completeness
   as unknown/provisional until the source offers a defensible finality guarantee.
6. Exercise delayed append, late insert, same-ID revision, file rewrite/delete and
   delayed referenced-data fixtures across each actual backend before claiming
   that backend's full-fidelity support.

These are named product follow-ons, not implemented behavior or new completion
barriers for the agreed first Claude pipeline.
