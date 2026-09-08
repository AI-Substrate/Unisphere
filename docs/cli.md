# Unisphere CLI

Unisphere provides configuration inspection plus an explicit Claude JSONL session
pipeline. Configuration-command JSON and OTLP telemetry JSONL are distinct output
contracts. No implicit HOME scan, daemon, remote service or persistent ingest store
is started. Session projection is not lossless or complete-session capture.

## Install and run

From a checkout with the documented Rust 1.95.0 toolchain:

```sh
cargo install --path crates/app --root ./local-install
./local-install/bin/unisphere --help --human
./local-install/bin/unisphere --version --json
./local-install/bin/unisphere config check --json
```

The executable is `unisphere`, packaged by `unisphere-app`. `unisphere-cli` is the
frontend library; it does not provide a second executable. See
[development.md](development.md) for the supported build/proof environment.
Node, DD, Builder, and a running agent harness are not runtime prerequisites.
The executable can run outside the checkout once installed. Automated platform
claims come from the development lane; a successful build alone does not prove
all platform-specific filesystem permissions or real-terminal behavior.

## Grammar

```text
unisphere config check [--config PATH]
    [--source-root ROOT ... | --clear-source-roots] [--json | --human]
unisphere [--json | --human] --help
unisphere [--json | --human] --version
```

`--source-root` is repeatable: use `--source-root first --source-root second`.
Use `--source-root=--literal` for a root beginning with a hyphen.
`--clear-source-roots` conflicts with `--source-root`. `--json` and `--human` are
global options and may appear before, between, or after subcommands; they
conflict. `--` ends option processing. `--help`/`-h` is available at each command
level; `--version`/`-V` returns the executable-supplied version. Missing/unknown
commands, missing option values, conflicting options and unexpected positional
arguments are invocation failures. Bare `unisphere` is not a configuration check.

## Configuration and precedence

An explicit configuration file is a JSON object with one optional key:

```json
{"source_roots":["relative-root","~/literal","/explicit/root"]}
```

`{}` and `{"source_roots":[]}` are valid. The default is an empty root list.
There is no implicit config-file discovery, environment override, or HOME/XDG
lookup. `--config PATH` selects the only file to read. Relative config paths are
joined to the working directory captured by the executable. Absolute paths are
passed through; the frontend does not canonicalize paths or check existence.
Missing, unreadable and oversized explicit files are failures, never defaults.

Precedence, shared with the SDK:

1. Defaults.
2. Present `source_roots` in the explicit JSON document.
3. Explicit CLI overrides. Repeated `--source-root` replaces the entire list;
   `--clear-source-roots` explicitly replaces it with `[]`.

Every supplied configuration layer is validated, including an invalid document
whose roots would otherwise be overridden. Roots must be strings whose trimmed
value is nonempty. Valid values are preserved exactly, including whitespace,
order and duplicates. Roots are not expanded, canonicalized, opened, or required
to exist. Unknown/duplicate keys, malformed or non-UTF8 JSON, non-object documents,
wrong value types, blank roots, and documents over 1,048,576 bytes are rejected.
Validation and explicit-file access belong to the injected SDK service, not a
second CLI parser.

A self-contained successful check without a file:

```sh
unisphere config check --source-root relative-root --source-root '~/literal' --json
```

```json
{"ok":true,"command":"config.check","v":1,"data":{"configuration":{"source_roots":["relative-root","~/literal"]}}}
```

A deterministic actionable failure without relying on filesystem permissions:

```sh
unisphere config check --source-root '   ' --json
```

This exits 1 with `error.kind` `invalid_configuration`, code `UNI-CONFIG-INVALID`,
a structural `source_roots[0]` field location when supplied by the service, and a
fix explaining the accepted configuration. Root values and whole documents are
not echoed into failures. File examples:

```sh
unisphere config check --config ./config.json --human
unisphere config check --config ./config.json --clear-source-roots --json
```

## Output contract: version 1

Captured/piped stdout defaults to machine mode; terminal stdout defaults to human
mode. Explicit `--json` or `--human` wins over terminal detection. No environment
variable changes this selection. If both mode flags are supplied, the invocation
fails and `--json` wins error rendering, regardless of flag order.

Machine output is exactly one JSON object followed by one LF on stdout, with no
ANSI decoration, log lines, banners, or stderr diagnostics on a successfully
written response. JSON object key order is not a compatibility promise. Consumers
must branch on `ok`, check `v`, and inspect the typed `error` rather than matching
human prose. Success omits `error`; failure omits `data`.

```json
{"ok":false,"command":"config.check","v":1,"error":{"kind":"invalid_arguments","code":"UNI-ARGS-INVALID","message":"The command arguments are invalid.","fix":"Use --help for accepted arguments; do not combine conflicting options.","retryable":false,"location":null}}
```

The command discriminators are:

| Operation | `command` | Successful `data` |
|---|---|---|
| Configuration inspection | `config.check` | `{"configuration":{"source_roots":[...]}}` |
| Help | `help` | `{"text":"..."}` |
| Version | `version` | `{"version":"0.1.0"}` |

Invocation failures use `command: "config.check"`. Help/version honor the selected
mode and do not invoke configuration inspection. Help prose and human layouts are
not machine-stable contracts.

The error object has `kind`, `code`, `message`, `fix`, `retryable`, and `location`:

| `kind` | `code` | Meaning |
|---|---|---|
| `invalid_configuration` | `UNI-CONFIG-INVALID` | Invalid explicit configuration or override |
| `configuration_read` | `UNI-CONFIG-READ` | Missing, unreadable, oversized or otherwise unreadable explicit file |
| `invalid_arguments` | `UNI-ARGS-INVALID` | Invalid invocation |

`retryable` is false in this foundation. `location` is either null or an object
with nullable `path`, `field`, `line`, and `column` fields. Locations supplied by
the service identify the file or structural field when known; arbitrary parser
or I/O error strings and hostile argument values are not forwarded. Supplied
file paths may appear in locations: avoid secrets in file names.

Human success/help/version goes to stdout. Human failure goes to stderr and
includes the stable code, fixed message, known location, and fix. Root strings
and location paths/fields use JSON string quoting to preserve values without
interpreting embedded newlines or terminal-control sequences.

| Exit | Meaning |
|---|---|
| 0 | Successful check, help, or version |
| 1 | Configuration, explicit-file I/O, or output I/O failure |
| 2 | Invalid command arguments |

An output write or flush failure takes precedence over these operation exits and
returns 1. When possible, stderr receives `unisphere: could not write output.`
A failed writer may contain a partial response; no second JSON object is appended
to repair it. Consumers must check process status and parse the complete result.

## Embedded frontend

`unisphere_cli::run` accepts argv including argv[0], `&CliContext`,
`&dyn unisphere_core::InspectionApi`, and caller-owned stdout/stderr writers, and
returns the exit code as `u8`. `CliContext` contains an absolute `cwd: PathBuf`,
`stdout_is_terminal: bool`, and `version: String`. The executable captures this
context and injects the real SDK inspector. The frontend reads no process-global
context, does not exit the process, and flushes but does not close writers.
A relative config argument with a non-absolute supplied cwd is an invocation
failure; it never falls back to the ambient working directory.

Frontend contract tests use only `unisphere-core` and `FakeInspector` from the
dev-only testkit. They prove requests, rendering, safe diagnostics and output
failure behavior without an SDK/app implementation. Real-file validation,
SDK/CLI parity, installation, and platform/runtime isolation require the separate
composed proof lane. No Flowspace3 or git-ai source was copied into this frontend.

## Session commands

```sh
unisphere sessions list --root /explicit/leaf-project --max-sessions 4096
unisphere sessions export --adapter claude-code --input /explicit/session.jsonl
unisphere sessions export --input /explicit/session.jsonl --include-content --output ./new.jsonl
```

Listing is nonrecursive, returns `recursive:false`, and reports an explanatory
diagnostic for an empty leaf directory. The initial file loader is Unix-only.
Export emits OTLP LogsData JSONL on stdout or creates a new file, never silently
overwriting one; summary and typed errors go to stderr. Help prints usage instead
of telemetry. Invalid arguments/limits exit 2, operational failures exit 1, success
exits 0. Output failures can leave a partial new file for the caller to inspect.

Metadata-only is the default, not anonymity: paths, native IDs, model and kind
metadata remain. `--include-content` enables supported message parts and tool I/O,
not opaque unknown payloads, sidecars or a full source archive.

Batch flags are `--max-records`, `--max-record-bytes` and `--max-batch-bytes`.
An oversize record errors at its starting byte and is not skipped; retry with
larger compatible limits. The summary reports mapped records, batches, total
diagnostics, `incomplete_tail` and final byte `offset`.

**Rerunning an export starts from the beginning.** There is no persisted resume,
`--harness`/session-ID resolution, source-record ordinal from/to range or idempotent
ingestion store. SDK callers can retain a source-bound `ReadCursor`; the CLI does
not persist it. EOF is only an observed boundary and delayed data can arrive later.
See [fidelity.md](fidelity.md) for implemented guarantees and follow-on work.
