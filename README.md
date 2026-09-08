# Unisphere

Rust SDK and CLI for explicit configuration inspection and native agent-session
projection. Shared bounded JSONL and revision-snapshot loaders supply data to pure
Claude, Codex, Oh My Pi, Pi, Copilot, VS Code and Cursor adapters; a separate writer
emits OTLP LogsData JSONL.

**This is not lossless or complete-session capture:** metadata-only is the default,
content is opt-in, and unsupported fields/references/revisions have documented
[fidelity gaps](docs/fidelity.md). No daemon or implicit private-store discovery.

## Build and run

Use Rust 1.95.0 with edition 2024. Node, DD and Builder are development tools,
not requirements of the SDK or installed CLI.

```sh
cargo build --locked --workspace
cargo run --locked -p unisphere-app -- config check --json
cargo install --locked --path crates/app --root target/install
./target/install/bin/unisphere config check --source-root ./sessions --json
```

## Discover adapters and export explicit sources

The catalog describes usual locations without scanning them. Unix JSONL listing
is nonrecursive; callers choose an explicit leaf directory or file:

```sh
unisphere adapters list --json
unisphere sessions list --root /absolute/path/to/leaf-project
unisphere sessions export --adapter claude-code --input /absolute/path/session.jsonl
unisphere sessions export --input /absolute/path/session.jsonl --include-content --output ./new-export.jsonl
unisphere sessions export --adapter codex --input /absolute/path/rollout.jsonl
unisphere sessions export --adapter vscode-copilot --input /absolute/path/session.jsonl --source-format json-journal
unisphere sessions export --adapter cursor-ide --input /absolute/path/state.vscdb
```

Exports write OTLP JSONL to stdout or create a **new** output file; diagnostics and
the summary use stderr. Existing output files are not overwritten. Metadata-only
still includes source paths, IDs, model and kind metadata—it is not anonymity.

Reads are bounded by `--max-records`, `--max-record-bytes` and `--max-batch-bytes`.
An oversized record fails without skipping it; retry with larger compatible
limits. An incomplete tail is reported and deferred. EOF is an observed boundary,
not final source completeness.

Snapshots use separate key/content-revision identities, not invented byte offsets.
They export a complete current projection and closing replacement manifest,
including an empty result after deletion. This is not retained history or a
destination transaction. See [snapshot loading](docs/snapshot-loader.md) and
[CLI source-format selection](docs/cli.md#native-revision-snapshots).

Each CLI export starts at the beginning; there is no persisted resume or
`--harness`/session-ID lookup yet. The SDK exposes caller-owned cursors and returns
the next one only after the output accepts a batch. It does not promise exactly-once
delivery or automatic recovery from in-place revisions.

## Existing configuration inspection

The default configuration is an empty root list. A configuration document is a
JSON object such as `{"source_roots":["./sessions","~/literal"]}`; `{}` and an
empty list are valid. Roots remain literal strings: no expansion, normalization,
existence check, or session access is performed.

```sh
unisphere config check --config ./config.json --json
unisphere config check --config ./config.json --clear-source-roots --human
unisphere config check --source-root ' ' --json  # invalid configuration; exit 1
unisphere --help
unisphere --version
```

Precedence is defaults < explicit document < explicit overrides. Repeated
`--source-root` values replace the document list; `--clear-source-roots` clears it.
Every supplied layer is validated, even when overridden. Explicit documents are
limited to 1,048,576 bytes. Unknown/duplicate keys, invalid JSON/UTF-8, wrong value
types and blank roots are rejected without echoing document values in errors.

For configuration commands, captured/piped output defaults to one JSON envelope plus a newline. Terminal
output defaults to human-readable text; `--json` and `--human` override detection.
Both flags together are an argument error rendered as JSON. Human failures use
stderr; machine operation failures use a structured stdout envelope.

| Exit | Meaning |
| --- | --- |
| 0 | Successful check, help or version |
| 1 | Configuration, explicit-input I/O or output/runtime failure |
| 2 | Invalid command arguments |

## Use the SDK

An external Cargo application can depend on this checkout directly. Adjust the
path to its `crates/sdk` directory; registry publication is not required.

```toml
[dependencies]
unisphere-sdk = { path = "../Unisphere/crates/sdk" }
```

```rust
use unisphere_sdk::{inspect, ConfigSource, InspectionRequest};

fn main() -> Result<(), unisphere_sdk::Failure> {
    let report = inspect(&InspectionRequest {
        source: ConfigSource::Inline(br#"{"source_roots":["./sessions"]}"#.to_vec()),
        ..InspectionRequest::default()
    })?;
    assert_eq!(report.configuration.source_roots, ["./sessions"]);
    Ok(())
}
```

For injected I/O and caller defaults, use `Inspector<R>` through `InspectionApi`.
The SDK reexports the core ports and types. Library calls have no implicit HOME,
environment or global configuration lookup; an SDK file source must be absolute.

For JSONL, inject `SessionLoader`, `SessionAdapter` and `RecordWriter` into
`Collector`. For native snapshots, inject `SnapshotLoader`, `SnapshotAdapter` and
the same writer into `SnapshotCollector`. Both expose core-owned application
ports; the SDK depends inward, not on concrete adapters. Mappers receive only
provided data and return contracted records—never file handles or loader objects.

## Development and proof

- [SDK contract and examples](docs/sdk.md)
- [CLI envelope, modes and exits](docs/cli.md)
- [Toolchain selection, validation and isolation](docs/development.md)
- [Licence and reuse notes](THIRD_PARTY_NOTICES.md)
- [Shared loader and cursor limits](docs/session-loader.md)
- [Pure Claude mapping coverage](docs/claude-adapter.md)
- [Normative OTLP output profile](docs/telemetry-profile.md)
- [Implementing another adapter](docs/adapters.md)
- [Full-fidelity meaning and current gaps](docs/fidelity.md)

`harness checks --json` is the quality lane. `harness boot --json` additionally
exercises configuration parity, external SDK consumption, temporary installation,
every registered native dialect and changed/deleted/late snapshot behavior under
sealed runtime environments. Passing boot is scoped to
`configuration-and-native-session-projections`, not full-fidelity collection.
Source/purity review complements the lexical sensor; neither is a runtime
network-denial trace.

Local verification targets macOS ARM64 with the documented toolchain. Linux/macOS
CI configuration is provided; its presence alone is not an observed CI result.

MIT licensed; see [LICENSE](LICENSE).
