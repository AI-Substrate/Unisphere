# Unisphere

**Standardise agent-harness telemetry into one common format for easier integration and reporting.**

Unisphere is a **Rust SDK and CLI** that reads supported native session formats
and Git-ai-format Git Notes, projecting source facts to **OTLP LogsData JSONL**.
Use one SDK and output format instead of building a parser into every integration.

Adapters provide an extension path for other harnesses. That is the goal—not a
claim that every harness, version or field is already supported. Unisphere is not
a central database, hosted service or automatic background collector: you own the
transport, storage and reporting built around its output.

## How it fits

```text
Native session stores
        |
   bounded loaders       filesystem / read-only SQLite
        |
    pure adapters        supplied data -> typed records
        |
  common OTLP JSONL
        |
Your integrations, storage and reporting
```

Loaders own storage access. Adapters map supplied data without filesystem,
environment, network or clock access. The SDK composes those ports; the thin CLI
provides explicit-file exports. The same pipeline can be embedded in your own app.

## Installation

Build from a checkout with **Rust 1.95.0** and [just](https://just.systems/man/en/installation.html).
The filesystem/SQLite loaders support Unix; Ubuntu and macOS CI have passed.
Windows execution is not verified. Node, DD and Builder are development tooling,
not runtime requirements for the SDK or installed CLI.

```sh
git clone https://github.com/AI-Substrate/Unisphere.git
cd Unisphere
just install
```

`just install` builds from this checkout, reinstalls the CLI into
`$HOME/.local/bin/unisphere`, then runs its version and adapter-catalog smoke.
Run it again after updating the checkout. It uses Cargo's `--force` reinstall
option for the selected CLI; it does not copy a stale binary or link back to this
worktree. No sudo, shell-profile edits or toolchain changes are performed.

Ensure the matching `bin` directory is on your shell's PATH:

```sh
export PATH="$HOME/.local/bin:$PATH"
unisphere --version --json
```

Choose another prefix when needed; quote paths containing spaces:

```sh
just install "/path/to/prefix"
export PATH="/path/to/prefix/bin:$PATH"
```

Without `just`, the equivalent build/reinstall command is:

```sh
cargo install --locked --path crates/app --root "$HOME/.local" --force
```

Or run directly from the checkout without installing:

```sh
cargo run --locked -p unisphere-app -- adapters list --json
```

These instructions use the checked-out source, not an assumed crates.io package
or prebuilt release binary.

## CLI quick start

List the implemented adapters and their usual-location hints:

```sh
unisphere adapters list --json
```

Hints are metadata, **not detected installations or discovered sessions**. Choose
an explicit source yourself. JSONL listing examines one leaf directory only:

```sh
unisphere sessions list --root /absolute/path/to/project
unisphere sessions export --adapter claude-code --input /absolute/path/session.jsonl
```

Exports default to metadata-only OTLP JSONL on stdout. Include supported message
and tool content only when you intend to, and create a new output file explicitly:

```sh
unisphere sessions export --adapter claude-code --input /absolute/path/session.jsonl --include-content --output ./new-export.jsonl
```

Session summaries and errors stay on stderr; existing output files are not
silently overwritten. For snapshot sources, select the native representation:

```sh
unisphere sessions export --adapter vscode-copilot --input /absolute/path/session.jsonl --source-format json-journal
unisphere sessions export --adapter cursor-ide --input /absolute/path/state.vscdb
```

Git Notes attribution needs standard Git, not a Git AI installation:

```sh
unisphere sessions list --adapter git-ai --repo /absolute/repository
unisphere sessions export --adapter git-ai --repo /absolute/repository --git-executable /usr/bin/git
```

This reads an explicit local notes ref without modifying the source or fetching.
Metadata is the default; human strings/custom attributes/legacy messages require
`--include-content`. Attribution is not a complete conversation or token ledger.

See the [CLI reference](docs/cli.md) for limits, native session selectors,
configuration inspection, output envelopes and exit codes. For example,
`unisphere config check --json` inspects configuration without reading sessions.

## Supported harnesses

| Application | Adapter ID | Native representation |
| --- | --- | --- |
| Claude Code | `claude-code` | Session JSONL |
| Codex | `codex` | Rollout JSONL |
| Oh My Pi | `oh-my-pi` | Title-slot and session-entry JSONL |
| Pi | `pi` | v3 session-tree JSONL |
| GitHub Copilot CLI | `copilot-cli` | Event JSONL |
| GitHub Copilot CLI, legacy | `copilot-cli-snapshot` | Whole-session JSON |
| VS Code Copilot | `vscode-copilot` | Session JSON or native mutation journal |
| Cursor transcripts | `cursor-transcript` | Agent-transcript JSONL |
| Cursor IDE | `cursor-ide` | Read-only `cursorDiskKV` SQLite snapshot |
| Git-ai-format attribution | `git-ai` | Read-only commit-attached Git Notes; Git AI not required |

These are projections of documented native facts, not universal version coverage.
Cursor's opaque CLI blob store is not the IDE or transcript format and is not
semantically decoded. Each adapter's [fidelity report](docs/fidelity.md) names
preserved fields, omissions, source limitations and unsupported variants.

## Embed the Rust SDK

Use the SDK when collection belongs inside your integration rather than a shell
pipeline. This example exports an explicit Claude JSONL file through the same
loader, adapter and OTLP writer as the CLI.

For an `export-session` application beside the `Unisphere` checkout, use this
`Cargo.toml` (adjust all four paths if your layout differs):

```toml
[package]
name = "export-session"
version = "0.1.0"
edition = "2024"

[dependencies]
unisphere-sdk = { path = "../Unisphere/crates/sdk" }
unisphere-loader-jsonl = { path = "../Unisphere/crates/loader-jsonl" }
unisphere-adapter-claude = { path = "../Unisphere/crates/adapter-claude" }
unisphere-output-otlp = { path = "../Unisphere/crates/output-otlp" }
```

Put this in `src/main.rs`:

```rust
use unisphere_adapter_claude::ClaudeCodeAdapter;
use unisphere_loader_jsonl::FileSessionLoader;
use unisphere_output_otlp::OtlpJsonlWriter;
use unisphere_sdk::{CollectionApi, Collector, MappingOptions, ReadLimits, SessionRef};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = std::env::args_os()
        .nth(1)
        .ok_or("usage: export-session /absolute/path/session.jsonl")?;
    let source = SessionRef { path: input.into() };
    let collector = Collector::new(FileSessionLoader, ClaudeCodeAdapter, OtlpJsonlWriter);
    let mut destination = std::io::stdout().lock();
    let mut cursor = None;

    loop {
        let batch = collector.collect_batch(
            &source,
            cursor.as_ref(),
            ReadLimits::default(),
            MappingOptions::default(), // metadata-only
            &mut destination,
        )?;
        // Returned only after the writer accepts this batch.
        cursor = Some(batch.next_cursor);
        if !batch.more {
            if batch.incomplete_tail {
                eprintln!("Incomplete final record deferred; the source may append more data.");
            }
            break;
        }
    }
    Ok(())
}
```

Run it from the external application:

```sh
cargo run -- /absolute/path/session.jsonl > events.jsonl
```

The application owns the destination and checkpoint policy. This example keeps
its cursor in memory; it does not persist resume state or make stdout durable.
A write failure may leave partial bytes and returns no accepted checkpoint.
Use `MappingOptions { include_content: true }` only for intentional content export.
See [SDK usage](docs/sdk.md) for injected ports, configuration inspection and
`SnapshotCollector` for whole-source revisions.

## Common output and integration

Each output line is an OTLP `LogsData` JSON object. Native semantics that do not
fit a standard attribute are explicit `unisphere.*` extensions; they are not
invented provider facts or usage totals. The [telemetry profile](docs/telemetry-profile.md)
defines encoding, provenance and snapshot manifests.

Send the JSONL to your own importer, storage or reporting pipeline. Writing a file
or stdout is not an OTLP network export, central ingestion service or durable sink.
Unisphere deliberately leaves those choices to the caller.

## Privacy, fidelity and reruns

- Metadata-only is the default, **not anonymity**: paths, IDs, model and kind may remain.
- Content is opt-in; sidecars, attachments and referenced artifacts are not opened implicitly.
- JSONL CLI reruns start at byte zero. SDK byte cursors assume the documented source behavior.
- Snapshot exports describe a complete current replacement projection, not retained revision history.
- No persisted CLI resume, lossless raw archive, exactly-once ingestion or session-finality guarantee is made.

Read the [full fidelity assessment](docs/fidelity.md) before treating an export as
a complete transcript or aggregating native usage snapshots.

## Contributing and deeper documentation

Start with [CONTRIBUTING.md](CONTRIBUTING.md) for development, testing and adapter
contributions. Detailed references:

- [SDK APIs and examples](docs/sdk.md)
- [CLI commands and output contracts](docs/cli.md)
- [OTLP profile and native extensions](docs/telemetry-profile.md)
- [Fidelity and known limits](docs/fidelity.md)
- [Adapter authoring](docs/adapters.md)
- [JSONL loader](docs/session-loader.md) and [snapshot loader](docs/snapshot-loader.md)
- [Development toolchain and proof](docs/development.md)

MIT licensed: [LICENSE](LICENSE). See [third-party notices](THIRD_PARTY_NOTICES.md)
for provenance and reuse notes.
