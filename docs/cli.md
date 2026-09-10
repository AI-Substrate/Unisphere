# Unisphere CLI

Unisphere provides configuration inspection, a registered-adapter catalog,
explicit native session projections and read-only Git-ai-format Git Notes.
Catalog/configuration JSON and OTLP telemetry JSONL are distinct contracts.
No implicit HOME scan, daemon, remote service or persistent ingest store starts.
Projection is not lossless or complete-session capture.

## Install and run

From a checkout with the documented Rust 1.95.0 toolchain:

```sh
cargo install --path crates/app --root ./local-install
./local-install/bin/unisphere --help --human
./local-install/bin/unisphere --version --json
./local-install/bin/unisphere config check --json
./local-install/bin/unisphere adapters list --json
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
unisphere adapters list [--json | --human]
unisphere [--json | --human] --help
unisphere [--json | --human] --version
```

`--source-root` is repeatable: use `--source-root first --source-root second`.
Use `--source-root=--literal` for a root beginning with a hyphen.
`--clear-source-roots` conflicts with `--source-root`. For configuration commands,
`--json` and `--human` may appear before, between, or after subcommands. Catalog
mode flags follow the `adapters` token, before or after `list`. The flags conflict.
`--` ends option processing. `--help`/`-h` is available at each command level;
root `--version`/`-V` returns the executable-supplied version. Missing/unknown
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

Configuration and catalog commands share these mode and envelope conventions.
Session telemetry/diagnostic streams are described separately below.

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
| Registered-adapter catalog | `adapters.list` | `{"adapters":[...]}` |
| Help | `help` | `{"text":"..."}` |
| Version | `version` | `{"version":"0.1.0"}` |

Catalog invocation failures use `command: "adapters.list"`; configuration/root
invocation failures use `command: "config.check"`. Help/version honor the selected
mode and do not invoke configuration inspection or source collection. Help prose and human layouts are
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

`unisphere_cli::run_adapters` accepts argv including the binary name and `adapters`,
`&CliContext`, a borrowed `&[&AdapterDescriptor]` and caller-owned output writers.
The descriptor types live in `unisphere-core` and are re-exported by the CLI.
The function renders supplied data without acquiring a loader or inspecting the
environment, so embedding applications retain registration and discovery control.

## Registered-adapter catalog

```sh
unisphere adapters list --json
unisphere adapters list --human
```

The catalog contains registered production adapters only, never the test fixture.
The current IDs are `claude-code`, `codex`, `oh-my-pi`, `pi`, `copilot-cli`,
`cursor-transcript`, `vscode-copilot`, `cursor-ide`, `copilot-cli-snapshot` and `git-ai`.
Cursor transcript/IDE and Copilot event/legacy-snapshot dialects remain distinct.
The v1 envelope is `{"ok":true,"command":"adapters.list","v":1,"data":{"adapters":[...]}}`.
Each descriptor supplies:

| Field | Meaning |
| --- | --- |
| `id`, `application`, `description` | Stable selection/provenance ID, application name and projection scope. |
| `locations` | Usual-location hints, not detected installations or an inventory of existing sessions. |
| `locations[].platforms`, `base`, `path` | Applicable platform names and a relative path beneath a symbolic base such as `home`. |
| `locations[].session_glob`, `storage_format` | Pattern relative to the hinted path and native format; the catalog evaluates neither. |
| `capabilities.export_platforms`, `output_formats` | Registered pipeline support, distinct from location applicability. `unix` denotes the Unix family; other values name explicit platforms. Current writers emit `otlp-jsonl`. |
| `capabilities.sdk_caller_owned_cursor` | Whether the pipeline returns an append cursor; not a guarantee of safe resume after arbitrary rewrites. Snapshot revisions are a separate mechanism. |
| `capabilities.cursor_source_assumption` | The adapter's declared source assumption; append-only JSONL does not detect every rewrite/regrowth/truncation above a checkpoint. |
| `capabilities.cli_persisted_resume` | False: JSONL starts at byte zero and snapshots read a fresh complete revision each invocation. |
| `capabilities.delayed_revision_reconciliation`, `lossless_archive` | Stronger history/archive guarantees, distinct from exporting the current replacement projection; see [fidelity.md](fidelity.md). |

The usual Claude hint on macOS/Linux is symbolic `home` + `.claude/projects`,
pattern `*/*.jsonl`, native format `jsonl`. No HOME/config lookup, glob expansion,
source scan or arbitrary command execution occurs when listing the catalog.
Custom storage locations remain valid: callers choose the location, and pass an
explicit leaf directory to `sessions list --root` or file to `sessions export --input`.
The existing session listing is nonrecursive; the hint does not add automatic discovery.

Location bases use one lowercase vocabulary: `home` means the caller-chosen user
home; `appdata` means Windows roaming application data (usually the location named
by `APPDATA`). These are symbolic labels, never environment lookups performed by
the catalog. Each hint has one atomic `storage_format`: `jsonl`, `json_document`,
`json_journal` or `sqlite_key_value`. Snapshot formats correspond respectively to
CLI `--source-format json-document`, `json-journal` and `sqlite-key-value`;
`jsonl` uses the ordinary append-record runner. VS Code supplies separate `.json`
and `.jsonl` hints so a program need not infer a format from a combined pattern.

`delayed_revision_reconciliation` means retained cross-revision ingestion/state,
not pure journal replay or reading another current snapshot. It is false for the
current snapshot pipelines. VS Code's description names journal replay separately;
no descriptor promises a background tracker, persistent/idempotent sink or history.
The `git-ai` descriptor has no location hints: callers select a repository, not
a HOME-relative store. It declares Unix export, OTLP JSONL, no append cursor,
`pinned_git_notes_ref` source semantics, and no persisted resume, reconciliation
or lossless archive. Catalog listing does not probe either Git or Git AI.


Piped catalog output defaults to JSON; terminal output defaults to readable
descriptions, hints and capabilities. **Catalog JSON failures go to stdout;
session-command errors remain on stderr.** Human catalog failures go to stderr.
This deliberate stream distinction preserves the existing command contracts.

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

## Native revision snapshots

```sh
unisphere sessions export --adapter vscode-copilot --input /explicit/session.json
unisphere sessions export --adapter vscode-copilot --input /explicit/session.jsonl --source-format json-journal
unisphere sessions export --adapter copilot-cli-snapshot --input /explicit/legacy.json --include-content
unisphere sessions export --adapter cursor-ide --input /explicit/state.vscdb --session-id alpha
```

Snapshot adapters use `--max-records` (100,000), `--max-record-bytes` (32 MiB)
and `--max-snapshot-bytes` (64 MiB), not `--max-batch-bytes`. The aggregate bound
includes UTF-8 native keys plus raw value bytes; the record bound covers each raw
value. The unchanged OTLP writer additionally limits the complete encoded output
batch to 32 MiB. These are independent limits; a larger input budget does not
disable the output cap.

`--source-format` accepts `json-document`, `json-journal` or `sqlite-key-value`.
Defaults come from the registration: JSON document for VS Code/legacy Copilot;
SQLite `cursorDiskKV` for Cursor IDE. Select VS Code journals explicitly; file
extension alone does not choose a decoder. `--table` overrides a SQLite table
only, and must name real stored `key`/`value` columns. No caller SQL is executed.
The source must be an explicit absolute path or resolve under the captured cwd;
final symlinks, nonregular files and unsafe/inconsistent reads fail.

`--session-id` selects a native logical session within a supplied snapshot; it is
not global session discovery. The pure mapper verifies native identity. SQLite
reads one bounded read-only transaction; VS Code journal operations are reduced
before message projection. A partial/malformed journal never publishes a prefix
snapshot as success.

Every successful invocation emits a full projection followed by one closing
`unisphere.session.snapshot` manifest, including for an empty projection.
Snapshot records use native keys/content revisions and **no byte offset**.
The stderr summary names `revision`, `replace_projection`, unknown finality and
no persisted resume. `records` includes the manifest. Consumers accept the entire
output before replacing their prior projection; failed writes may leave bytes but
return no accepted SDK checkpoint. Repeated invocations are not deduplicated, and
no persistent history, destination transaction or exactly-once guarantee is added.

## Git Notes attribution

```sh
unisphere sessions list --adapter git-ai --repo /absolute/repository
unisphere sessions export --adapter git-ai --repo /absolute/repository --git-executable /usr/bin/git
unisphere sessions export --adapter git-ai --repo /absolute/repository --notes-ref refs/notes/ai --commit FULL_COMMIT_OID --include-content
```

Git AI is an input format only: no installation, executable invocation, crate,
library, cache, daemon or HTTP service from Git AI is needed. The app resolves
standard `git` from PATH unless `--git-executable` names an absolute trusted Git
binary. Help works without Git. Missing Git returns `git_unavailable`, not a
Git AI installation error.

`--repo` accepts an explicit local normal/bare repository or linked worktree,
including a nested directory; relative paths resolve under the captured cwd.
The default ref is `refs/notes/ai`. Only full `refs/notes/*` refs are accepted.
Repeat `--commit` for full lowercase 40/64-hex commit IDs; duplicates do not
duplicate observations. Omitting it selects all notes in the one requested ref,
not all refs or repository history. Tracking refs are never auto-aggregated.

Listing writes one JSON envelope on stdout with `data.adapter` and `data.listing`.
The listing contains canonical repository/common-dir/git-dir/worktree identities,
normalized selection, requested ref, nullable pinned ref tip and note references
with target commit and blob IDs. A missing valid ref is a normal empty result,
not evidence that no AI work occurred.

Export writes one complete bounded OTLP LogsData JSONL batch on stdout and a
structural summary on stderr. `--output FILE` instead creates a new file outside
the canonical source worktree, per-worktree Git directory and common Git directory.
Existing files/symlinks and symlink-parent aliases into those roots are rejected.
Output parents must be caller-controlled; concurrent malicious parent replacement
is not a filesystem sandbox guarantee. Shell redirection and SDK-supplied writers
remain caller-owned.

| Flag | Default | Hard ceiling |
| --- | --- | --- |
| `--max-notes` | 1,000 | 10,000 |
| `--max-records` | 10,000 including manifest | 100,000 |
| `--max-note-bytes` | 1 MiB | 32 MiB |
| `--max-total-bytes` | 16 MiB | 64 MiB |
| `--max-listing-bytes` | 1 MiB | 64 MiB |
| `--command-timeout-ms` | 5,000 per command | 60,000 |

All limits are positive; total bytes must cover the per-note budget. Selected
fanout lookup avoids enumerating unrelated notes; `All` retains a bounded listing.
The existing writer independently enforces 32 MiB of encoded output. Input size
does not imply encoded size. Overflows, timeout, malformed/unsupported notes and
read errors never produce a successful truncated projection. Write/flush failures
may leave partial destination bytes; discard them before retrying.

Metadata retains native paths, keys, declared agent identities and supplied
statistics. Human-author strings, custom attributes and legacy messages/URLs are
omitted unless `--include-content`; content is emitted once per declared identity,
not copied into every line-range event. URLs remain inert. Missing/null counts
are not zeros, and checkpoint IDs are never OpenTelemetry span IDs.

Errors use `command: "sessions"` on stderr with `error.kind`, fixed `message` and
nullable `output_code` for shared writer errors. Invalid arguments exit 2;
operational failures exit 1. Important kinds include `git_unavailable`,
`unsafe_repository`, `unsupported_repository`, `unsupported_target`, `object_read`,
`invalid_ref`, `unsupported_format`, `invalid_data` and the named budget failures.
Partial clones/promisor configuration are refused before object reads; inherited
Git configuration/helpers are cleared, protocols disabled and ownership checks
preserved. Global/system `safe.directory` entries are deliberately not inherited:
use a caller-owned checkout. No fetch, push, note/config/index/hook mutation occurs.

The source-specific parser is `unisphere_cli::run_git_notes`; it invokes an
app-owned constructor closure only after argument validation. Existing session
and snapshot frontend invocations are unchanged. This is not the broad session
exploration/query/extraction CLI, and it reads no retired harness telemetry.
