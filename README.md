# Unisphere

Rust SDK and CLI foundation for a common-format agent telemetry collector.
The current operation validates **explicit source-root configuration** and returns
effective settings or typed, actionable failures. It does not read sessions,
collect telemetry, start a daemon, or implement a telemetry output profile.

## Build and run

Use Rust 1.95.0 with edition 2024. Node, DD and Builder are development tools,
not requirements of the SDK or installed CLI.

```sh
cargo build --locked --workspace
cargo run --locked -p unisphere-app -- config check --json
cargo install --locked --path crates/app --root target/install
./target/install/bin/unisphere config check --source-root ./sessions --json
```

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

Captured/piped output defaults to one JSON envelope plus a newline. Terminal
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

## Development and proof

- [SDK contract and examples](docs/sdk.md)
- [CLI envelope, modes and exits](docs/cli.md)
- [Toolchain selection, validation and isolation](docs/development.md)
- [Licence and reuse notes](THIRD_PARTY_NOTICES.md)

`harness checks --json` is the quality lane. `harness boot --json` additionally
exercises real SDK/CLI parity, an external SDK consumer, and a temporary installed
CLI under sealed runtime environments. These prove this configuration foundation,
not native telemetry collection. No-network assurance also includes independent
source/dependency inspection; it is not a runtime network-denial trace.

Local verification targets macOS ARM64 with the documented toolchain. Linux/macOS
CI configuration is provided; its presence alone is not an observed CI result.

MIT licensed; see [LICENSE](LICENSE).
