# Rust SDK

`unisphere-sdk` 0.1.0 is an in-process configuration inspection library targeting
Rust 1.95.0, edition 2024. This foundation does not collect telemetry, discover
harnesses, read session content, normalize records, or persist data. No daemon,
CLI process, async runtime, Node, Builder, database, or model service is required
at runtime. Registry publication is outside this release; use the checkout.

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

No native-session format or telemetry compatibility is promised here. The SDK
uses portable Rust APIs, but OS-specific permissions and path forms remain OS
contracts; support claims are limited to the platforms exercised by the project
proof lane. This implementation introduces no copied Flowspace3 or git-ai source;
architectural inspiration does not add their runtime dependencies.
