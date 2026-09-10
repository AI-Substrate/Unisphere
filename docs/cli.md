# Unisphere CLI

Unisphere queries versioned local agent evidence through injected SDK ports and
retains explicit native OTLP export. It starts no daemon, remote service, model
service, index, or implicit HOME scan. Query output is not OTLP and snapshot
exports are current replacement projections, not persistent history or source
finality.

## Start offline

The installed binary carries version-matched operating guides and query schemas.
These routes run before configuration, source-provider, Git, or Git-AI setup:

```sh
unisphere --help --human
unisphere --version --json
unisphere docs list --json
unisphere docs get start --human
unisphere schema show sessions --json
```

Every completed command and every safe diagnostic supplies a nonempty next
action. JSON command envelopes use `next_action`. Human output labels `Next:`.
Raw JSONL, CSV, text, Markdown, and OTLP streams contain data only; their action
and coverage summary uses stderr.

## Root grammar

The root parser owns every command. The application dispatches its typed
`ParsedCommand`; it does not inspect raw tokens or choose a default adapter before
parsing.

```text
unisphere adapters list
unisphere config check
unisphere docs list
unisphere docs get TOPIC
unisphere schema show DATASET

unisphere sources list SCOPE [QUERY OPTIONS]
unisphere sources check --source SOURCE [QUERY OPTIONS]

unisphere sessions list SCOPE [QUERY OPTIONS]
unisphere sessions show SESSION SCOPE [QUERY OPTIONS]
unisphere sessions tree SESSION SCOPE [QUERY OPTIONS]
unisphere sessions stats SCOPE [QUERY OPTIONS]
unisphere sessions extract SCOPE [QUERY OPTIONS]
unisphere sessions export NATIVE-SOURCE [NATIVE OPTIONS]

unisphere turns list|stats|extract SCOPE [QUERY OPTIONS]
unisphere turns show TURN SCOPE [QUERY OPTIONS]
unisphere messages list|extract SCOPE [QUERY OPTIONS]
unisphere messages show MESSAGE SCOPE [QUERY OPTIONS]
unisphere tools list|stats|extract SCOPE [QUERY OPTIONS]
unisphere tools show CALL SCOPE [QUERY OPTIONS]
unisphere events list|extract SCOPE [QUERY OPTIONS]
unisphere events show EVENT SCOPE [QUERY OPTIONS]
```

A query scope is exactly one of:

```text
--repo PATH [--repo-scope exact|tree|worktrees]
--source PATH_OR_SOURCE_ID
--input FILE|-
--pij ID
```

Relative paths resolve lexically against the executable-supplied absolute working
directory. The frontend does not use ambient cwd or canonicalize a path while
parsing.

`--input -` reads bounded stdin as versioned query JSON by default. Add
`--stdin-format jsonl` for row-framed query JSONL; `--stdin-format json` is the
explicit default. The flag is rejected with file/live scopes. Unisphere never
sniffs stdin to guess framing. Saved files retain the loader's extension-based
selection.

`--pij ID` is optional and invokes one bounded `pij state ID --json` lookup through
the installed Pij CLI. It accepts live or retired seats with a recorded native
session, verifies that identity through registered local source decoding, and
then uses the ordinary SDK query view. For example:

```sh
unisphere sessions show --pij pij-example-seat --json
unisphere tools extract --pij pij-example-seat --tool-family shell --format jsonl
```

The example seat is a placeholder for your ID. Do not combine `--pij` with another
scope or native session/harness selector. The resolver does not use Pij's cwd hint
to relocate a query, fetch a remote transcript, start a daemon, or combine old
seat incarnations. It emits resolution provenance on stderr using canonical
hashed IDs; continuations use the pinned native source/session rather than
resolving the alias again. Missing Pij, unknown seat, missing recorded native ID,
unsupported protocol/harness and missing local transcript have distinct error
codes and concrete recovery. Ordinary native/query selectors do not require Pij.

Common query selectors include `--harness`, `--source-adapter`, `--session`,
`--native-id`, `--model`, `--name`, `--branch`, `--since`, `--until`,
`--time-field`, `--include-undated`, `--contains`, `--regex`, and
`--ignore-case`. Dataset selectors include turn/message/tool/event identity,
roles, tool names/families/status, duration and observed error fields. Different
fields combine with AND; repeated values in one field combine with OR. Unsupported
field/operation combinations fail with a schema recovery action instead of
returning a misleading empty set.

Projection options are `--columns`, `--sort`, `--limit`, `--cursor`,
`--include-content`, `--allow-partial`, and `--format`. `--format` accepts
`json`, `jsonl`, `csv`, `table`, `text`, or `markdown` only where the selected
dataset operation declares that capability. `--csv-safety spreadsheet` is the
default; `raw` retains potentially active spreadsheet prefixes. CSV collapses
projected absence, null, and empty strings and serializes compound values as JSON
text. Use JSON or JSONL where those distinctions matter.

`--json`, `--human`, and `--format` are mutually exclusive output selectors.
Piped query output defaults to JSON; terminal query output defaults to a table.
TTY status changes presentation only, never row selection.

## Native versus query sessions

These forms have deliberately different meanings:

```sh
# Existing bounded, nonrecursive native JSONL inventory
unisphere sessions list --root /explicit/leaf-project --max-sessions 4096

# Existing native Git-AI note inventory; the registered native adapter owns --repo
unisphere sessions list --adapter git-ai --repo /explicit/repository
unisphere sessions export --adapter git-ai --repo /explicit/repository --include-content

# Logical query; --repo is association scope and --source-adapter is a predicate
unisphere sessions list --repo . --harness claude-code --source-adapter claude-code --format json
unisphere sessions list --repo . --source-adapter git-ai --format json

# Native OTLP export is always native, never query extraction
unisphere sessions export --adapter claude-code --input /explicit/session.jsonl
```

`--adapter` has no query alias. Mixing `--root` or native `--adapter` with query
selectors is `UNI-CLI-ROUTE`; the parser names the three valid alternatives.
`sessions export` remains OTLP JSONL. Query extraction uses the SDK query schema
and never masquerades as `LogsData`.

Native Git Notes list/export also accepts `--notes-ref`, repeated `--commit`,
absolute `--git-executable`, `--max-notes`, `--max-records`,
`--max-note-bytes`, `--max-total-bytes`, `--max-listing-bytes`, and
`--command-timeout-ms`. Defaults remain `refs/notes/ai`, 1,000 notes, 10,000
records, 1 MiB per note/listing, 16 MiB total, and 5,000 ms. These options are
valid only with `--adapter git-ai --repo`; query `--source-adapter git-ai`
remains a representation predicate and does not invoke native Git Notes routing.

Native JSONL and snapshot export accept explicit create-new output paths. Existing
files are not overwritten. Native OTLP bytes stay on stdout or in the new output;
summaries and failures stay on stderr. Query file output is written to a private
same-directory staging file and published through create-new linking only after
serialization and flush complete. A failed stdout writer may contain a prefix;
the nonzero exit and stderr diagnostic state that it is incomplete.

## Privacy and content

Metadata-only output is the default, not anonymity. Names, bodies, commands,
arguments/results, reasoning, identity strings, and free-form native attributes
require the dataset's explicit content capability plus `--include-content`.
Search predicates authorize local inspection of the named content only; they do
not authorize emission. The CLI never inserts rejected raw argument values,
source payloads, or private paths into parse/query recovery text or generated
next actions. When an action requires a private path or original filter, it names
the required input separately instead of inventing or exposing a value.

## Configuration and adapter catalog

```sh
unisphere config check [--config PATH]
    [--source-root ROOT ... | --clear-source-roots] [--json | --human]
unisphere adapters list [--json | --human]
```

Configuration inspection retains defaults → explicit JSON document → explicit CLI
override precedence. It performs no implicit configuration discovery. Repeated
`--source-root` preserves order and duplicates; `--clear-source-roots` replaces
the list with `[]`. The adapter catalog renders only injected registered metadata;
location hints are not installation detection or source discovery.

## Exit and stream contract

| Exit | Meaning |
|---|---|
| 0 | Completed command, including help/version and a legitimate zero-match query |
| 1 | Source, query, serialization, publication, or destination failure |
| 2 | Invalid arguments or an invalid native/query route |

JSON command output is one versioned envelope ending in LF. Query JSONL is one
versioned row per line. CSV has one header and row stream. Text/Markdown and OTLP
remain clean data. Operational warnings, partial coverage, raw-stream summaries,
and next actions go to stderr. A diagnostic-channel failure never turns partial
data into success.

## Embedded frontend

`unisphere_cli::parse(Vec<OsString>, &CliContext)` returns:

```text
ParsedCommand::Config | Catalog | Docs | Schema | Query(QueryCommand)
  | NativeRootList | NativeGitNotesList | NativeExport | Help | Version
```

`QueryCommand` owns a validated `QueryRequest`, output format/CSV policy,
`stdin_format: SavedFormat`, and an optional absolute output path. `run_query`
accepts injected `&dyn QueryApi` and `&dyn QueryWriter` plus caller-owned
stdout/stderr writers. Matching,
reconstruction, privacy authorization, context, continuation, and statistics
remain in `QueryApi`; CLI serialization remains in `QueryWriter`.

Typed execution entrypoints are `run_config`, `run_catalog`, `run_help`,
`run_version`, `run_native_list`, `run_native_export`,
`run_native_snapshot_export`, `run_query`, `emit_query_failure`, `run_docs`, and
`run_schema`. `emit_query_failure` keeps initialization failures on the same safe
diagnostic channel without constructing a fake query response. Call
`diagnostic_mode(&args, stdout_is_terminal)` before moving argv into `parse` when
a parse failure must be rendered. These entrypoints consume parsed DTOs and never
inspect argv. The older `run`, `run_adapters`, `run_sessions`, and
`run_snapshot_sessions` APIs remain thin compatibility wrappers around the same
root parser and typed execution. New application composition parses once and
dispatches `ParsedCommand`; `requested_session_adapter` was removed with no
raw-argv compatibility shim.
