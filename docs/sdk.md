# Rust SDK

`unisphere-sdk` 0.1.0 provides in-process configuration inspection and an injected
session collection service on Rust 1.95.0, edition 2024. No daemon, CLI subprocess,
async runtime, Node, Builder, database or model service is required. Concrete
loaders, pure source adapters and output writers are composed explicitly rather
than selected through ambient configuration.

## Consume from another Cargo package

```toml
[dependencies]
unisphere-sdk = { path = "../Unisphere/crates/sdk" }
```

Adjust the dependency path to your checkout. A successful inspection:

```rust
use unisphere_sdk::{ConfigSource, InspectionRequest, inspect};

fn main() -> Result<(), unisphere_sdk::Failure> {
    let report = inspect(&InspectionRequest {
        source: ConfigSource::Inline(
            br#"{"source_roots":[" relative-root ","~/literal","relative-root"]}"#.to_vec(),
        ),
        ..InspectionRequest::default()
    })?;
    assert_eq!(
        report.configuration.source_roots,
        [" relative-root ", "~/literal", "relative-root"],
    );
    Ok(())
}
```

Run the consumer with `cargo run`. Source roots remain literal strings: whitespace,
order, duplicate entries, relative forms, `~`, and environment-like text are not
rewritten. The library does not check whether roots exist or open them.

## Configuration and precedence

The document must be a UTF-8 JSON object with only the optional `source_roots`
field. `{}` and `{"source_roots":[]}` are valid. Values must be an array of
strings, each with a nonempty Rust `str::trim()` result. Invalid UTF-8, malformed
JSON, non-object documents, unknown keys, duplicate known keys, incompatible
value types, and blank strings are rejected.

| Priority | Input | Omitted versus empty |
|---|---|---|
| Lowest | `Configuration` passed to `Inspector::with_defaults` | Built-in defaults are an empty list |
| Middle | Explicit inline/file document | Omitted `source_roots` retains defaults; `[]` clears |
| Highest | `ConfigOverrides::source_roots` | `None` retains; `Some(vec![])` clears |

Every supplied layer is validated. Valid overrides cannot conceal an invalid
document or invalid caller defaults. Defaults are checked on each inspection,
not when the inspector is constructed. The first detected invalid layer returns
a failure, not a partially resolved configuration.

`ConfigSource::Defaults` reads nothing. `ConfigSource::Inline` consumes caller
bytes. `ConfigSource::File` requires an explicit absolute path; relative file
paths fail before invoking the reader. The SDK never searches the working
directory, HOME, XDG directories, or environment variables for configuration.
The CLI may resolve its explicitly supplied file argument before calling the SDK;
that is not an SDK ambient lookup.

Inline and file documents have a 1,048,576-byte limit (`MAX_CONFIG_BYTES`), including
JSON whitespace. Inline oversize is `InvalidConfiguration` with no location;
file oversize is `ConfigurationRead` with `ReadFailure::TooLarge` and the supplied
path. `StdConfigReader` reads regular files, maps I/O errors to core categories,
and reads no more than the requested limit plus one byte, even if the file grows
after its size check. It rejects direct relative-path calls with `ReadFailure::Other`.
Symlinks are followed by the OS; this API is not a filesystem sandbox or a
canonicalization facility. Callers choose and authorize their explicit file paths.

## Inject a reader and defaults

The service depends only on the `ConfigReader` port. Implement it to choose an
application-owned byte source; there is no global registry or hidden fallback.
The reader must honor `max_bytes` without first copying an unbounded document.

```rust
use std::path::Path;
use unisphere_sdk::{
    ConfigOverrides, ConfigReader, ConfigSource, Configuration, InspectionApi,
    InspectionRequest, Inspector, ReadFailure,
};

struct ApplicationReader;

impl ConfigReader for ApplicationReader {
    fn read(&self, _path: &Path, max_bytes: usize) -> Result<Vec<u8>, ReadFailure> {
        let bytes = br#"{"source_roots":["document"]}"#;
        if bytes.len() > max_bytes {
            return Err(ReadFailure::TooLarge);
        }
        Ok(bytes.to_vec())
    }
}

fn main() -> Result<(), unisphere_sdk::Failure> {
    let inspector = Inspector::with_defaults(
        ApplicationReader,
        Configuration { source_roots: vec!["default".into()] },
    );
    let report = inspector.inspect(&InspectionRequest {
        source: ConfigSource::File(if cfg!(windows) {
            r"C:\app\config.json".into()
        } else {
            "/app/config.json".into()
        }),
        overrides: ConfigOverrides { source_roots: Some(vec![]) },
    })?;
    assert!(report.configuration.source_roots.is_empty());
    Ok(())
}
```

`Inspector::new(reader)` selects empty defaults. `Inspector<R>` implements
`InspectionApi` and is `Send + Sync` through its reader contract. Independent
instances may use different readers/defaults concurrently. Inline/default
requests do not invoke even the injected reader.

## Handle a failure without parsing prose

```rust
use unisphere_sdk::{ConfigSource, FailureKind, InspectionRequest, inspect};

fn main() {
    let failure = inspect(&InspectionRequest {
        source: ConfigSource::Inline(br#"{"source_roots":[" "]}"#.to_vec()),
        ..InspectionRequest::default()
    }).unwrap_err();
    assert_eq!(failure.kind(), FailureKind::InvalidConfiguration);
    assert_eq!(failure.code(), "UNI-CONFIG-INVALID");
    assert_eq!(failure.location().unwrap().field.as_deref(), Some("source_roots[0]"));
    eprintln!("{}", failure); // Fixed code, message, and remediation; no input values.
}
```

`FailureKind::InvalidConfiguration` (`UNI-CONFIG-INVALID`) describes invalid
configuration. `FailureKind::ConfigurationRead` (`UNI-CONFIG-READ`) describes
explicit input I/O, with `read_failure()` distinguishing `NotFound`,
`PermissionDenied`, `TooLarge`, and `Other`. `InvalidArguments` is available in
the re-exported core contract for frontends; the SDK inspection service does not
produce it. Failures are not retryable in this foundation.

Locations identify supplied file paths, known fields/indexes, and JSON
line/column positions where known. Unknown document keys and root values are
never copied into diagnostics. A path can itself be sensitive: applications
choose whether to expose the optional location. Do not log raw
`InspectionRequest`/`ConfigSource` debug representations; they contain input bytes.

## Development proof and support limits

The scoped SDK contracts run with:

```sh
cargo test -p unisphere-sdk --lib --tests
cargo test -p unisphere-sdk --doc
```

`tests/service_in_isolation.rs` compiles the actual service source against core
and an injected fake without compiling the SDK facade or filesystem module into
that test's service module. Filesystem tests create private temporary fixtures;
no real user configuration or session stores are inputs. The assembled plan's
external-consumer, CLI parity, and dependency checks are separate PM-owned proof;
SDK unit tests alone do not establish installation or no-network syscall proof.

The configuration operation itself does not read sessions. Collection supports
the documented native JSONL and revision-snapshot projections, not lossless
telemetry or complete session reconstruction; see [fidelity.md](fidelity.md).
Concrete filesystem/SQLite loading is Unix-only; mapping and encoding operate on
supplied data. Runtime proof is limited to the exercised platforms and fixtures.

## Collect a batch through injected ports

Add path dependencies on `crates/loader-jsonl`, `crates/adapter-claude` and
`crates/output-otlp` when selecting those concrete implementations; the SDK itself
depends inward on core contracts, not those adapters.

```rust
use unisphere_sdk::{CollectionApi, Collector, MappingOptions, ReadLimits, SessionRef};
use unisphere_loader_jsonl::FileSessionLoader;
use unisphere_adapter_claude::ClaudeCodeAdapter;
use unisphere_output_otlp::OtlpJsonlWriter;

let collector = Collector::new(FileSessionLoader, ClaudeCodeAdapter, OtlpJsonlWriter);
let source = SessionRef { path: "/explicit/session.jsonl".into() };
let mut output = Vec::new();
let batch = collector.collect_batch(
    &source, None, ReadLimits::default(), MappingOptions::default(), &mut output,
)?;
// Retain batch.next_cursor only after your destination policy accepts output.
# Ok::<(), unisphere_sdk::PipelineError>(())
```

The same `CollectionApi` can be backed by a fake collector for a frontend test.
For lower-level tests inject `FakeSessionLoader`, a pure adapter and
`FakeRecordWriter`; adapters themselves take bytes directly and need no mock I/O.

The returned cursor is caller-owned, not automatically persisted. A write/flush
failure returns no accepted checkpoint, but may leave partial output; retries are
not exactly-once. `more` and `incomplete_tail` describe the current read boundary,
not producer finality. `MappedBatch` contains typed diagnostics and physical source
provenance, not a claim that unsupported payloads or references were captured.

## Export a complete native snapshot revision

`SnapshotCollector` composes the separate `SnapshotLoader`, `SnapshotAdapter` and
existing `RecordWriter`. Its core-owned `SnapshotCollectionApi` takes a
`SnapshotRequest`; it never invents a `ReadCursor` or LF offset for a database or
whole-document revision.

```rust
use unisphere_sdk::{
    MappingOptions, SnapshotCollectionApi, SnapshotCollector, SnapshotFormat,
    SnapshotLimits, SnapshotRef, SnapshotRequest,
};
use unisphere_loader_snapshot::FileSnapshotLoader;
use unisphere_adapter_vscode_copilot::VsCodeCopilotAdapter;
use unisphere_output_otlp::OtlpJsonlWriter;

let collector = SnapshotCollector::new(
    FileSnapshotLoader, VsCodeCopilotAdapter, OtlpJsonlWriter,
);
let request = SnapshotRequest {
    source: SnapshotRef {
        path: "/explicit/session.jsonl".into(),
        format: SnapshotFormat::JsonJournal,
        session_id: None,
    },
    limits: SnapshotLimits::default(),
    options: MappingOptions::default(),
};
let mut output = Vec::new();
let result = collector.collect_snapshot(&request, &mut output)?;
// result.checkpoint is returned only after the writer accepts the full batch.
# Ok::<(), unisphere_sdk::PipelineError>(())
```

Formats are `JsonDocument`, `JsonJournal`, and `SqliteKeyValue { table }`. The
loader returns bounded raw documents/operations/key-value rows plus a content
revision; journal reconstruction stays in the pure VS Code mapper. The service
validates the requested source/limits and revalidates injected loader results.
An unrelated returned source fails before mapping/output.

Each invocation emits the complete current projection followed by one
`unisphere.session.snapshot` replacement manifest, even if the projection is
empty. It returns a `SnapshotCheckpoint` binding source, revision, adapter and
content policy after successful output. It does **not** implicitly skip matching
revisions, persist checkpoints, retain historical versions, bind a destination or
provide exactly-once ingestion. Consumers apply replacement only after accepting
the complete output; partial write/flush failure returns no checkpoint.

The manifest identifies `replace_projection`, record count, source selection,
content policy and unknown finality. A revision is an observation of that bounded
source representation, not producer finality or a global clock. Caller-side
history, destination transactions and cross-source deduplication remain separate.

## Prepare canonical tables in process

`unisphere prep` is composed from public ports, so an external consumer runs the
same incremental prep in process with its own store. `unisphere_sdk::prep`
re-exports the `core::prep` contract: `PrepLoader` (read-only discovery, stat,
bounded reads to the last complete LF, anchors, `record_at`), the pure
`PrepFold`/`PrepFoldSession`, and `PrepStore` (`state`, `load`, `commit` rows
before state, `compact`). Core and SDK carry no Parquet, SQLite or engine
dependency; `unisphere-output-prep`'s `ParquetPrepStore` is one store, and any
`PrepStore` — including an in-memory one — is another.

```rust
use std::sync::Arc;
use unisphere_sdk::prep::{
    PrepApi, PrepBinding, PrepOptions, PrepReadLimits, PrepRequest, PrepSourceSet, Preparer,
};
use unisphere_sdk::{ReadLimits, SnapshotLimits};
use unisphere_adapter_claude::ClaudePrepFold;
use unisphere_loader_jsonl::FileSessionLoader;

# fn run(my_store: impl unisphere_sdk::prep::PrepStore) -> Result<(), unisphere_sdk::PipelineError> {
let preparer = Preparer::new(
    vec![PrepBinding { fold: Arc::new(ClaudePrepFold), loader: Arc::new(FileSessionLoader) }],
    my_store,
);
let report = preparer.prep(&PrepRequest {
    target: "/explicit/target".into(),
    roots: vec![PrepSourceSet {
        harness: "claude-code".into(),
        label: "default".into(),
        root: "/home/me/.claude/projects".into(),
    }],
    options: PrepOptions::default(), // metadata only
    limits: PrepReadLimits {
        read: ReadLimits { max_records: usize::MAX, ..ReadLimits::default() },
        snapshot: SnapshotLimits::default(),
    },
    threads: 4,
    modified_since_ns: None,
})?;
// report.sets: per-set coverage; report.sources: every source neither unchanged nor skipped.
# Ok(()) }
```

`Preparer` owns every prep decision: root binding by harness (an unbound harness
is reported `supported: false` with its sources `unsupported`), change detection
into `new`/`unchanged`/`appended`/`replaced{reason}`/`skipped`/`unreadable`/
`missing`, generations, pending tails, bounded parallelism and the content gate on
`record`. The CLI builds the same `PrepRequest` and renders the returned
`PrepReport`; the equivalent command is:

```sh
unisphere prep --target /explicit/target --harness claude-code --threads 4
```

`fold_source` is the single-source fold `prep` itself runs, without a store or
target directory — for example, live session status from `SessionFacts`:

```rust
use unisphere_sdk::prep::{fold_source, PrepLoader, PrepOptions, PrepReadLimits, PrepSourceSet};
use unisphere_adapter_claude::ClaudePrepFold;
use unisphere_loader_jsonl::FileSessionLoader;

# fn run(set: PrepSourceSet, limits: PrepReadLimits) -> Result<(), unisphere_sdk::PipelineError> {
let loader = FileSessionLoader;
let stat = loader.stat(&set.root, &set.root.join("project/session.jsonl"))?;
let folded = fold_source(
    &loader, &ClaudePrepFold, &stat, &set.source_key(&stat.file),
    0, None, PrepOptions::default(), limits,
    &mut |rows| { /* this batch's calls/turns/triggers/events/tool_uses */ },
)?;
// folded.facts: SessionFacts; keep folded.cursor + folded.checkpoint as a
// PrepResume to continue from the last complete record next time.
# Ok(()) }
```

## Query supplied session evidence in process

`QueryService<S>` implements `QueryApi` for any injected `S: QuerySource`. The
source receives a typed `SourceSelection` before it enumerates or reads stores;
the SDK never discovers HOME, the current directory, Git, native clients or a
network service on its own. A caller can execute a one-shot request or retain an
immutable `QueryView` and call `execute_view` repeatedly without another source
read:

```rust
use unisphere_sdk::{QueryService, execute_view};
use unisphere_sdk::query::{
    Dataset, Operation, QueryApi, QueryFailure, QueryRequest, QuerySource,
};

fn inspect<S: QuerySource>(
    service: &QueryService<S>,
    request: &QueryRequest,
) -> Result<(), QueryFailure> {
    let response = service.execute(request)?;
    assert_eq!(response.dataset, Dataset::Sessions);

    let view = service.open_view(request)?;
    let same_semantics = execute_view(&view, request)?;
    assert_eq!(same_semantics.query.operation, Operation::List.kind());
    Ok(())
}
# let _ = inspect::<unisphere_testkit::query::FakeQuerySource>;
```

The `unisphere_sdk::query` facade re-exports the core query vocabulary.
`QuerySource::load` accepts only the explicit `QueryScope`, pre-I/O
`SourceSelection`, `QueryLimits` and derived `ContentAccess`, and returns either
a supplied `NativeQueryView` or
bounded versioned saved input. Concrete discovery/loaders remain separate
adapters composed by the application.

### Immutable view and typed datasets

`QueryView` owns six typed collections: `SourceRow`, `SessionRow`, `TurnRow`,
`MessageRow`, `ToolRow` and `EventRow`. These raw in-process evidence rows are
not serializable. `execute_view` selects the same rows for list, show, tree,
extract and statistics, then returns only validated `ProjectedRow` values.

Local entity IDs never masquerade as native conversation IDs. Session identity
is scoped by source, native namespace and participant evidence. Source-only
fragments remain source/event evidence when session membership is unavailable.
Initiating request markers create turns; tool-result, injected-context and
summary records do not. Calls pair with results/progress only by a supported
native call ID inside the same session/branch. Missing, duplicate or reversed
evidence remains explicitly incomplete, ambiguous or invalid-clock rather than
being paired by adjacency or text.
Validated native parent trees derive one branch per terminal path. Shared-prefix
rows carry every descendant branch membership; the SDK never selects an active
leaf. Turn continuation, tool pairing and context expansion use those exact
memberships, so sibling branches cannot collapse into one transcript.

Every view exposes `digest()`, `admitted_scope()`, `source_selection()`,
`retained_capability()` and `input_basis()`. The digest binds schema and
reconstruction versions, admitted scope/repository roots, selected source IDs,
revisions and policy versions, association/read facts, retained fields and saved
input origin. It deliberately excludes row filters, output columns, page size,
cursors, result universes, actions and matched/emitted counts. Reopen a view when
the source revision, admission or required retained capability changes.

### Filters, time, ordering and continuation

Filters are typed: different field/operator groups combine with AND, repeated
values in one group combine with OR, then exclusions subtract. Literal contains,
Rust regex and glob matching are distinct and case-sensitive unless
`ignore_case` is set. Unsupported fields or predicates fail against the dataset
schema; unknown values do not satisfy positive comparisons. Adapter and harness
equal/in/exclude predicates additionally become the pre-I/O `SourceSelection`.

`since` is inclusive and `until` exclusive. Date-only values are UTC midnight.
Sessions, turns and tools default to `started_at`; messages and events use
`timestamp`. Session creation, first observed event and source-file modification
are separate facts. Undated rows match a bounded time window only when
`include_undated` is explicit. Unknown sort values remain last in either
direction, with stable entity-ID tie-breaking.

List defaults to 50 rows; `limit: Some(0)` means all within `QueryLimits`.
Continuation tokens contain only a version, view digest, normalized request
digest, next index and corruption checksum. They carry no paths, content or sort
payload and are not authorization. Changed request options and changed source
views produce distinct stale-cursor reasons instead of silently restarting.

Turn/message context expands after matching and stays within each admitted
session/branch native order. Overlapping windows are unioned; contextual rows
may therefore fall outside the original time predicate. Every emitted row carries
the metadata-only `is_context` boolean (`false` for a match, `true` for an added
neighbour), even when explicit columns omit it. Offline context requires complete
partition/order/membership metadata rather than silently shortening a window.

### Privacy and saved input

Metadata projection is the default. Native IDs, names/models, paths, message
text/parts, tool names/commands/arguments/results and reasoning are sensitive.
A sensitive search authorizes local inspection of only that field; it does not
authorize emission. Requesting a sensitive column requires `include_content`,
otherwise `UNI-QUERY-CONTENT-CONSENT` returns a typed
`UseMetadataOrConsent` recovery. Responses contain approved projections, safe
coverage, a per-response `ResultUniverse` and a semantic `QueryAction`; they do
not contain the raw `QueryView`.

Saved `QueryJsonV1` input validates the versioned response envelope, coverage,
universe, unique row IDs, field types and source revisions. Standalone
`QueryJsonlV1` rows are always a bounded provided-row universe with unknown
completeness; EOF is not proof of complete capture. Filtering/list/show can use
available rows. Statistics remain explicitly input-bounded, while context or
higher-level reconstruction that needs missing partition evidence returns
`InputSubset` with `UseCompleteInput`. Neither path triggers live enrichment.

### Statistics and limits

Statistics reduce the full logical matched set before group pagination. Tool
duration metrics include only finite source-reported or valid paired-clock
measurements; missing durations are counted separately. Percentiles use exact
nearest-rank `ceil(p*n)`. Failure-rate denominator is
`succeeded + failed + cancelled`; cancellation is neither success nor failure,
and incomplete/unknown calls are excluded. Cumulative usage snapshots contribute
only the latest compatible native-owned value instead of being summed as replayed
usage.
Derived aggregate and usage metric fields are valid only for statistics. Using
one as a column or sort key on a row operation returns
`UnsupportedOperation` before `QuerySource::load`; source-qualified row fields
such as a tool call's `duration_ms` remain available to regular operations.

`QueryLimits` bound source/input bytes, observations and rows, retained payload,
patterns and scanned text, context neighbours, branch memberships, cursor size
and output size. Invalid, zero, overflowing or over-hard-ceiling limits fail
before source I/O. Bound violations and availability failures carry stable codes,
safe explanations and typed recovery actions; no diagnostic includes raw source
payload or a content-bearing filter value.

`QueryLimits::default()` permits 1 GiB of total input, equal to that field's
existing hard ceiling. Other defaults remain independent: 64 MiB per source,
200,000 observations/rows, 256 MiB retained data and 128 MiB output. Raising the
input allowance does not raise these budgets or change query materialisation
and paging behavior.

## Read Git-ai-format Git Notes

Add path dependencies on `crates/loader-git`, `crates/adapter-git-ai` and
`crates/output-otlp`. These are Unisphere implementations, not Git AI libraries.
The SDK itself still depends inward on core, not concrete loaders/adapters.
Standard Git is the only Git-side runtime prerequisite; Git AI need not exist.

```rust
use unisphere_sdk::{
    GitNoteSelection, GitNotesApi, GitNotesCollector, GitNotesLimits,
    GitNotesRequest, GitNotesScope, MappingOptions,
};
use unisphere_loader_git::GitObjectLoader;
use unisphere_adapter_git_ai::GitAiAdapter;
use unisphere_output_otlp::OtlpJsonlWriter;

let collector = GitNotesCollector::new(
    GitObjectLoader::new("/usr/bin/git".into()),
    GitAiAdapter,
    OtlpJsonlWriter,
);
let scope = GitNotesScope {
    repository: "/explicit/repository".into(),
    notes_ref: "refs/notes/ai".into(),
    selection: GitNoteSelection::All,
};
let limits = GitNotesLimits::default();
let listing = collector.list_notes(&scope, limits)?;
let mut output = Vec::new();
let result = collector.collect_notes(
    &GitNotesRequest { scope, limits, options: MappingOptions::default() },
    &mut output,
)?;
# Ok::<(), unisphere_sdk::GitNotesError>(())
```

Use a trusted absolute standard Git executable. `GitObjectLoader` runs on Unix;
the pure `GitAiAdapter` accepts supplied `LoadedGitNote` values on any platform.
Neither parser nor SDK reads environment/configuration, invokes Git AI, or
dereferences message URLs. There is no upstream Git AI code/library dependency.

`GitNoteSelection::Commits(vec![full_oid])` selects exact lowercase SHA-1/SHA-256
commit IDs. Duplicates collapse deterministically; an empty vector selects
nothing, unlike `All`. Selected lookup follows only matching Git-notes fanout
paths, so an unrelated large notes tree need not fit the all-notes listing budget.
No tracking refs are discovered or aggregated.

The listing retains the canonical selected repository, Git common-directory
identity, per-worktree Git directory, optional worktree top-level, requested ref
and pinned ref tip. Each `GitNoteRef` adds target commit and note blob. Through
the `GitNoteLoader` trait, `read_note(&reference, limits)` verifies membership
against that immutable tip, even after the named ref changes. This pins identity,
not retention: later Git garbage collection can make old objects unreadable.

Missing valid refs (`notes_tip: None`) and existing empty selections (`Some(tip)`)
are successful, distinct results. Invalid refs, non-commit selected targets,
missing objects, unsupported/malformed notes, ownership refusal, unavailable Git,
deadlines and limits are typed `GitNotesError` failures. Unknown source values
are not filled with zero. Valid unresolved attribution keys remain explicit
unresolved evidence, without speculative cross-note or cache lookup.

Collection stages the complete bounded selection and writes one OTLP batch with
a closing `unisphere.git_notes.snapshot` manifest, including for empty results.
`records_written` includes that manifest. No result is accepted after mapping,
write or flush failure; a destination failure can leave partial bytes. Consumers
replace only the identified repository/ref/selection after accepting the entire
batch. There is no persisted cursor, history store or exactly-once guarantee.
See [CLI limits](cli.md#git-notes-attribution) and the
[Git Notes profile](telemetry-profile.md#git-notes-attribution-and-selection-manifests).
