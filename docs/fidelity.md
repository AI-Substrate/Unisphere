# Telemetry fidelity: implemented boundaries and remaining gaps

**These are source-derived JSONL and native snapshot projections, not a lossless
archive or complete telemetry history.** A successful export means the selected
representation was handled under the chosen policy. EOF or a snapshot revision
never establishes that the producer has finished emitting or revising data.

Full fidelity means preserving the telemetry the source actually makes available,
including native facts not yet understood by our mapping, and identifying what was
missing, deliberately excluded, unsupported or not verified. It cannot reconstruct
content the source never stored or has already irreversibly redacted.

The original Claude assessment was a delivery report, not an implicit expansion
gate. Plan010 separately authorizes six more applications and native snapshot
storage. The explicit-content policy is unchanged; supported projection is not
full-fidelity retention.

## Current native coverage and classified gaps

| Application / dialect | Preserved projection | Actual exercised proof | Remaining gap / classification |
| --- | --- | --- | --- |
| [Claude JSONL](claude-adapter.md) | Physical message/tool/usage fragments and native provenance. | Existing external SDK/installed CLI parity and loader/conformance cases. | Unknown/raw fields, sidecars and full conversation reconstruction are unsupported; detailed original matrix below. |
| [Codex rollouts](codex-adapter.md) | Record-local messages/reasoning/tools, native IDs/model context and distinct last/cumulative usage observations. | Ten scoped regressions plus real SDK/CLI/installed parity in both policies. | Encrypted/media payloads, streaming/world/custom state and unknown newer variants unsupported; repeated counters are not aggregate operation totals. |
| [Oh My Pi JSONL](oh-my-pi-adapter.md) | Title slot, physical v3 entries, messages/tools, control records and scoped native usage. | Eleven scoped regressions, title/partition cases and real export parity. | Retitles before a saved byte cursor, branch/header enrichment, old-header migration, opaque blobs/signatures/custom state and sidecars are unsupported. |
| [Pi v3 JSONL](pi-adapter.md) | Physical tree IDs, built-in roles, structured content/tools and separate assistant/tool/summary usage. | Ten scoped regressions and real export parity. | v1/v2 migration, hook/extension roles, opaque data, active-branch reconstruction and rewritten-history reconciliation unsupported. |
| [Copilot CLI events](copilot-cli-adapter.md) | Native event/lineage/tool/model facts; message, API-call, checkpoint and shutdown counters keep separate scopes. | Ten event regressions and real export parity. | Binary/provider-private internals and unknown variants unsupported; no replay/dedup/finality guarantee. |
| [Copilot legacy JSON](copilot-cli-adapter.md) | Separate session/chat/timeline views with key/revision provenance and native argument types. | Eight snapshot regressions and real document export parity. | Coverage is bounded by retained two-file structural evidence; model/usage or message timestamps absent in that evidence are not inferred. Opaque mentions/unknown variants unsupported. |
| [VS Code Copilot](vscode-copilot-adapter.md) | v1/v2/v3 snapshots and pure kind0/1/2/3 journal reduction into final request/response state; scoped usage/tool observations. | Fourteen regressions; real JSON/journal SDK/CLI/installed parity, changed/deleted requests and partial/late journal scenarios. | Historical versions of the unversioned journal envelope, unsupported parts/sidecars and full edit history are unsupported. Sparse expansion is bounded; producer-omitted LM tool parameters are a source limitation. |
| [Cursor transcripts](cursor-adapter.md) | Native role/text/idless tools, overview/control and turn-ended records. | Ten transcript regressions and real export parity. | The native serializer discards model/usage/timing/IDs/tool results: source limitations, not facts to reconstruct. Full-write fallback exceeds append-only cursor guarantees. |
| [Cursor IDE SQLite](cursor-adapter.md) | Composer-declared bubble order/identity, explicit native dates/model configuration, measured counters and opt-in native tools. | Thirteen IDE regressions; real read-only SQLite export parity, late rows, changed values and empty deletion replacement. | Alternate branches/blob state, unavailable composerHeaders cross-table joins and opaque CLI BLOB codec are explicitly unsupported; no complete Cursor CLI blob-store claim. |

All rows use synthetic fixtures and bounded structural/source evidence, not copied
private transcripts. Referenced attachments/context/spills remain unresolved by
policy. Metadata-only is not anonymity; IDs, paths and permitted native metadata
can be sensitive.

## Revision behavior, not retained history

The shared snapshot loader returns a complete bounded raw representation and a
SHA256 revision. SQLite uses a read-only transaction and native keys; JSON journals
are reduced by the pure mapper before projection. Limits are enforced during
loading, with no partial snapshot success. The SQLite loader's initial ancestor-
symlink failure was retained and corrected without weakening final-leaf nofollow,
read-only or WAL-consistency checks; 23 scoped storage tests then passed.

Each snapshot export closes with a `replace_projection` manifest and unknown
finality. A changed/deleted source revision replaces the *current projection*; it
does not create a history database, reconcile arbitrary past revisions or make a
sink idempotent. The SDK returns checkpoint evidence only after output acceptance;
it never silently skips a repeated revision or persists a destination binding.
An output failure may leave bytes and returns no accepted checkpoint.

`unisphere-proof native` exercises external SDK, in-tree CLI and a temporary
installed CLI across every registered dialect and both content policies, then
real changed/deleted JSON and SQLite sources, incomplete/late journals and a
partial destination write without checkpoint publication. This is bounded runtime
evidence, not all-platform, all-dialect, network-denial or full-fidelity proof.

## Original Claude JSONL baseline

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

JSONL selection uses `--adapter <registered-id> --input <path>`, not `--harness`
plus global session lookup. Each invocation starts at byte zero; `--output`
creates a new file, never reopens a destination for resume. The SDK returns a
caller-owned cursor, but the CLI persists no checkpoint.

The summary reports mapped record count, batches, aggregate diagnostic count,
incomplete-tail state and the final byte offset. It does not report starting
cursor, native physical-line count, per-code diagnostic breakdown or a source
record-ordinal from/to range. SDK callers retain the individual diagnostic codes
and offsets. These are **unsupported CLI behaviors**, not idempotence guarantees.

Native snapshots instead report a content revision and closing replacement
manifest with unknown finality; no byte offset or record-ordinal range is invented.
Their `--session-id` is a selector within the explicitly supplied snapshot, not
automatic application/session discovery.

Future checkpointed ingestion needs an unambiguous source/harness namespace and
backend-native revision token, plus bindings to destination, projection version
and content policy. It must label byte ranges separately from logical record IDs
and ordinals, and publish progress only after destination acceptance; crash/replay
deduplication needs an explicit contract rather than an assumed atomic two-file
write. Durable ingestion remains separate from the current explicit export paths.

## Follow-on changes needed for full fidelity

1. Separate raw evidence retention from the current semantic/export projection,
   with explicit consent, unknown-field/opaque-payload handling and source digests.
2. Build durable consumer policy around the current native snapshot tokens and
   append cursors, with explicit destination/projection/content-policy bindings.
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

These are named fidelity follow-ons, not claims that the supported projections
are lossless or new completion barriers for the agreed delivery scopes.
