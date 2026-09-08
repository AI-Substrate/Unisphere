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
